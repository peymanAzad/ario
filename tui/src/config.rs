//! TUI-only configuration: server URL and theme selection.
//! Shares the same "ario" config directory as `server.toml`

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TuiConfig {
    #[serde(default = "default_server_url")]
    pub server_url: String,
    #[serde(default = "default_true")]
    pub auto_start_server: bool,
    /// Path to `ario_daemon`. Empty = sibling of this binary, then PATH.
    #[serde(default)]
    pub server_binary: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Optional rendering mode: "nerd", "unicode", or "ascii".
    /// Unset means Nerd Font on UTF-8 locales, ASCII otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glyph_mode: Option<String>,
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
    pub status_warning: Option<String>,
    pub text_muted: Option<String>,
    pub accent: Option<String>,
}

fn default_server_url() -> String {
    "http://127.0.0.1:47812".to_string()
}

fn default_theme() -> String {
    "default".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            server_url: default_server_url(),
            auto_start_server: true,
            server_binary: String::new(),
            theme: default_theme(),
            glyph_mode: None,
            custom_theme: CustomTheme::default(),
        }
    }
}

pub fn resolve_glyph_mode(
    cli: Option<crate::icons::GlyphMode>,
    configured: Option<&str>,
    lc_all: Option<&str>,
    lc_ctype: Option<&str>,
    lang: Option<&str>,
) -> (crate::icons::GlyphMode, Option<String>) {
    if let Some(mode) = cli {
        return (mode, None);
    }

    if let Some(value) = configured {
        match value.parse() {
            Ok(mode) => return (mode, None),
            Err(_) => {
                let fallback = locale_glyph_mode(lc_all, lc_ctype, lang);
                return (
                    fallback,
                    Some(format!(
                        "tui.toml: glyph_mode = {value:?} is invalid (expected \"nerd\", \"unicode\", or \"ascii\") — using {fallback:?}"
                    )),
                );
            }
        }
    }

    (locale_glyph_mode(lc_all, lc_ctype, lang), None)
}

fn locale_glyph_mode(
    lc_all: Option<&str>,
    lc_ctype: Option<&str>,
    lang: Option<&str>,
) -> crate::icons::GlyphMode {
    let locale = [lc_all, lc_ctype, lang]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if locale.contains("utf-8") || locale.contains("utf8") {
        crate::icons::GlyphMode::NerdFont
    } else {
        crate::icons::GlyphMode::Ascii
    }
}

pub fn config_dir() -> anyhow::Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::GlyphMode;

    #[test]
    fn glyph_resolution_obeys_precedence() {
        assert_eq!(
            resolve_glyph_mode(
                Some(GlyphMode::Ascii),
                Some("nerd"),
                None,
                None,
                Some("en_US.UTF-8")
            )
            .0,
            GlyphMode::Ascii
        );
        assert_eq!(
            resolve_glyph_mode(None, Some("nerd"), None, None, Some("C")).0,
            GlyphMode::NerdFont
        );
    }

    #[test]
    fn locale_default_distinguishes_utf8() {
        assert_eq!(
            resolve_glyph_mode(None, None, None, None, Some("en_US.UTF-8")).0,
            GlyphMode::NerdFont
        );
        assert_eq!(
            resolve_glyph_mode(None, None, Some("C"), None, Some("en_US.UTF-8")).0,
            GlyphMode::Ascii
        );
    }

    #[test]
    fn invalid_config_warns_and_falls_back() {
        let (mode, warning) = resolve_glyph_mode(None, Some("emoji"), None, None, Some("C.UTF-8"));
        assert_eq!(mode, GlyphMode::NerdFont);
        assert!(warning.unwrap().contains("glyph_mode"));
    }
}
