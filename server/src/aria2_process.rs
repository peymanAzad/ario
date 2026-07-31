use rand::RngExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::aria2::Aria2Client;

pub struct Aria2ProcessConfig {
    pub binary_path: String,
    pub rpc_port: u16,
    pub rpc_secret: String,
    pub session_path: PathBuf,
    pub log_path: PathBuf,
    pub download_dir: PathBuf,
}

impl Aria2ProcessConfig {
    pub fn new_with_random_secret(data_dir: PathBuf, download_dir: PathBuf) -> Self {
        let rpc_secret: String = {
            let mut rng = rand::rng();
            (0..32)
                .map(|_| format!("{:x}", rng.random_range(0..16u8)))
                .collect()
        };
        Self {
            binary_path: "aria2c".to_string(),
            rpc_port: 6800,
            rpc_secret,
            session_path: data_dir.join("aria2-session.txt"),
            log_path: data_dir.join("aria2.log"),
            download_dir,
        }
    }

    pub fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}/jsonrpc", self.rpc_port)
    }
}

pub struct Aria2Process {
    config: Aria2ProcessConfig,
    child: Mutex<Option<Child>>,
}

impl Aria2Process {
    pub fn new(config: Aria2ProcessConfig) -> Self {
        Self {
            config,
            child: Mutex::new(None),
        }
    }

    async fn spawn_child(&self) -> std::io::Result<Child> {
        let log_out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.log_path)?;
        let log_err = log_out.try_clone()?;

        if !self.config.session_path.exists() {
            std::fs::File::create(&self.config.session_path)?;
        }

        Command::new(&self.config.binary_path)
            .arg("--enable-rpc")
            .arg("--rpc-listen-all=false")
            .arg(format!("--rpc-listen-port={}", self.config.rpc_port))
            .arg(format!("--rpc-secret={}", self.config.rpc_secret))
            .arg(format!("--dir={}", self.config.download_dir.display()))
            .arg(format!(
                "--save-session={}",
                self.config.session_path.display()
            ))
            .arg("--save-session-interval=60")
            .arg(format!(
                "--input-file={}",
                self.config.session_path.display()
            ))
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .stdin(Stdio::null())
            .spawn()
    }

    fn ensure_port_available(&self) -> std::io::Result<()> {
        match std::net::TcpListener::bind(("127.0.0.1", self.config.rpc_port)) {
            Ok(listener) => {
                drop(listener);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "port {} is already in use — another aria2c (or unrelated \
                     process) is already listening there. Stop it, or set a \
                     different [aria2] rpc_port in server.toml.",
                    self.config.rpc_port
                ),
            )),
            Err(e) => Err(e),
        }
    }

    async fn spawn_child_checked(&self) -> std::io::Result<Child> {
        self.ensure_port_available()?;
        self.spawn_child().await
    }

    pub async fn start(&self) -> std::io::Result<()> {
        let child = self.spawn_child_checked().await?;
        *self.child.lock().await = Some(child);
        Ok(())
    }

    pub async fn supervise(self: Arc<Self>) {
        let mut consecutive_fast_exits = 0u32;

        loop {
            let maybe_child = self.child.lock().await.take();
            let mut child = match maybe_child {
                Some(c) => c,
                None => {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let started_at = Instant::now();
            match child.wait().await {
                Ok(status) => eprintln!("aria2c exited: {status}"),
                Err(e) => eprintln!("aria2c wait() failed: {e}"),
            }

            consecutive_fast_exits = if started_at.elapsed() < Duration::from_secs(3) {
                consecutive_fast_exits + 1
            } else {
                0
            };

            if consecutive_fast_exits >= 5 {
                eprintln!(
                    "aria2c crash-looping — giving up on respawning. Check {}",
                    self.config.log_path.display()
                );
                return;
            }

            match self.spawn_child_checked().await {
                Ok(new_child) => *self.child.lock().await = Some(new_child),
                Err(e) => {
                    eprintln!("failed to respawn aria2c: {e}");
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    pub async fn shutdown(&self, client: &Aria2Client) {
        let _ = client.shutdown().await; // best-effort; aria2 may already be gone

        if let Some(mut child) = self.child.lock().await.take() {
            let exited = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            if exited.is_err() {
                let _ = child.kill().await;
            }
        }
    }
}
