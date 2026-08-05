use regex::Regex;
use std::sync::OnceLock;

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(https?://[^\s]+|magnet:\?[^\s]+)").expect("hardcoded regex is valid")
    })
}

pub fn scan_clipboard_for_urls() -> Vec<String> {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => {
            return Vec::new();
        }
    };
    let text = match clipboard.get_text() {
        Ok(t) => t,
        Err(_) => {
            return Vec::new();
        }
    };

    url_regex()
        .find_iter(&text)
        .map(|m| {
            m.as_str()
                .trim_end_matches(|c: char| ".,;:)]}\"'".contains(c))
                .to_string()
        })
        .collect()
}
