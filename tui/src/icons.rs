use std::str::FromStr;

use common::enums::{DownloadStatus, FileCategory, QueueStatus};
use ratatui::style::Color;

use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphMode {
    NerdFont,
    Unicode,
    Ascii,
}

impl FromStr for GlyphMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "nerd" => Ok(Self::NerdFont),
            "unicode" => Ok(Self::Unicode),
            "ascii" => Ok(Self::Ascii),
            _ => Err(format!(
                "invalid glyph mode {value:?}; expected nerd, unicode, or ascii"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconSet {
    mode: GlyphMode,
}

impl IconSet {
    pub const fn new(mode: GlyphMode) -> Self {
        Self { mode }
    }

    pub const fn glyph_mode(self) -> GlyphMode {
        self.mode
    }

    pub const fn ellipsis(self) -> &'static str {
        match self.mode {
            GlyphMode::Ascii => "...",
            GlyphMode::NerdFont | GlyphMode::Unicode => "…",
        }
    }

    pub const fn all(self) -> &'static str {
        match self.mode {
            GlyphMode::NerdFont => "\u{f07b}",
            GlyphMode::Unicode => "◆",
            GlyphMode::Ascii => "*",
        }
    }

    pub fn category(self, category: &FileCategory) -> &'static str {
        match (self.mode, category) {
            (GlyphMode::NerdFont, FileCategory::Video) => "\u{f03d}",
            (GlyphMode::NerdFont, FileCategory::Music) => "\u{f001}",
            (GlyphMode::NerdFont, FileCategory::Document) => "\u{f15c}",
            (GlyphMode::NerdFont, FileCategory::Archive) => "\u{f06eb}",
            (GlyphMode::NerdFont, FileCategory::Program) => "\u{f0614}",
            (GlyphMode::NerdFont, FileCategory::Other) => "\u{f15b}",
            (GlyphMode::Unicode, FileCategory::Video) => "▶",
            (GlyphMode::Unicode, FileCategory::Music) => "♪",
            (GlyphMode::Unicode, FileCategory::Document) => "▤",
            (GlyphMode::Unicode, FileCategory::Archive) => "▣",
            (GlyphMode::Unicode, FileCategory::Program) => "⌘",
            (GlyphMode::Unicode, FileCategory::Other) => "•",
            (GlyphMode::Ascii, FileCategory::Video) => "V",
            (GlyphMode::Ascii, FileCategory::Music) => "M",
            (GlyphMode::Ascii, FileCategory::Document) => "D",
            (GlyphMode::Ascii, FileCategory::Archive) => "A",
            (GlyphMode::Ascii, FileCategory::Program) => "P",
            (GlyphMode::Ascii, FileCategory::Other) => "O",
        }
    }

    pub fn download_status(self, status: &DownloadStatus) -> &'static str {
        match (self.mode, status) {
            (GlyphMode::NerdFont, DownloadStatus::Pending) => "\u{f017}",
            (GlyphMode::NerdFont, DownloadStatus::Active) => "\u{f04b}",
            (GlyphMode::NerdFont, DownloadStatus::Paused) => "\u{f04c}",
            (GlyphMode::NerdFont, DownloadStatus::Completed) => "\u{f00c}",
            (GlyphMode::NerdFont, DownloadStatus::Error(_)) => "\u{f00d}",
            (GlyphMode::NerdFont, DownloadStatus::Removed) => "\u{f1f8}",
            (GlyphMode::Unicode, DownloadStatus::Pending) => "◷",
            (GlyphMode::Unicode, DownloadStatus::Active) => "↓",
            (GlyphMode::Unicode, DownloadStatus::Paused) => "‖",
            (GlyphMode::Unicode, DownloadStatus::Completed) => "✓",
            (GlyphMode::Unicode, DownloadStatus::Error(_)) => "✕",
            (GlyphMode::Unicode, DownloadStatus::Removed) => "⊘",
            (GlyphMode::Ascii, DownloadStatus::Pending) => ".",
            (GlyphMode::Ascii, DownloadStatus::Active) => ">",
            (GlyphMode::Ascii, DownloadStatus::Paused) => "|",
            (GlyphMode::Ascii, DownloadStatus::Completed) => "+",
            (GlyphMode::Ascii, DownloadStatus::Error(_)) => "!",
            (GlyphMode::Ascii, DownloadStatus::Removed) => "x",
        }
    }

    pub fn download_status_color(self, status: &DownloadStatus, theme: &Theme) -> Color {
        match status {
            DownloadStatus::Pending | DownloadStatus::Paused => theme.status_warning,
            DownloadStatus::Active | DownloadStatus::Completed => theme.status_ok,
            DownloadStatus::Error(_) => theme.status_error,
            DownloadStatus::Removed => theme.text_muted,
        }
    }

    pub fn queue_status(self, status: &QueueStatus) -> &'static str {
        match (self.mode, status) {
            (GlyphMode::NerdFont, QueueStatus::Active) => "\u{f04b}",
            (GlyphMode::NerdFont, QueueStatus::Paused) => "\u{f04c}",
            (GlyphMode::Unicode, QueueStatus::Active) => "▶",
            (GlyphMode::Unicode, QueueStatus::Paused) => "‖",
            (GlyphMode::Ascii, QueueStatus::Active) => ">",
            (GlyphMode::Ascii, QueueStatus::Paused) => "|",
        }
    }

    pub fn queue_status_color(self, status: &QueueStatus, theme: &Theme) -> Color {
        match status {
            QueueStatus::Active => theme.status_ok,
            QueueStatus::Paused => theme.status_warning,
        }
    }

    pub const fn scheduler(self) -> &'static str {
        match self.mode {
            GlyphMode::NerdFont => "\u{f13ab}",
            GlyphMode::Unicode => "◷",
            GlyphMode::Ascii => "@",
        }
    }
}

