use super::*;
use std::fs::File;
use tempfile::tempdir;

#[test]
fn test_get_local_ip_with_bind_ip() {
    let bind_ip = Some("10.11.12.13".parse().unwrap());
    let result = get_local_ip(bind_ip);
    assert_eq!(result, "10.11.12.13".parse::<IpAddr>().unwrap());
}

#[test]
fn test_get_local_ip_without_bind_ip() {
    let bind_ip = None;
    let result = get_local_ip(bind_ip);
    assert!(result.is_ipv4() || result.is_ipv6());
    assert!(!result.is_loopback());
}

#[test]
fn test_validate_and_get_absolute_path_valid_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("test_file.txt");
    File::create(&file_path).unwrap();

    let absolute_path = validate_and_get_absolute_path(&file_path);
    assert_eq!(absolute_path, file_path.canonicalize().unwrap());
}

#[test]
fn test_generate_file_url_path() {
    let file_path = PathBuf::from("test_file.txt");
    let url_prefix_length = 8;
    let url_path = generate_file_url_path(&Some(file_path), url_prefix_length);

    assert!(url_path.starts_with('/'));
    assert!(url_path.ends_with("/test_file.txt"));
    assert_eq!(
        url_path.split('/').nth(1).unwrap().len(),
        url_prefix_length as usize
    );
}
