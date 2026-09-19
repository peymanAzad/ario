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
const SPAWN_ERROR_NOTICE_INTERVAL: Duration = Duration::from_secs(10);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

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

    fn lifecycle(event_sender: &Sender<Event>, state: crate::app::LifecycleState) {
        let _ = event_sender.send(Event::App(AppEvent::Lifecycle(state)));
    }

    fn notify(event_sender: &Sender<Event>, message: impl Into<String>) {
        let _ = event_sender.send(Event::App(AppEvent::Toast {
            message: message.into(),
            level: ToastLevel::Error,
        }));
    }

    fn supervise(&self, event_sender: Sender<Event>) {
        use crate::app::LifecycleState;
        let mut consecutive_fast_exits = 0u32;
        let mut next_spawn = Instant::now();
        let mut readiness_deadline: Option<Instant> = None;
        let mut readiness_reported = false;
        let mut connected = false;
        let mut last_spawn_error: Option<Instant> = None;
        Self::lifecycle(&event_sender, LifecycleState::Starting);

        while !self.stop.load(Ordering::SeqCst) {
            let exited = {
                let mut slot = self.child.lock().expect("server child lock poisoned");
                match slot.as_mut() {
                    Some(running) => match running.process.try_wait() {
                        Ok(Some(status)) => {
                            let runtime = running.started_at.elapsed();
                            *slot = None;
                            Some((
                                runtime,
                                format!(
                                    "ario_daemon exited: {status}. Check {}",
                                    self.config.log_path.display()
                                ),
                            ))
                        }
                        Ok(None) => None,
                        Err(e) => {
                            *slot = None;
                            Some((
                                Duration::ZERO,
                                format!(
                                    "ario_daemon try_wait() failed: {e}. Check {}",
                                    self.config.log_path.display()
                                ),
                            ))
                        }
                    },
                    None => None,
                }
            };
            if let Some((runtime, message)) = exited {
                Self::notify(&event_sender, message.clone());
                connected = false;
                readiness_deadline = None;
                consecutive_fast_exits = if runtime < FAST_EXIT_THRESHOLD {
                    consecutive_fast_exits + 1
                } else {
                    0
                };
                if consecutive_fast_exits >= MAX_FAST_EXITS {
                    Self::lifecycle(
                        &event_sender,
                        LifecycleState::Failed(format!(
                            "ario_daemon crash-looping. Check {}",
                            self.config.log_path.display()
                        )),
                    );
                    return;
                }
                Self::lifecycle(&event_sender, LifecycleState::Retrying);
                next_spawn = Instant::now() + RESPAWN_BACKOFF;
            }

            // Health is checked even with an owned child so slow readiness can recover
            // after the first five-second diagnostic.
            let healthy = api::health(&self.config.api_base).is_ok();
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if healthy {
                if !connected {
                    Self::lifecycle(&event_sender, LifecycleState::Connected);
                }
                connected = true;
                readiness_deadline = None;
                readiness_reported = false;
            } else {
                if connected {
                    connected = false;
                    Self::lifecycle(&event_sender, LifecycleState::Retrying);
                }
                if let Some(deadline) = readiness_deadline {
                    if Instant::now() >= deadline && !readiness_reported {
                        let message = format!(
                            "ario_daemon did not become healthy within five seconds. Check {}",
                            self.config.log_path.display()
                        );
                        Self::lifecycle(&event_sender, LifecycleState::Failed(message.clone()));
                        Self::notify(&event_sender, message);
                        readiness_reported = true;
                    }
                }
            }

            if !healthy && !self.has_child() && Instant::now() >= next_spawn {
                let _spawn_guard = self.spawn_lock.lock().expect("server spawn lock poisoned");
                if self.stop.load(Ordering::SeqCst) {
                    return;
                }
                if !self.has_child() {
                    // The port may belong to a daemon whose health endpoint is still
                    // coming up; the check prevents a duplicate owned process.
                    match self.spawn_child_checked() {
                        Ok(process) => {
                            self.owns_process.store(true, Ordering::SeqCst);
                            *self.child.lock().expect("server child lock poisoned") =
                                Some(RunningChild {
                                    process,
                                    started_at: Instant::now(),
                                });
                            readiness_deadline = Some(Instant::now() + HEALTH_WAIT_TIMEOUT);
                            readiness_reported = false;
                            Self::lifecycle(&event_sender, LifecycleState::Starting);
                        }
                        Err(e) => {
                            let now = Instant::now();
                            if last_spawn_error.is_none_or(|last| {
                                now.duration_since(last) >= SPAWN_ERROR_NOTICE_INTERVAL
                            }) {
                                Self::notify(
                                    &event_sender,
                                    format!(
                                        "failed to start ario_daemon: {e}. Check {}",
                                        self.config.log_path.display()
                                    ),
                                );
                                last_spawn_error = Some(now);
                            }
                            Self::lifecycle(&event_sender, LifecycleState::Retrying);
                            next_spawn = now + RESPAWN_BACKOFF;
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

    pub fn terminate_owned(&self) {
        self.stop_supervisor();
        if !self.owns_process() {
            return;
        }
        if let Some(running) = self
            .child
            .lock()
            .expect("server child lock poisoned")
            .as_mut()
        {
            signal_terminate(&mut running.process);
        }
        self.wait_for_shutdown();
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
                Err(e) => {
                    eprintln!("ario_daemon wait failed: {e}; killing it");
                    let _ = child.process.kill();
                    let _ = child.process.wait();
                    return;
                }
            }
        }
    }
}

fn signal_terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LifecycleState;
    use std::sync::atomic::AtomicU64;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[cfg(unix)]
    fn isolated_process(binary: &str, api_base: String) -> Arc<ServerProcess> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let log_path = std::env::temp_dir().join(format!(
            "ario-tui-test-{}-{}.log",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: binary.into(),
            api_base,
            target: ManagedServerTarget {
                host: "127.0.0.1".parse().unwrap(),
                port,
            },
            log_path,
        }))
    }

    #[cfg(unix)]
    fn fake_health(ready: Arc<AtomicBool>) -> (String, Arc<AtomicBool>, JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let handle = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0u8; 1024];
                        let _ = stream.read(&mut buffer);
                        let body = r#"{"server":"ok","aria2_reachable":true,"tui_managed":true}"#;
                        let status = if ready.load(Ordering::SeqCst) {
                            "200 OK"
                        } else {
                            "503 Service Unavailable"
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => return,
                }
            }
        });
        (base, stop, handle)
    }

    #[cfg(unix)]
    fn clean_child(process: &ServerProcess) {
        process.stop_supervisor();
        if let Some(mut child) = process.child.lock().unwrap().take() {
            let _ = child.process.kill();
            let _ = child.process.wait();
        }
    }

    #[cfg(unix)]
    fn false_binary() -> &'static str {
        ["/usr/bin/false", "/bin/false"]
            .into_iter()
            .find(|path| std::path::Path::new(path).is_file())
            .unwrap_or("false")
    }

    fn test_process() -> Arc<ServerProcess> {
        Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: false_binary().into(),
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

    #[cfg(unix)]
    #[test]
    fn immediate_exit_reports_status_and_retries_after_backoff() {
        let process = isolated_process(false_binary(), "http://127.0.0.1:1".into());
        let (sender, receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_exit = false;
        while Instant::now() < deadline {
            if let Ok(Event::App(AppEvent::Toast { message, .. })) =
                receiver.recv_timeout(Duration::from_millis(100))
            {
                if message.contains("exited: exit status: 1") && message.contains(".log") {
                    saw_exit = true;
                    break;
                }
            }
        }
        assert!(saw_exit);
        thread::sleep(Duration::from_millis(500));
        assert!(!process.has_child());
        clean_child(&process);
    }

    #[cfg(unix)]
    #[test]
    fn failed_spawn_recovers_after_backoff() {
        let mut process = isolated_process(false_binary(), "http://127.0.0.1:1".into());
        let script = process.config.log_path.with_extension("sh");
        Arc::get_mut(&mut process).unwrap().config.binary_path =
            script.to_string_lossy().into_owned();
        let (sender, _receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        thread::sleep(Duration::from_millis(300));
        assert!(!process.has_child());
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        thread::sleep(Duration::from_secs(3));
        assert!(process.has_child());
        clean_child(&process);
        std::fs::remove_file(script).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn slow_readiness_recovers_after_timeout_without_duplicate() {
        let ready = Arc::new(AtomicBool::new(false));
        let (api_base, stop, health_thread) = fake_health(ready.clone());
        let script = std::env::temp_dir().join(format!(
            "ario-slow-daemon-{}-{}.sh",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let process = isolated_process(&script.to_string_lossy(), api_base);
        let (sender, receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        let deadline = Instant::now() + Duration::from_secs(7);
        let mut saw_timeout = false;
        while Instant::now() < deadline {
            if let Ok(Event::App(AppEvent::Lifecycle(LifecycleState::Failed(message)))) =
                receiver.recv_timeout(Duration::from_millis(100))
            {
                if message.contains("five seconds") {
                    saw_timeout = true;
                    break;
                }
            }
        }
        assert!(saw_timeout);
        let pid = process.child.lock().unwrap().as_ref().unwrap().process.id();
        ready.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut connected = false;
        while Instant::now() < deadline {
            if let Ok(Event::App(AppEvent::Lifecycle(LifecycleState::Connected))) =
                receiver.recv_timeout(Duration::from_millis(100))
            {
                connected = true;
                break;
            }
        }
        assert!(connected);
        assert_eq!(
            process.child.lock().unwrap().as_ref().unwrap().process.id(),
            pid
        );
        clean_child(&process);
        stop.store(true, Ordering::SeqCst);
        health_thread.join().unwrap();
        std::fs::remove_file(script).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn attaches_to_existing_daemon_and_quit_stops_worker() {
        let ready = Arc::new(AtomicBool::new(true));
        let (api_base, stop, health_thread) = fake_health(ready);
        let process = isolated_process(false_binary(), api_base);
        let (sender, receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut connected = false;
        while Instant::now() < deadline {
            if let Ok(Event::App(AppEvent::Lifecycle(LifecycleState::Connected))) =
                receiver.recv_timeout(Duration::from_millis(100))
            {
                connected = true;
                break;
            }
        }
        assert!(connected);
        assert!(!process.owns_process());
        assert!(!process.has_child());
        process.stop_supervisor();
        stop.store(true, Ordering::SeqCst);
        health_thread.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn five_fast_exits_stop_daemon_retries() {
        let process = isolated_process(false_binary(), "http://127.0.0.1:1".into());
        let (sender, receiver) = std::sync::mpsc::channel();
        process.start_supervisor(sender);
        let deadline = Instant::now() + Duration::from_secs(11);
        let mut gave_up = false;
        while Instant::now() < deadline {
            if let Ok(Event::App(AppEvent::Lifecycle(LifecycleState::Failed(message)))) =
                receiver.recv_timeout(Duration::from_millis(100))
            {
                if message.contains("crash-looping") {
                    gave_up = true;
                    break;
                }
            }
        }
        assert!(gave_up);
        assert!(!process.has_child());
        process.stop_supervisor();
    }

    #[cfg(unix)]
    #[test]
    fn terminate_owned_signals_child_without_long_wait() {
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

        let started = Instant::now();
        process.terminate_owned();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!process.has_child());
    }

    #[cfg(unix)]
    #[test]
    fn quitting_during_startup_prevents_later_respawn() {
        let process = isolated_process("/bin/sh", "http://127.0.0.1:1".into());
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
        thread::sleep(Duration::from_millis(150));
        process.stop_supervisor();
        clean_child(&process);
        thread::sleep(Duration::from_millis(2200));
        assert!(!process.has_child());
        assert!(process.supervisor.lock().unwrap().is_none());
    }
}
