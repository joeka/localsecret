use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::fs::File;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use wait_timeout::ChildExt;

#[test]
fn secret_file_doesnt_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("localsecret")?;

    cmd.arg("--secret-file").arg("test/file/doesnt/exist");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("secret file doesn't exist"));

    Ok(())
}

#[test]
fn secret_file_can_be_retrieved_once() -> Result<(), Box<dyn std::error::Error>> {
    // Set up test file
    let dir = tempdir()?;
    let file_path = dir.path().join("test_file.txt");
    let mut file = File::create(&file_path)?;
    writeln!(file, "secret: 42")?;
    file.flush()?;

    // Start the command and web server
    let mut cmd = Command::cargo_bin("localsecret")?;
    let mut child = cmd
        .arg("--secret-file")
        .arg(file_path)
        .stdout(Stdio::piped())
        .spawn()?;

    // Read the stdout of the child process
    // This is where the URL will be printed
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut url = String::new();
    reader.read_line(&mut url)?;

    // Trim the URL to remove any trailing newline or whitespace
    let url = url.trim();

    // Check if the URL matches the expected pattern
    let url_predicate =
        predicate::str::is_match(r"^http://\d+\.\d+\.\d+\.\d+:\d+/[a-zA-Z0-9]{42}/test_file\.txt$")
            .unwrap();
    assert!(url_predicate.eval(url));

    // Get the content from the URL
    let response = reqwest::blocking::get(url)?;
    let body = response.text()?;

    // Check if the content matches the expected pattern
    let content_predicate = predicate::str::is_match(r"^secret: 42\n?$").unwrap();
    assert!(content_predicate.eval(&body));

    // Check if the URL is not reachable a second time
    let response = reqwest::blocking::get(url);
    assert!(
        response.is_err(),
        "the URL should't be reachable a 2nd time"
    );

    // Kill the process if it's still running
    match child.wait_timeout(Duration::from_secs(3))? {
        Some(exit_code) => assert_eq!(exit_code.code(), Some(0)),
        None => {
            child.kill()?;
            panic!("Process didn't terminate in time");
        }
    }
    Ok(())
}
