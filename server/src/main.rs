use crate::{
    aria2::Aria2Client,
    aria2_process::{Aria2Process, Aria2ProcessConfig},
    db::Database,
    state::AppState,
};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

mod aria2;
mod aria2_process;
mod config;
mod db;
mod error;
mod live_status;
mod poller;
mod routes;
mod scheduler;
mod state;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let server_config = config::load_or_create()?;

    let db_path = config::config_dir()?.join("ario.db");
    let database = Database::open(db_path.to_str().unwrap())?;

    let download_dir = config::expand_tilde(&server_config.aria2.download_dir)?;
    std::fs::create_dir_all(&download_dir)?;
    let data_dir = config::config_dir()?;

    let mut aria2_config = Aria2ProcessConfig::new_with_random_secret(data_dir, download_dir);
    aria2_config.binary_path = server_config.aria2.binary_path.clone();
    aria2_config.rpc_port = server_config.aria2.rpc_port;

    let aria2_client = Aria2Client::new(
        aria2_config.rpc_url(),
        Some(aria2_config.rpc_secret.clone()),
    );

    let aria2_process = Arc::new(Aria2Process::new(aria2_config));
    aria2_process.start().await?;
    tokio::spawn(Arc::clone(&aria2_process).supervise());

    let state = AppState::new(database, aria2_client, server_config);
    tokio::spawn(scheduler::run(state.clone()));
    tokio::spawn(poller::run(state.clone()));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(routes::downloads::router())
        .merge(routes::queues::router())
        .merge(routes::misc::router())
        .layer(cors)
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:47812").await?;
    println!("ario is running on http://localhost:47812");

    let shutdown_state = state.clone();
    let shutdown_process = Arc::clone(&aria2_process);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            println!("shutting down: stopping aria2c...");
            shutdown_process.shutdown(&shutdown_state.aria2).await;
        })
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