pub fn parse_glyph_args<I, S>(args: I) -> anyhow::Result<Option<GlyphMode>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let mut mode = None;
    while let Some(arg) = args.next() {
        let value = if arg == "--glyphs" {
            args.next()
                .ok_or_else(|| anyhow::anyhow!("--glyphs requires nerd, unicode, or ascii"))?
        } else if let Some(value) = arg.strip_prefix("--glyphs=") {
            value.to_string()
        } else {
            anyhow::bail!("unknown argument {arg:?}");
        };
        if mode.is_some() {
            anyhow::bail!("--glyphs may only be specified once");
        }
        mode = Some(value.parse().map_err(anyhow::Error::msg)?);
    }
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [GlyphMode; 3] = [GlyphMode::NerdFont, GlyphMode::Unicode, GlyphMode::Ascii];
    const CATEGORIES: [FileCategory; 6] = [
        FileCategory::Video,
        FileCategory::Music,
        FileCategory::Document,
        FileCategory::Archive,
        FileCategory::Program,
        FileCategory::Other,
    ];

    #[test]
    fn every_semantic_icon_is_defined_in_every_mode() {
        let statuses = [
            DownloadStatus::Pending,
            DownloadStatus::Active,
            DownloadStatus::Paused,
            DownloadStatus::Completed,
            DownloadStatus::Error("failure".into()),
            DownloadStatus::Removed,
        ];
        let queue_statuses = [QueueStatus::Active, QueueStatus::Paused];

        for mode in MODES {
            let icons = IconSet::new(mode);
            for glyph in std::iter::once(icons.all())
                .chain(CATEGORIES.iter().map(|category| icons.category(category)))
                .chain(statuses.iter().map(|status| icons.download_status(status)))
                .chain(
                    queue_statuses
                        .iter()
                        .map(|status| icons.queue_status(status)),
                )
                .chain(std::iter::once(icons.scheduler()))
            {
                assert!(!glyph.is_empty());
                assert_ne!(glyph, "?");
            }
        }
    }

    #[test]
    fn parses_both_cli_forms_and_rejects_bad_values() {
        assert_eq!(
            parse_glyph_args(["--glyphs=nerd"]).unwrap(),
            Some(GlyphMode::NerdFont)
        );
        assert_eq!(
            parse_glyph_args(["--glyphs", "ascii"]).unwrap(),
            Some(GlyphMode::Ascii)
        );
        assert!(parse_glyph_args(["--glyphs=emoji"]).is_err());
        assert!(parse_glyph_args(["--glyphs"]).is_err());
        assert!(parse_glyph_args(["--other"]).is_err());
    }
}
