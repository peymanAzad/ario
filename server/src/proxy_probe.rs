use reqwest::{Client, Proxy, Url, redirect::Policy};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{net::TcpStream, sync::watch, time};

const TARGET: &str = "https://www.cloudflare.com/cdn-cgi/trace";
const TCP_INTERVAL: Duration = Duration::from_secs(2);
const TCP_TIMEOUT: Duration = Duration::from_millis(500);
const DATA_INTERVAL: Duration = Duration::from_secs(30);
const DATA_TIMEOUT: Duration = Duration::from_secs(5);

pub fn start_reachability(endpoint: String) -> watch::Receiver<Option<bool>> {
    let endpoint = Arc::<str>::from(endpoint);
    start(TCP_INTERVAL, move || {
        let endpoint = endpoint.clone();
        async move {
            matches!(
                time::timeout(TCP_TIMEOUT, TcpStream::connect(endpoint.as_ref())).await,
                Ok(Ok(_))
            )
        }
    })
}

pub fn start_data(proxy_url: &Url) -> watch::Receiver<Option<bool>> {
    let Ok(client) = build_client(proxy_url) else {
        return watch::channel(Some(false)).1;
    };
    start(DATA_INTERVAL, move || {
        let client = client.clone();
        async move { receives_data(&client, TARGET).await }
    })
}

fn start<F>(
    interval: Duration,
    probe: impl Fn() -> F + Send + 'static,
) -> watch::Receiver<Option<bool>>
where
    F: Future<Output = bool> + Send,
{
    let (result, receiver) = watch::channel(None);
    tokio::spawn(async move {
        let mut ticks = time::interval(interval);
        ticks.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = result.closed() => break,
                _ = ticks.tick() => {
                    if result.send(Some(probe().await)).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

fn build_client(proxy_url: &Url) -> reqwest::Result<Client> {
    Client::builder()
        .proxy(Proxy::all(proxy_url.clone())?)
        .redirect(Policy::none())
        .pool_max_idle_per_host(0)
        .timeout(DATA_TIMEOUT)
        .build()
}

async fn receives_data(client: &Client, target: &str) -> bool {
    let Ok(mut response) = client.get(target).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    while let Ok(Some(bytes)) = response.chunk().await {
        if !bytes.is_empty() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::{Request, State},
        http::{Method, StatusCode},
    };

    #[tokio::test]
    async fn probe_requires_success_and_body_bytes_from_the_configured_proxy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_url = Url::parse(&format!(
            "http://user:p%2540@{}",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let (seen_connect, connect) = watch::channel(false);
        let proxy = Router::new()
            .fallback(
                |State(seen_connect): State<watch::Sender<bool>>, request: Request| async move {
                    assert_eq!(
                        request.headers()["proxy-authorization"],
                        "Basic dXNlcjpwJTQw"
                    );
                    if request.method() == Method::CONNECT {
                        assert_eq!(request.uri().authority().unwrap(), "www.cloudflare.com:443");
                        seen_connect.send_replace(true);
                        return (StatusCode::PROXY_AUTHENTICATION_REQUIRED, "denied");
                    }
                    assert_eq!(request.uri().host(), Some("probe.invalid"));
                    match request.uri().path() {
                        "/body" => (StatusCode::OK, "data"),
                        "/empty" => (StatusCode::OK, ""),
                        _ => (StatusCode::BAD_GATEWAY, "error page"),
                    }
                },
            )
            .with_state(seen_connect);
        let server = tokio::spawn(async move { axum::serve(listener, proxy).await.unwrap() });
        let client = build_client(&proxy_url).unwrap();
        assert!(receives_data(&client, "http://probe.invalid/body").await);
        assert!(!receives_data(&client, "http://probe.invalid/empty").await);
        assert!(!receives_data(&client, "http://probe.invalid/error").await);
        let mut result = start_data(&proxy_url);
        time::timeout(DATA_TIMEOUT + Duration::from_secs(1), result.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(*result.borrow(), Some(false));
        assert!(*connect.borrow());
        server.abort();
    }
}
