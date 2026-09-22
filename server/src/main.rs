use crate::{
    aria2::Aria2Client,
    aria2_process::{Aria2Process, Aria2ProcessConfig},
    db::Database,
    state::AppState,
};
use axum::Router;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    let launch = LaunchOptions::parse(std::env::args().skip(1))?;
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
    let supervisor = tokio::spawn(Arc::clone(&aria2_process).supervise());

    let aria2_global_options = load_aria2_global_options(&aria2_client).await;
    if aria2_global_options.is_none() {
        eprintln!(
            "warning: could not cache aria2 global options during startup; the TUI will use text fallbacks"
        );
    }

    let state = AppState::new(database, aria2_client, server_config, launch.tui_managed)
        .with_aria2_global_options(aria2_global_options);
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

    let listen_addr = SocketAddr::new(launch.host, launch.port);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    println!("ario is running on http://{listen_addr}");

    let shutdown_state = state.clone();
    let shutdown_process = Arc::clone(&aria2_process);
    let shutdown_notify = Arc::clone(&state.shutdown_notify);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal(shutdown_notify).await;
            println!("shutting down: stopping aria2c...");
            shutdown_process.shutdown(&shutdown_state.aria2).await;
            if let Err(e) = supervisor.await {
                eprintln!("aria2c supervisor failed: {e}");
            }
        })
        .await?;

    Ok(())
}

async fn load_aria2_global_options(
    client: &Aria2Client,
) -> Option<common::finetune::Aria2GlobalOptions> {
    use std::time::Duration;
    use tokio::time::{Instant, sleep, timeout};

    const MAX_WAIT: Duration = Duration::from_secs(2);
    const RETRY_DELAY: Duration = Duration::from_millis(50);

    let deadline = Instant::now() + MAX_WAIT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }

        match timeout(remaining, client.get_global_options()).await {
            Ok(Ok(options)) => return Some(options),
            Ok(Err(_)) => {}
            Err(_) => return None,
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        sleep(RETRY_DELAY.min(remaining)).await;
    }
}

async fn shutdown_signal(shutdown_notify: Arc<tokio::sync::Notify>) {
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
        _ = shutdown_notify.notified() => {},
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LaunchOptions {
    host: IpAddr,
    port: u16,
    tui_managed: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 47812,
            tui_managed: false,
        }
    }
}

impl LaunchOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--host" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--host requires a value"))?;
                    options.host = if value.eq_ignore_ascii_case("localhost") {
                        IpAddr::V4(Ipv4Addr::LOCALHOST)
                    } else {
                        value
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid --host: {value}"))?
                    };
                    if !options.host.is_loopback() {
                        anyhow::bail!("--host must be a loopback address");
                    }
                }
                "--port" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--port requires a value"))?;
                    options.port = value
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid --port: {value}"))?;
                    if options.port == 0 {
                        anyhow::bail!("--port must be greater than zero");
                    }
                }
                "--tui-managed" => options.tui_managed = true,
                _ => anyhow::bail!("unknown argument: {arg}"),
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn parses_custom_managed_listener() {
        let options = LaunchOptions::parse(
            ["--host", "::1", "--port", "49123", "--tui-managed"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.host, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(options.port, 49123);
        assert!(options.tui_managed);
    }

    #[test]
    fn rejects_non_loopback_and_unknown_arguments() {
        assert!(
            LaunchOptions::parse(["--host", "192.168.1.2"].into_iter().map(str::to_string))
                .is_err()
        );
        assert!(LaunchOptions::parse(["--wat"].into_iter().map(str::to_string)).is_err());
    }
}
