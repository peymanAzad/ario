use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::enums::{CategoryExtensions, FileCategory};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProxySettings {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub default_download_location: String,
    pub category_locations: HashMap<FileCategory, String>,
    pub category_extensions: CategoryExtensions,
    pub proxy: ProxySettings,
    pub start_daemon_on_login: bool,
    pub stop_daemon_on_close: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_download_location: "~/Downloads".to_string(),
            category_locations: HashMap::new(),
            category_extensions: FileCategory::default_extensions(),
            proxy: ProxySettings::default(),
            start_daemon_on_login: false,
            stop_daemon_on_close: false,
        }
    }
}
