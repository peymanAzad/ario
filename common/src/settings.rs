use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::enums::FileCategory;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProxySettings {
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Setting options will be read from the config file
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub default_download_location: String,
    /// Per-category overrides of `default_download_location`. A category with no
    /// entry here falls back to the default.
    pub category_locations: HashMap<FileCategory, String>,
    pub proxy: ProxySettings,
    pub start_daemon_on_login: bool,
    pub stop_daemon_on_close: bool,
    pub file_types: HashMap<FileCategory, Vec<String>>,
}
