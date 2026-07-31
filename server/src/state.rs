use crate::aria2::Aria2Client;
use crate::config::ServerConfig;
use crate::db::Database;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub aria2: Arc<Aria2Client>,
    pub config: Arc<ServerConfig>,
}

impl AppState {
    pub fn new(db: Database, aria2: Aria2Client, config: ServerConfig) -> Self {
        Self {
            db: Arc::new(db),
            aria2: Arc::new(aria2),
            config: Arc::new(config),
        }
    }
}
