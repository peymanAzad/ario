//! TUI-only configuration: server URL and theme selection.
//! Shares the same "ario" config directory as `server.toml`

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TuiConfig {
    #[serde(default = "default_server_url")]
    pub server_url: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub custom_theme: CustomTheme,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CustomTheme {
    pub foreground: Option<String>,
    pub border: Option<String>,
    pub border_focused: Option<String>,
    pub selected_bg: Option<String>,
    pub selected_fg: Option<String>,
    pub status_ok: Option<String>,
    pub status_error: Option<String>,
    pub text_muted: Option<String>,
    pub accent: Option<String>,
}

fn default_server_url() -> String {
    "http://127.0.0.1:47812".to_string()
}

fn default_theme() -> String {
    "default".to_string()
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            server_url: default_server_url(),
            theme: default_theme(),
            custom_theme: CustomTheme::default(),
        }
    }
}

fn config_dir() -> anyhow::Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ario")
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    Ok(dirs.config_dir().to_path_buf())
}

fn config_file_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("tui.toml"))
}

pub fn load_or_create() -> anyhow::Result<TuiConfig> {
    let path = config_file_path()?;

    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        let config: TuiConfig = toml::from_str(&text)?;
        Ok(config)
    } else {
        let config = TuiConfig::default();
        save(&config)?;
        Ok(config)
    }
}

pub fn save(config: &TuiConfig) -> anyhow::Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let text = toml::to_string_pretty(config)?;
    std::fs::write(config_file_path()?, text)?;
    Ok(())
}
