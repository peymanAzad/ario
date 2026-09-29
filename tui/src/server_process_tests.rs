use super::*;
use crate::app::LifecycleState;
use crate::msg::Msg;
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
        if let Ok(Event::Msg(Msg::Toast { message, .. })) =
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
        if let Ok(Event::Msg(Msg::Lifecycle(LifecycleState::Failed(message)))) =
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
        if let Ok(Event::Msg(Msg::Lifecycle(LifecycleState::Connected))) =
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
        if let Ok(Event::Msg(Msg::Lifecycle(LifecycleState::Connected))) =
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
        if let Ok(Event::Msg(Msg::Lifecycle(LifecycleState::Failed(message)))) =
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
