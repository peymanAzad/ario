use super::support::{Fault, Fixture, weekly};
use reqwest::Method;

#[tokio::test]
async fn managed_idle_shutdown_commits_and_rejects_future_mutations() {
    let f = Fixture::with_managed(true).await;
    let result = f.json(Method::POST, "/shutdown-if-idle", None).await;
    assert_eq!(result["outcome"], "shutting_down");
    assert_eq!(result["active_downloads"], 0);
    assert_eq!(result["scheduled_queues"], 0);
    assert_eq!(
        f.response(Method::POST, "/queues/1/resume", None)
            .await
            .status(),
        503
    );
    f.close().await;
}

#[tokio::test]
async fn managed_work_and_enabled_schedules_block_idle_but_force_shutdown_commits() {
    for scheduled in [false, true] {
        let f = Fixture::with_managed(true).await;
        if scheduled {
            f.queue(true, weekly(12, 13), false).await;
        } else {
            f.add(1, 1, true).await;
        }
        let result = f.json(Method::POST, "/shutdown-if-idle", None).await;
        assert_eq!(result["outcome"], "kept_running");
        assert_eq!(
            result[if scheduled {
                "scheduled_queues"
            } else {
                "active_downloads"
            }],
            1
        );
        let result = f.json(Method::POST, "/shutdown", None).await;
        assert_eq!(result["outcome"], "shutting_down");
        assert_eq!(
            f.response(Method::POST, "/queues/1/pause", None)
                .await
                .status(),
            503
        );
        f.close().await;
    }
}

#[tokio::test]
async fn unmanaged_shutdown_endpoints_preserve_daemon_availability() {
    let f = Fixture::new().await;
    for path in ["/shutdown-if-idle", "/shutdown"] {
        assert_eq!(
            f.json(Method::POST, path, None).await["outcome"],
            "not_managed"
        );
    }
    f.add(1, 1, false).await;
    assert_eq!(f.json(Method::GET, "/health", None).await["server"], "ok");
    f.close().await;
}

#[tokio::test]
async fn health_checks_the_proxy_independently_of_aria2() {
    let mut f = Fixture::new().await;
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = (*f.state.config).clone();
    config.settings.proxy.host = Some("127.0.0.1".into());
    config.settings.proxy.port = Some(proxy.local_addr().unwrap().port());
    f.reconfigure(config.clone()).await;
    let health = f.json(Method::GET, "/health", None).await;
    assert_eq!(health["aria2_reachable"], true);
    assert!(health.get("proxy_reachable").is_none());

    config.settings.proxy.enabled = true;
    let mut aria2_config =
        crate::aria2_process::Aria2ProcessConfig::new_with_random_secret(".".into(), ".".into());
    aria2_config
        .configure_proxy(&config.settings.proxy)
        .unwrap();
    let mut reachability =
        crate::proxy_probe::start_reachability(aria2_config.proxy_endpoint().unwrap());
    f.state.proxy_reachable = Some(reachability.clone());
    let (probe, result) = tokio::sync::watch::channel(None);
    f.state.proxy_data = Some(result);
    f.reconfigure(config).await;

    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        reachability.wait_for(|status| *status == Some(true)),
    )
    .await
    .unwrap()
    .unwrap();
    f.fault("aria2.getVersion", Fault::Transport);
    let health = f.json(Method::GET, "/health", None).await;
    assert_eq!(health["aria2_reachable"], false);
    assert_eq!(health["proxy_reachable"], true);
    assert!(health.get("proxy_data").is_none());

    probe.send(Some(false)).unwrap();
    assert_eq!(
        f.json(Method::GET, "/health", None).await["proxy_data"],
        false
    );
    probe.send(Some(true)).unwrap();
    assert_eq!(
        f.json(Method::GET, "/health", None).await["proxy_data"],
        true
    );

    drop(proxy);
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        reachability.wait_for(|status| *status == Some(false)),
    )
    .await
    .unwrap()
    .unwrap();
    let health = f.json(Method::GET, "/health", None).await;
    assert_eq!(health["aria2_reachable"], true);
    assert_eq!(health["proxy_reachable"], false);
    assert!(health.get("proxy_data").is_none());
    f.close().await;
}

#[tokio::test]
async fn aria2_outage_health_and_polling_recover_without_duplicate_transfers() {
    let f = Fixture::new().await;
    let item = f.add(1, 1, true).await.remove(0);
    f.transfer(item.download.id, "active", 40).await;
    f.tick().await;
    let health = f.json(Method::GET, "/health", None).await;
    assert_eq!(health["active_downloads"], 1);
    assert_eq!(health["download_speed"], 10);
    f.fault("aria2.getVersion", Fault::Transport);
    assert_eq!(
        f.json(Method::GET, "/health", None).await["aria2_reachable"],
        false
    );
    f.fault("aria2.tellStatus", Fault::Transport);
    f.tick().await;
    assert_eq!(f.get(item.download.id).await.completed_length, 40);
    f.transfer(item.download.id, "active", 70).await;
    f.tick().await;
    assert_eq!(f.get(item.download.id).await.completed_length, 70);
    assert_eq!(
        f.get(item.download.id).await.download.aria2_gid,
        item.download.aria2_gid
    );
    assert_eq!(f.calls("aria2.addUri"), 1);
    assert_eq!(
        f.json(Method::GET, "/health", None).await["aria2_reachable"],
        true
    );
    f.post(&format!("/downloads/{}/pause", item.download.id))
        .await;
    f.tick().await;
    assert_eq!(
        f.json(Method::GET, "/health", None).await["download_speed"],
        0
    );
    f.close().await;
}
