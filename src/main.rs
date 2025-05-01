use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use clap::Parser;
use local_ip_address::local_ip;
use rand::{Rng, distr::Alphanumeric};
use std::{path::PathBuf, process::exit};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::{self, time::sleep};
use tower_http::services::ServeFile;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[command(about = "Share secrets via a local http server", long_about = None)]
struct Args {
    #[arg(short, long, help = "The secret file to share")]
    secret_file: PathBuf,

    #[arg(
        long,
        default_value_t = 42,
        help = "Length of the randomly generated url prefix"
    )]
    url_prefix_length: u16,

    #[arg(
        long,
        default_value_t = 1,
        help = "How often the shared url can be used"
    )]
    uses: u16,

    #[arg(
        long,
        default_value_t = 3,
        help = "How some invalid url can be used before the server stops. Don't set this to 0, as browser e.g. try to fetch the favicon.ico file"
    )]
    failed_attempts: u16,
}

#[derive(Clone)]
struct AccessState {
    uses: Arc<tokio::sync::Mutex<u16>>,
    maximum_uses: u16,
}

#[derive(Clone)]
struct FailState {
    failed_attempts: Arc<tokio::sync::Mutex<u16>>,
    maximum_failed_attempts: u16,
}

async fn limit_uses(State(state): State<AccessState>, request: Request, next: Next) -> Response {
    let mut lock = state.uses.lock().await;
    if *lock >= state.maximum_uses {
        return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    }

    let response = next.run(request).await;

    *lock += 1;
    if *lock >= state.maximum_uses {
        tokio::spawn(async move {
            sleep(Duration::from_secs(1)).await;
            exit(0)
        });
    }

    response
}

async fn handler_404(State(state): State<FailState>) -> impl IntoResponse {
    let mut lock = state.failed_attempts.lock().await;
    *lock += 1;
    if *lock >= state.maximum_failed_attempts {
        exit(1);
    }
    (StatusCode::NOT_FOUND, "404 Not Found")
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
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

    let access_state = AccessState {
        uses: Arc::new(Mutex::new(0)),
        maximum_uses: args.uses,
    };
    let fail_state = FailState {
        failed_attempts: Arc::new(Mutex::new(0)),
        maximum_failed_attempts: args.failed_attempts,
    };

    let router = Router::new()
        .route_service(&file_url, ServeFile::new(absolute_path))
        .layer(middleware::from_fn_with_state(access_state, limit_uses))
        .fallback(handler_404)
        .with_state(fail_state);
    let listener = tokio::net::TcpListener::bind(format!("{}:0", local_address))
        .await
        .unwrap();
    println!("http://{}{}", listener.local_addr().unwrap(), &file_url);
    axum::serve(listener, router).await.unwrap();
}
