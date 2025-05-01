#[macro_use]
extern crate rocket;
use clap::Parser;
use local_ip_address::local_ip;
use rand::{Rng, distr::Alphanumeric};
use single_file_server::SingleFileServer;
use std::path::PathBuf;
mod single_file_server;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[command(about = "Share secrets via a local http server", long_about = None)]
struct Args {
    #[arg(short, long, help = "The secret file to share")]
    secret_file: PathBuf,

    #[arg(
        short,
        long,
        default_value_t = 42,
        help = "Length of the randomly generated url prefix"
    )]
    url_prefix_length: u16,

    #[arg(
        short,
        long,
        default_value_t = 1,
        help = "How often the shared url can be used"
    )]
    attempts: u16,
}

#[launch]
fn rocket() -> _ {
    let args = Args::parse();

    let file_path = args.secret_file;
    if !file_path.is_file() {
        eprintln!(
            "The provided secret file doesn't exist or is not a file: {:?}",
            file_path
        );
        std::process::exit(1);
    }
    let absolute_path = match file_path.canonicalize() {
        Ok(absolute_path) => absolute_path,
        Err(error) => {
            eprintln!(
                "Can't determine absolute path of '{:?}': {:#?}",
                file_path, error
            );
            std::process::exit(1);
        }
    };

    let file_name = match file_path.file_name() {
        Some(file_name) => match file_name.to_str() {
            Some(file_name) => file_name,
            None => {
                eprintln!("Can't decode file name: {:#?}", file_path);
                std::process::exit(1);
            }
        },
        None => {
            eprintln!("Can't determine file name from: {:#?}", file_path);
            std::process::exit(1);
        }
    };
    let random_prefix: String = rand::rng()
        .sample_iter(Alphanumeric)
        .take(usize::from(args.url_prefix_length))
        .map(char::from)
        .collect();
    let file_url = format!("/{}/{}", random_prefix, file_name);

    let local_address = match local_ip() {
        Ok(ip) => ip,
        Err(error) => {
            eprintln!("Can't determine local ip: {:#?}", error);
            std::process::exit(1);
        }
    };

    let figment = rocket::Config::figment()
        .merge(("address", local_address))
        .merge(("port", 0));

    rocket::custom(figment).mount(file_url, SingleFileServer::new(absolute_path))
}
