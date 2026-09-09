//! TUI-owned supervision of the local `ario_daemon` process.
//! Mirrors the server's aria2 supervisor: wait on exit, respawn with a
//! crash-loop guard, and only shut down a child we spawned.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::api;

const DEFAULT_PORT: u16 = 47812;
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HEALTH_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const FAST_EXIT_THRESHOLD: Duration = Duration::from_secs(3);
const MAX_FAST_EXITS: u32 = 5;
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

pub struct ServerProcessConfig {
    pub binary_path: String,
    pub api_base: String,
    pub port: u16,
    pub log_path: PathBuf,
}

pub struct ServerProcess {
    config: ServerProcessConfig,
    child: Mutex<Option<Child>>,
    stop: AtomicBool,
    /// True once we have successfully spawned at least one child this session.
    /// Used so shutdown only kills processes we own.
    owns_process: AtomicBool,
}

impl ServerProcess {
    pub fn new(config: ServerProcessConfig) -> Self {
        Self {
            config,
            child: Mutex::new(None),
            stop: AtomicBool::new(false),
            owns_process: AtomicBool::new(false),
        }
    }

    pub fn has_child(&self) -> bool {
        self.child.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// If the server is already healthy, do nothing. Otherwise spawn a daemon
    /// (when the port is free) and wait briefly for `/health`.
    pub fn ensure_started(&self) -> anyhow::Result<()> {
        if api::health(&self.config.api_base).is_ok() {
            return Ok(());
        }

        {
            let guard = self.child.lock().expect("server child lock poisoned");
            if guard.is_some() {
                // Child is running but not healthy yet — give it a moment.
                drop(guard);
                let _ = wait_until_healthy(&self.config.api_base);
                return Ok(());
            }
        }

        let child = self.spawn_child_checked()?;
        self.owns_process.store(true, Ordering::SeqCst);
        *self.child.lock().expect("server child lock poisoned") = Some(child);

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
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err))
            .stdin(Stdio::null())
            .spawn()
    }

    fn ensure_port_available(&self) -> std::io::Result<()> {
        match TcpListener::bind(("127.0.0.1", self.config.port)) {
            Ok(listener) => {
                drop(listener);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "port {} is already in use — another ario_daemon (or unrelated \
                     process) is already listening there.",
                    self.config.port
                ),
            )),
            Err(e) => Err(e),
        }
    }

    fn spawn_child_checked(&self) -> std::io::Result<Child> {
        self.ensure_port_available()?;
        self.spawn_child()
    }

    pub fn supervise(self: Arc<Self>) {
        let mut consecutive_fast_exits = 0u32;

        loop {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }

            let maybe_child = self.child.lock().expect("server child lock poisoned").take();
            let mut child = match maybe_child {
                Some(c) => c,
                None => {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };

            let started_at = Instant::now();
            match child.wait() {
                Ok(status) => eprintln!("ario_daemon exited: {status}"),
                Err(e) => eprintln!("ario_daemon wait() failed: {e}"),
            }

            if self.stop.load(Ordering::SeqCst) {
                return;
            }

            consecutive_fast_exits = if started_at.elapsed() < FAST_EXIT_THRESHOLD {
                consecutive_fast_exits + 1
            } else {
                0
            };

            if consecutive_fast_exits >= MAX_FAST_EXITS {
                eprintln!(
                    "ario_daemon crash-looping — giving up on respawning. Check {}",
                    self.config.log_path.display()
                );
                return;
            }

            match self.spawn_child_checked() {
                Ok(new_child) => {
                    self.owns_process.store(true, Ordering::SeqCst);
                    *self.child.lock().expect("server child lock poisoned") = Some(new_child);
                }
                Err(e) => {
                    eprintln!("failed to respawn ario_daemon: {e}");
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);

        if !self.owns_process.load(Ordering::SeqCst) {
            return;
        }

        let Some(mut child) = self.child.lock().expect("server child lock poisoned").take() else {
            return;
        };

        terminate_child(&mut child);

        let deadline = Instant::now() + SHUTDOWN_WAIT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(_) => {
                    let _ = child.kill();
                    return;
                }
            }
        }
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
}

pub fn is_loopback_url(url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")
}

pub fn port_from_url(url: &str) -> u16 {
    url_port(url).unwrap_or(DEFAULT_PORT)
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

pub fn should_manage(auto_start_server: bool, server_url: &str) -> bool {
    auto_start_server && is_loopback_url(server_url)
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

fn url_host(url: &str) -> Option<String> {
    // Minimal parse: scheme://host[:port][/...]
    let rest = url.split("://").nth(1)?;
    let authority = rest.split('/').next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, host_port)| host_port)
        .unwrap_or(authority);
    if let Some(host) = host.strip_prefix('[') {
        // IPv6 literal [::1]:port
        return host.split(']').next().map(str::to_string);
    }
    Some(host.split(':').next()?.to_string())
}

fn url_port(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split('/').next()?;
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, host_port)| host_port)
        .unwrap_or(authority);
    if host_port.starts_with('[') {
        // [::1]:port
        let after = host_port.split("]:").nth(1)?;
        return after.parse().ok();
    }
    let port = host_port.splitn(2, ':').nth(1)?;
    port.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_url("http://127.0.0.1:47812"));
        assert!(is_loopback_url("http://localhost:47812"));
        assert!(is_loopback_url("http://[::1]:47812"));
        assert!(!is_loopback_url("http://example.com:47812"));
        assert!(!is_loopback_url("http://192.168.1.5:47812"));
    }

    #[test]
    fn port_parsing() {
        assert_eq!(port_from_url("http://127.0.0.1:47812"), 47812);
        assert_eq!(port_from_url("http://127.0.0.1"), DEFAULT_PORT);
        assert_eq!(port_from_url("http://[::1]:9999"), 9999);
    }
}
