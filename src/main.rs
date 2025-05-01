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
use std::path::PathBuf;
use std::sync::Arc;
use tokio::{
    self, signal,
    sync::{Mutex, mpsc},
};
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
    shutdown_channel: mpsc::Sender<()>,
}

#[derive(Clone)]
struct FailState {
    failed_attempts: Arc<tokio::sync::Mutex<u16>>,
    maximum_failed_attempts: u16,
    shutdown_channel: mpsc::Sender<()>,
}

async fn limit_uses(State(state): State<AccessState>, request: Request, next: Next) -> Response {
    let mut lock = state.uses.lock().await;
    if *lock >= state.maximum_uses {
        // If the maximum number of uses is reached, return a 404 response
        // this should never happen, as the server should be stopped before this
        return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
    }

    let response = next.run(request).await;

    *lock += 1;
    if *lock >= state.maximum_uses {
        // If the maximum number of uses is reached, send a shutdown signal
        state.shutdown_channel.send(()).await.unwrap();
    }

    response
}

async fn handler_404(State(state): State<FailState>) -> impl IntoResponse {
    let mut lock = state.failed_attempts.lock().await;
    *lock += 1;
    if *lock >= state.maximum_failed_attempts {
        // If the maximum number of failed attempts is reached, send a shutdown signal
        // this happens when the user tries to access path other than the shared file
        state.shutdown_channel.send(()).await.unwrap();
    }
    (StatusCode::NOT_FOUND, "404 Not Found")
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = Args::parse();

    let absolute_path = validate_and_get_absolute_path(&args.secret_file);
    let file_url_path = generate_file_url_path(&args.secret_file, args.url_prefix_length);

    let local_address = match local_ip() {
        Ok(ip) => ip,
        Err(error) => {
            eprintln!("Can't determine local ip: {:#?}", error);
            std::process::exit(1);
        }
    };

    let (shutdown_sender, shutdown_receiver) = mpsc::channel(16);

    let access_state = create_access_state(args.uses, shutdown_sender.clone());
    let fail_state = create_fail_state(args.failed_attempts, shutdown_sender);

    let router = {
        let file_url = file_url_path.clone();
        Router::new()
            .route_service(&file_url, ServeFile::new(absolute_path))
            .layer(middleware::from_fn_with_state(access_state, limit_uses))
            .fallback(handler_404)
            .with_state(fail_state)
    };
    let listener = tokio::net::TcpListener::bind(format!("{}:0", local_address))
        .await
        .unwrap();

    println!(
        "http://{}{}",
        listener.local_addr().unwrap(),
        &file_url_path
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal(shutdown_receiver))
        .await
        .unwrap();
}

fn validate_and_get_absolute_path(file_path: &PathBuf) -> PathBuf {
    if !file_path.is_file() {
        eprintln!(
            "The provided secret file doesn't exist or is not a file: {:?}",
            file_path
        );
        std::process::exit(1);
    }
    match file_path.canonicalize() {
        Ok(absolute_path) => absolute_path,
        Err(error) => {
            eprintln!(
                "Can't determine absolute path of '{:?}': {:#?}",
                file_path, error
            );
            std::process::exit(1);
        }
    }
}

fn generate_file_url_path(file_path: &PathBuf, url_prefix_length: u16) -> String {
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
        .take(usize::from(url_prefix_length))
        .map(char::from)
        .collect();
    format!("/{}/{}", random_prefix, file_name)
}

fn create_access_state(maximum_uses: u16, shutdown_channel: mpsc::Sender<()>) -> AccessState {
    AccessState {
        uses: Arc::new(Mutex::new(0)),
        maximum_uses,
        shutdown_channel,
    }
}

fn create_fail_state(
    maximum_failed_attempts: u16,
    shutdown_channel: mpsc::Sender<()>,
) -> FailState {
    FailState {
        failed_attempts: Arc::new(Mutex::new(0)),
        maximum_failed_attempts,
        shutdown_channel,
    }
}

async fn shutdown_signal(mut shutdown_receiver: mpsc::Receiver<()>) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
        _ = shutdown_receiver.recv() => {},
    }
}
