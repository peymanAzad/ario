use crate::{db::Database, state::AppState};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

pub mod db;
pub mod error;
pub mod routes;
pub mod state;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let db = Database::open("./ario.db")?;
    let state = AppState::new(db);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(routes::downloads::router())
        .merge(routes::queues::router())
        .layer(cors)
        .with_state(state);

    let listner = tokio::net::TcpListener::bind("127.0.0.1:47812").await?;
    println!("ario is running on http://localhost:47812");

    axum::serve(listner, app).await?;
    Ok(())
}
