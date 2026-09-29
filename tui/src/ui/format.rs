use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "-".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec))
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    format!("{size:.1} {}", UNITS[unit_idx])
}

pub(crate) fn format_eta(seconds: u64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if h > 0 {
        format!("{h}h{m}m")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

pub(crate) fn middle_truncate(text: &str, width: usize, ellipsis: &str) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let marker_width = ellipsis.width();
    if width < marker_width {
        return ellipsis.chars().take(width).collect();
    }
    let remaining = width - marker_width;
    let head_budget = remaining / 2;
    let tail_budget = remaining - head_budget;
    let mut head = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        if used + grapheme.width() > head_budget {
            break;
        }
        head.push_str(grapheme);
        used += grapheme.width();
    }
    let mut tail = Vec::new();
    used = 0;
    for grapheme in text.graphemes(true).rev() {
        if used + grapheme.width() > tail_budget {
            break;
        }
        tail.push(grapheme);
        used += grapheme.width();
    }
    format!(
        "{head}{ellipsis}{}",
        tail.into_iter().rev().collect::<String>()
    )
}

/// Character wrapping preserves spaces and also works for filenames without word boundaries.
pub(crate) fn wrap_name(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = vec![String::new()];
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        // A terminal narrower than a single grapheme cannot display that grapheme.
        let grapheme = if grapheme.width() > width {
            "?"
        } else {
            grapheme
        };
        let cells = grapheme.width();
        if used + cells > width {
            lines.push(String::new());
            used = 0;
        }
        lines.last_mut().unwrap().push_str(grapheme);
        used += cells;
    }
    lines
}

pub(crate) fn detail_lines(text: &str, width: usize, height: usize, ellipsis: &str) -> Vec<String> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let full = wrap_name(text, width);
    if full.len() <= height {
        return full;
    }
    // Wide graphemes can leave unused cells at line ends, so fit the wrapped result too.
    let mut budget = width * height;
    loop {
        let shortened = middle_truncate(text, budget, ellipsis);
        let lines = wrap_name(&shortened, width);
        if lines.len() <= height || budget == 0 {
            return lines;
        }
        budget -= 1;
    }
}
