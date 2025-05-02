use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use clap::{CommandFactory, Parser};
use local_ip_address::local_ip;
use rand::{Rng, distr::Alphanumeric};
use std::sync::Arc;
use std::{
    io::{self, IsTerminal, Read},
    process::exit,
};
use std::{net::IpAddr, path::PathBuf};
use tokio::{
    self, signal,
    sync::{Mutex, mpsc},
};
use tower_http::services::ServeFile;

#[derive(Parser, Debug)]
#[command(version, about = "Share secrets via a local http server", long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        help = "The secret file to share. If not set, expects the input to be piped to stdin"
    )]
    secret_file: Option<PathBuf>,

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

    #[arg(
        long,
        help = "IP address to bind the server to. If not set, will try to find the local IP address"
    )]
    bind_ip: Option<IpAddr>,
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

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = Args::parse();

    let mut stdin = io::stdin();
    let input_from_stdin = !stdin.is_terminal();

    let file_url_path = generate_file_url_path(&args.secret_file, args.url_prefix_length);

    let (shutdown_sender, shutdown_receiver) = mpsc::channel(16);
    let access_state = AccessState {
        uses: Arc::new(Mutex::new(0)),
        maximum_uses: args.uses,
        shutdown_channel: shutdown_sender.clone(),
    };
    let fail_state = FailState {
        failed_attempts: Arc::new(Mutex::new(0)),
        maximum_failed_attempts: args.failed_attempts,
        shutdown_channel: shutdown_sender,
    };

    let router = match args.secret_file {
        Some(file_path) => {
            let absolute_path = validate_and_get_absolute_path(&file_path);
            Router::new().route_service(&file_url_path, ServeFile::new(absolute_path))
        }
        None => {
            if !input_from_stdin {
                Args::command().print_help().unwrap();
                eprintln!("Please provide a secret file to share or pipe the secret to stdin");
                exit(1);
            }
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer).unwrap();
            Router::new().route(&file_url_path, get(|| async { buffer }))
        }
    }
    .layer(middleware::from_fn_with_state(access_state, limit_uses))
    .fallback(handler_404)
    .with_state(fail_state);

    let local_address = get_local_ip(args.bind_ip);
    let listener = create_listener(local_address).await;

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

async fn create_listener(local_address: IpAddr) -> tokio::net::TcpListener {
    match tokio::net::TcpListener::bind(format!("{}:0", local_address)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Can't bind to local address: {:#?}", error);
            std::process::exit(1);
        }
    }
}

fn get_local_ip(bind_ip: Option<IpAddr>) -> IpAddr {
    match bind_ip {
        Some(ip) => ip,
        None => match local_ip() {
            Ok(ip) => ip,
            Err(error) => {
                eprintln!("Can't determine local ip: {:#?}", error);
                std::process::exit(1);
            }
        },
    }
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

fn generate_file_url_path(file_path: &Option<PathBuf>, url_prefix_length: u16) -> String {
    let random_prefix: String = rand::rng()
        .sample_iter(Alphanumeric)
        .take(usize::from(url_prefix_length))
        .map(char::from)
        .collect();
    match file_path {
        Some(file_path) => {
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

            format!("/{}/{}", random_prefix, file_name)
        }
        None => format!("/{}", random_prefix),
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

#[cfg(test)]
mod tests;
