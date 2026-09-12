//! TUI-owned supervision of a local `ario_daemon` process.

use std::net::{IpAddr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::api;
use crate::app::AppEvent;
use crate::event::Event;
use crate::toast::ToastLevel;

const DEFAULT_PORT: u16 = 47812;
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HEALTH_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const FAST_EXIT_THRESHOLD: Duration = Duration::from_secs(3);
const MAX_FAST_EXITS: u32 = 5;
const RESPAWN_BACKOFF: Duration = Duration::from_secs(2);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManagedServerTarget {
    pub host: IpAddr,
    pub port: u16,
}

impl ManagedServerTarget {
    pub fn api_base(self) -> String {
        format!("http://{}", std::net::SocketAddr::new(self.host, self.port))
    }
}

pub struct ServerProcessConfig {
    pub binary_path: String,
    pub api_base: String,
    pub target: ManagedServerTarget,
    pub log_path: PathBuf,
}

struct RunningChild {
    process: Child,
    started_at: Instant,
}

pub struct ServerProcess {
    config: ServerProcessConfig,
    child: Mutex<Option<RunningChild>>,
    spawn_lock: Mutex<()>,
    stop: AtomicBool,
    owns_process: AtomicBool,
    supervisor: Mutex<Option<JoinHandle<()>>>,
}

impl ServerProcess {
    pub fn new(config: ServerProcessConfig) -> Self {
        Self {
            config,
            child: Mutex::new(None),
            spawn_lock: Mutex::new(()),
            stop: AtomicBool::new(false),
            owns_process: AtomicBool::new(false),
            supervisor: Mutex::new(None),
        }
    }

    pub fn has_child(&self) -> bool {
        self.child.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn owns_process(&self) -> bool {
        self.owns_process.load(Ordering::SeqCst)
    }

    pub fn ensure_started(&self) -> anyhow::Result<()> {
        if api::health(&self.config.api_base).is_ok() {
            return Ok(());
        }

        let _spawn_guard = self.spawn_lock.lock().expect("server spawn lock poisoned");
        if api::health(&self.config.api_base).is_ok() {
            return Ok(());
        }
        if self.has_child() {
            if wait_until_healthy(&self.config.api_base) {
                return Ok(());
            }
            anyhow::bail!("ario_daemon is running but is not healthy");
        }

        let child = self.spawn_child_checked()?;
        self.owns_process.store(true, Ordering::SeqCst);
        *self.child.lock().expect("server child lock poisoned") = Some(RunningChild {
            process: child,
            started_at: Instant::now(),
        });

        if !wait_until_healthy(&self.config.api_base) {
            anyhow::bail!(
                "ario_daemon started but did not become healthy within {:?}",
                HEALTH_WAIT_TIMEOUT
            );
        }
        Ok(())
    }

    fn spawn_child(&self) -> std::io::Result<Child> {
        let log_out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.log_path)?;
        let log_err = log_out.try_clone()?;

        Command::new(&self.config.binary_path)
            .arg("--host")
            .arg(self.config.target.host.to_string())
            .arg("--port")
            .arg(self.config.target.port.to_string())
            .arg("--tui-managed")
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .stdin(Stdio::null())
            .spawn()
    }

    fn ensure_port_available(&self) -> std::io::Result<()> {
        match TcpListener::bind((self.config.target.host, self.config.target.port)) {
            Ok(listener) => {
                drop(listener);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "port {} is already in use on {}",
                    self.config.target.port, self.config.target.host
                ),
            )),
            Err(e) => Err(e),
        }
    }

    fn spawn_child_checked(&self) -> std::io::Result<Child> {
        self.ensure_port_available()?;
        self.spawn_child()
    }

    pub fn start_supervisor(self: &Arc<Self>, event_sender: Sender<Event>) {
        let mut slot = self.supervisor.lock().expect("supervisor lock poisoned");
        if slot.is_none() {
            self.stop.store(false, Ordering::SeqCst);
            let process = Arc::clone(self);
            *slot = Some(thread::spawn(move || process.supervise(event_sender)));
        }
    }

    fn notify(event_sender: &Sender<Event>, message: impl Into<String>) {
        let _ = event_sender.send(Event::App(AppEvent::Toast {
            message: message.into(),
            level: ToastLevel::Error,
        }));
    }

    fn supervise(&self, event_sender: Sender<Event>) {
        let mut consecutive_fast_exits = 0u32;
        let mut next_spawn = Instant::now();

        while !self.stop.load(Ordering::SeqCst) {
            let exited = {
                let mut child = self.child.lock().expect("server child lock poisoned");
                match child.as_mut() {
                    Some(running) => match running.process.try_wait() {
                        Ok(Some(status)) => {
                            let runtime = running.started_at.elapsed();
                            Self::notify(&event_sender, format!("ario_daemon exited: {status}"));
                            *child = None;
                            Some(runtime)
                        }
                        Ok(None) => None,
                        Err(e) => {
                            Self::notify(
                                &event_sender,
                                format!("ario_daemon try_wait() failed: {e}"),
                            );
                            *child = None;
                            Some(Duration::ZERO)
                        }
                    },
                    None => None,
                }
            };

            if let Some(runtime) = exited {
                consecutive_fast_exits = if runtime < FAST_EXIT_THRESHOLD {
                    consecutive_fast_exits + 1
                } else {
                    0
                };
                if consecutive_fast_exits >= MAX_FAST_EXITS {
                    Self::notify(
                        &event_sender,
                        format!(
                            "ario_daemon crash-looping — giving up on respawning. Check {}",
                            self.config.log_path.display()
                        ),
                    );
                    return;
                }
                next_spawn = Instant::now();
            }

            if !self.has_child()
                && Instant::now() >= next_spawn
                && api::health(&self.config.api_base).is_err()
            {
                let _spawn_guard = self.spawn_lock.lock().expect("server spawn lock poisoned");
                if !self.has_child() && !self.stop.load(Ordering::SeqCst) {
                    match self.spawn_child_checked() {
                        Ok(process) => {
                            self.owns_process.store(true, Ordering::SeqCst);
                            *self.child.lock().expect("server child lock poisoned") =
                                Some(RunningChild {
                                    process,
                                    started_at: Instant::now(),
                                });
                        }
                        Err(e) => {
                            Self::notify(
                                &event_sender,
                                format!("failed to respawn ario_daemon: {e}"),
                            );
                            next_spawn = Instant::now() + RESPAWN_BACKOFF;
                        }
                    }
                }
            }

            thread::sleep(HEALTH_POLL_INTERVAL);
        }
    }

    pub fn stop_supervisor(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self
            .supervisor
            .lock()
            .expect("supervisor lock poisoned")
            .take()
        {
            let _ = handle.join();
        }
    }

    pub fn detach(&self) {
        self.stop_supervisor();
        let _ = self
            .child
            .lock()
            .expect("server child lock poisoned")
            .take();
    }

    pub fn wait_for_shutdown(&self) {
        self.stop_supervisor();
        if !self.owns_process() {
            return;
        }
        let Some(mut child) = self
            .child
            .lock()
            .expect("server child lock poisoned")
            .take()
        else {
            return;
        };

        let deadline = Instant::now() + SHUTDOWN_WAIT;
        loop {
            match child.process.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.process.kill();
                    let _ = child.process.wait();
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(_) => {
                    let _ = child.process.kill();
                    return;
                }
            }
        }
    }
}

