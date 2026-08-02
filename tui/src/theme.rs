use ratatui::style::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub foreground: Color,
    pub border: Color,
    pub border_focused: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub status_ok: Color,
    pub status_error: Color,
    pub text_muted: Color,
    pub accent: Color,
}

impl Theme {
    pub fn default_dark() -> Self {
        Theme {
            foreground: Color::Black,
            border: Color::DarkGray,
            border_focused: Color::Blue,
            selected_bg: Color::Blue,
            selected_fg: Color::Black,
            status_ok: Color::Green,
            status_error: Color::Red,
            text_muted: Color::DarkGray,
            accent: Color::Magenta,
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "catppuccin-latte" => Self::from_catppuccin(&catppuccin::PALETTE.latte.colors),
            "catppuccin-frappe" | "catppuccin-frappé" => {
                Self::from_catppuccin(&catppuccin::PALETTE.frappe.colors)
            }
            "catppuccin-macchiato" => Self::from_catppuccin(&catppuccin::PALETTE.macchiato.colors),
            "catppuccin-mocha" => Self::from_catppuccin(&catppuccin::PALETTE.mocha.colors),
            _ => Self::default_dark(),
        }
    }

    fn from_catppuccin(c: &catppuccin::FlavorColors) -> Self {
        Theme {
            foreground: rgb(c.text.rgb),
            border: rgb(c.overlay0.rgb),
            border_focused: rgb(c.blue.rgb),
            selected_bg: rgb(c.blue.rgb),
            selected_fg: rgb(c.base.rgb),
            status_ok: rgb(c.green.rgb),
            status_error: rgb(c.red.rgb),
            text_muted: rgb(c.subtext0.rgb),
            accent: rgb(c.mauve.rgb),
        }
    }

    /// Applies field-level overrides from a hand-edited `[custom_theme]`
    /// table on top of `self` (typically the resolved named/preset theme —
    /// Malformed or unset hex fields are left untouched rather than causing
    /// an error — a typo in one field of a hand-edited config file shouldn't
    /// break every other field or stop the app from starting.
    pub fn apply_overrides(mut self, overrides: &crate::config::CustomTheme) -> Self {
        if let Some(c) = parse_hex(&overrides.foreground) {
            self.foreground = c;
        }
        if let Some(c) = parse_hex(&overrides.border) {
            self.border = c;
        }
        if let Some(c) = parse_hex(&overrides.border_focused) {
            self.border_focused = c;
        }
        if let Some(c) = parse_hex(&overrides.selected_bg) {
            self.selected_bg = c;
        }
        if let Some(c) = parse_hex(&overrides.selected_fg) {
            self.selected_fg = c;
        }
        if let Some(c) = parse_hex(&overrides.status_ok) {
            self.status_ok = c;
        }
        if let Some(c) = parse_hex(&overrides.status_error) {
            self.status_error = c;
        }
        if let Some(c) = parse_hex(&overrides.text_muted) {
            self.text_muted = c;
        }
        if let Some(c) = parse_hex(&overrides.accent) {
            self.accent = c;
        }
        self
    }
}

/// Parses `"#rrggbb"` or `"rrggbb"` into a `Color::Rgb`. Returns `None` if
/// the field is unset or malformed — the caller then leaves the existing
/// value in place rather than erroring.
fn parse_hex(value: &Option<String>) -> Option<Color> {
    let s = value.as_ref()?.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Returns a human-readable warning for each `[custom_theme]` field that's
/// set but doesn't parse as valid hex — intended to be printed (via
/// `eprintln!`) in `main` *before* entering the alternate screen.
pub fn validate(overrides: &crate::config::CustomTheme) -> Vec<String> {
    let fields: [(&str, &Option<String>); 9] = [
        ("foreground", &overrides.foreground),
        ("border", &overrides.border),
        ("border_focused", &overrides.border_focused),
        ("selected_bg", &overrides.selected_bg),
        ("selected_fg", &overrides.selected_fg),
        ("status_ok", &overrides.status_ok),
        ("status_error", &overrides.status_error),
        ("text_muted", &overrides.text_muted),
        ("accent", &overrides.accent),
    ];

    fields
        .iter()
        .filter_map(|(name, value)| {
            if value.is_some() && parse_hex(value).is_none() {
                Some(format!(
                    "tui.toml: [custom_theme].{name} = {:?} isn't valid hex (expected e.g. \"#89b4fa\") — ignoring, using the base theme's value instead",
                    value.as_ref().unwrap()
                ))
            } else {
                None
            }
        })
        .collect()
}

fn rgb(c: catppuccin::Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
