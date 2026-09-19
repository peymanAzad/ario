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
    lifecycle: Mutex<Lifecycle>,
}

struct RunningChild {
    child: Child,
    started_at: Instant,
}

#[derive(Default)]
struct Lifecycle {
    child: Option<RunningChild>,
    stopping: bool,
}

impl Aria2Process {
    pub fn new(config: Aria2ProcessConfig) -> Self {
        Self {
            config,
            lifecycle: Mutex::new(Lifecycle::default()),
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
        let mut state = self.lifecycle.lock().await;
        if state.stopping {
            return Err(std::io::Error::other("aria2c is stopping"));
        }
        let child = self.spawn_child_checked().await?;
        state.child = Some(RunningChild {
            child,
            started_at: Instant::now(),
        });
        Ok(())
    }

    pub async fn supervise(self: Arc<Self>) {
        let mut consecutive_fast_exits = 0u32;

        let mut next_spawn = Instant::now();
        loop {
            let mut state = self.lifecycle.lock().await;
            if state.stopping {
                return;
            }
            if let Some(running) = state.child.as_mut() {
                match running.child.try_wait() {
                    Ok(Some(status)) => {
                        eprintln!("aria2c exited: {status}");
                        let runtime = running.started_at.elapsed();
                        state.child = None;
                        consecutive_fast_exits = if runtime < Duration::from_secs(3) {
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
                        next_spawn = Instant::now() + Duration::from_secs(2);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("aria2c try_wait() failed: {e}");
                        state.child = None;
                        next_spawn = Instant::now() + Duration::from_secs(2);
                    }
                }
            } else if Instant::now() >= next_spawn {
                match self.spawn_child_checked().await {
                    Ok(child) => {
                        state.child = Some(RunningChild {
                            child,
                            started_at: Instant::now(),
                        })
                    }
                    Err(e) => {
                        eprintln!("failed to respawn aria2c: {e}");
                        next_spawn = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
            drop(state);
            sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn shutdown(&self, client: &Aria2Client) {
        let mut child = {
            let mut state = self.lifecycle.lock().await;
            state.stopping = true;
            state.child.take()
        };
        if let Some(ref mut running) = child {
            match tokio::time::timeout(Duration::from_millis(200), client.shutdown()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("aria2c shutdown RPC failed: {e}"),
                Err(_) => eprintln!("aria2c shutdown RPC timed out"),
            }
            match running.child.try_wait() {
                Ok(None) => terminate_child(&mut running.child),
                Ok(Some(_)) => {}
                Err(e) => eprintln!("aria2c try_wait() failed: {e}"),
            }
        }
    }
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            send_term(pid);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.start_kill();
    }
}

#[cfg(unix)]
fn send_term(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn false_binary() -> &'static str {
        ["/usr/bin/false", "/bin/false"]
            .into_iter()
            .find(|path| std::path::Path::new(path).is_file())
            .unwrap_or("false")
    }

    fn process(binary: &str) -> Arc<Aria2Process> {
        let dir = std::env::temp_dir().join(format!(
            "ario-aria2-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Aria2Process::new(Aria2ProcessConfig {
            binary_path: binary.into(),
            rpc_port: 0,
            rpc_secret: "test".into(),
            session_path: dir.join("session"),
            log_path: dir.join("log"),
            download_dir: dir,
        }))
    }

    fn client() -> Aria2Client {
        Aria2Client::new("http://127.0.0.1:1/jsonrpc", None)
    }

    async fn owned_sleep(process: &Aria2Process, seconds: &str) -> u32 {
        let child = Command::new("sleep").arg(seconds).spawn().unwrap();
        let pid = child.id().unwrap();
        process.lifecycle.lock().await.child = Some(RunningChild {
            child,
            started_at: Instant::now(),
        });
        pid
    }

    fn process_is_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn wait_until_reaped(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if !process_is_alive(pid) {
                return;
            }
            if Instant::now() >= deadline {
                panic!("process {pid} was not signaled");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[tokio::test]
    async fn shutdown_returns_quickly_and_blocks_respawn() {
        let process = process(false_binary());
        owned_sleep(&process, "30").await;
        let supervisor = tokio::spawn(Arc::clone(&process).supervise());
        let started = Instant::now();
        process.shutdown(&client()).await;
        supervisor.await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(400));
        assert!(process.lifecycle.lock().await.child.is_none());
        assert!(process.start().await.is_err());
    }

    #[tokio::test]
    async fn shutdown_signals_owned_child_without_waiting() {
        let process = process(false_binary());
        let pid = owned_sleep(&process, "30").await;
        let started = Instant::now();
        process.shutdown(&client()).await;
        assert!(started.elapsed() < Duration::from_millis(400));
        assert!(process.lifecycle.lock().await.child.is_none());
        wait_until_reaped(pid);
    }

    #[tokio::test]
    async fn failed_respawn_is_retried_after_backoff() {
        let mut process = process(false_binary());
        let script_path = process.config.log_path.with_file_name("test-aria2c");
        Arc::get_mut(&mut process).unwrap().config.binary_path =
            script_path.to_string_lossy().into_owned();
        let supervisor = tokio::spawn(Arc::clone(&process).supervise());
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(process.lifecycle.lock().await.child.is_none());
        let script = std::path::Path::new(&process.config.binary_path);
        std::fs::write(script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(process.lifecycle.lock().await.child.is_some());
        process.shutdown(&client()).await;
        supervisor.await.unwrap();
        std::fs::remove_file(script).unwrap();
    }

    #[tokio::test]
    async fn five_fast_exits_stop_supervision() {
        let process = process(false_binary());
        process.start().await.unwrap();
        let supervisor = tokio::spawn(Arc::clone(&process).supervise());
        tokio::time::timeout(Duration::from_secs(11), supervisor)
            .await
            .unwrap()
            .unwrap();
        assert!(process.lifecycle.lock().await.child.is_none());
        process.shutdown(&client()).await;
    }
}