pub fn managed_server_target(
    auto_start_server: bool,
    server_url: &str,
) -> Option<ManagedServerTarget> {
    if !auto_start_server {
        return None;
    }
    let url = reqwest::Url::parse(server_url).ok()?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = match url.host_str()? {
        "localhost" => "127.0.0.1".parse().ok()?,
        value => value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .ok()?,
    };
    if !host.is_loopback() {
        return None;
    }
    let port = url.port().unwrap_or(DEFAULT_PORT);
    if port == 0 {
        return None;
    }
    Some(ManagedServerTarget { host, port })
}

pub fn resolve_binary_path(configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_string();
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(daemon_binary_name());
        if sibling.is_file() {
            return sibling.to_string_lossy().into_owned();
        }
    }
    daemon_binary_name().to_string()
}

fn daemon_binary_name() -> &'static str {
    if cfg!(windows) {
        "ario_daemon.exe"
    } else {
        "ario_daemon"
    }
}

fn wait_until_healthy(api_base: &str) -> bool {
    let deadline = Instant::now() + HEALTH_WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if api::health(api_base).is_ok() {
            return true;
        }
        thread::sleep(HEALTH_POLL_INTERVAL);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn test_process() -> Arc<ServerProcess> {
        Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: "/bin/false".into(),
            api_base: "http://127.0.0.1:9".into(),
            target: ManagedServerTarget {
                host: "127.0.0.1".parse().unwrap(),
                port: 9,
            },
            log_path: std::env::temp_dir().join("ario-supervisor-test.log"),
        }))
    }

    #[test]
    fn parses_manageable_loopback_urls() {
        assert_eq!(
            managed_server_target(true, "http://localhost:9999"),
            Some(ManagedServerTarget {
                host: "127.0.0.1".parse().unwrap(),
                port: 9999
            })
        );
        assert_eq!(
            managed_server_target(true, "http://[::1]:47813"),
            Some(ManagedServerTarget {
                host: "::1".parse().unwrap(),
                port: 47813
            })
        );
        assert_eq!(
            managed_server_target(true, "http://127.0.0.1")
                .unwrap()
                .port,
            DEFAULT_PORT
        );
        assert_eq!(
            managed_server_target(true, "http://127.0.0.1")
                .unwrap()
                .api_base(),
            "http://127.0.0.1:47812"
        );
        assert_eq!(
            managed_server_target(true, "http://[::1]:47813")
                .unwrap()
                .api_base(),
            "http://[::1]:47813"
        );
    }

    #[test]
    fn rejects_urls_that_must_be_externally_managed() {
        for url in [
            "https://localhost:47812",
            "http://example.com:47812",
            "http://192.168.1.5:47812",
            "http://localhost:47812/api",
            "http://user@localhost:47812",
            "not a url",
        ] {
            assert_eq!(managed_server_target(true, url), None, "{url}");
        }
        assert_eq!(managed_server_target(false, "http://localhost:47812"), None);
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_keeps_running_child_available_for_shutdown() {
        let process = test_process();
        let child = Command::new("/bin/sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .unwrap();
        *process.child.lock().unwrap() = Some(RunningChild {
            process: child,
            started_at: Instant::now(),
        });
        process.owns_process.store(true, Ordering::SeqCst);

        let (sender, _receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        thread::sleep(Duration::from_millis(250));
        assert!(process.has_child());

        process.stop_supervisor();
        let mut child = process.child.lock().unwrap().take().unwrap();
        child.process.kill().unwrap();
        child.process.wait().unwrap();
    }
}
