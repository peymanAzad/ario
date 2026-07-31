use common::settings::Settings;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Aria2Config {
    pub binary_path: String,
    pub rpc_port: u16,
    pub download_dir: String,
}

impl Default for Aria2Config {
    fn default() -> Self {
        Self {
            binary_path: "aria2c".to_string(),
            rpc_port: 6800,
            download_dir: "~/Downloads".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ServerConfig {
    pub settings: Settings,
    pub aria2: Aria2Config,
}

pub fn config_dir() -> anyhow::Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ario")
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    Ok(dirs.config_dir().to_path_buf())
}

fn config_file_path() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("server.toml"))
}

pub fn load_or_create() -> anyhow::Result<ServerConfig> {
    let path = config_file_path()?;

    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        let config: ServerConfig = toml::from_str(&text)?;
        Ok(config)
    } else {
        let config = ServerConfig::default();
        save(&config)?;
        Ok(config)
    }
}

pub fn save(config: &ServerConfig) -> anyhow::Result<()> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let text = toml::to_string_pretty(config)?;
    std::fs::write(config_file_path()?, text)?;
    Ok(())
}

pub fn expand_tilde(path: &str) -> anyhow::Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(base.home_dir().join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}
