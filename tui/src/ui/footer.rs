use common::enums::{DownloadStatus, QueueStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, downloads::DownloadAction};

const HINT_SEPARATOR: &str = "   ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FooterHint {
    key: &'static str,
    label: &'static str,
}

impl FooterHint {
    const fn new(key: &'static str, label: &'static str) -> Self {
        Self { key, label }
    }

    fn width(self) -> usize {
        self.key.width() + 1 + self.label.width()
    }
}

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints = footer_hints(app);
    let accent = Style::default().fg(app.theme.accent);
    let muted = Style::default().fg(app.theme.text_muted);
    let available = area.width as usize;
    let mut used = 0;
    let mut spans = Vec::new();

    for (index, hint) in hints.into_iter().enumerate() {
        let separator_width = usize::from(index > 0) * HINT_SEPARATOR.width();
        let required = separator_width + hint.width();
        if index > 0 && used + required > available {
            break;
        }
        if index > 0 {
            spans.push(Span::styled(HINT_SEPARATOR, muted));
        }
        spans.push(Span::styled(hint.key, accent));
        spans.push(Span::styled(":", muted));
        spans.push(Span::styled(hint.label, muted));
        used += required;
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn footer_hints(app: &App) -> Vec<FooterHint> {
    let mut hints = vec![FooterHint::new("?", "Help")];
    match app.focus {
        Focus::Downloads => download_hints(app, &mut hints),
        Focus::Queues => queue_hints(app, &mut hints),
        Focus::Categories => category_hints(&mut hints),
    }
    hints
}

fn download_hints(app: &App, hints: &mut Vec<FooterHint>) {
    hints.extend([
        FooterHint::new("v", "Import Clipboard"),
        FooterHint::new("a", "Torrent"),
    ]);

    if let Some(download) = app.current_download() {
        if let Some(action) = app.current_download_action() {
            hints.push(download_action_hint(action));
        }
        let completed = download.download.status == DownloadStatus::Completed;
        hints.push(FooterHint::new(
            "Enter",
            if completed { "Open" } else { "Edit" },
        ));
        if completed {
            hints.push(FooterHint::new("f", "Folder"));
        }
        hints.push(FooterHint::new("d/D", "Delete/Delete+files"));
    }

    hints.extend([
        FooterHint::new("j/k", "Navigate"),
        FooterHint::new("Tab", "Pane"),
        FooterHint::new("q", "Quit"),
    ]);
}

fn download_action_hint(action: DownloadAction) -> FooterHint {
    match action {
        DownloadAction::Start => FooterHint::new("r", "Start"),
        DownloadAction::Resume => FooterHint::new("r", "Resume"),
        DownloadAction::Pause => FooterHint::new("p", "Pause"),
        DownloadAction::Retry => FooterHint::new("r", "Retry"),
        DownloadAction::Restart => FooterHint::new("r", "Restart"),
    }
}

fn queue_hints(app: &App, hints: &mut Vec<FooterHint>) {
    if app.selected_queue == 0 {
        hints.extend([
            FooterHint::new("x", "Clear completed"),
            FooterHint::new("n", "New"),
        ]);
    } else if let Some(queue) = app.current_queue() {
        hints.push(match queue.status {
            QueueStatus::Active => FooterHint::new("p", "Pause"),
            QueueStatus::Paused => FooterHint::new("r", "Resume"),
        });
        hints.extend([
            FooterHint::new("Enter", "Edit"),
            FooterHint::new("n", "New"),
            FooterHint::new("x", "Clear completed"),
        ]);
        if app.can_delete_selected_queue() {
            hints.push(FooterHint::new("d", "Delete"));
        }
    } else {
        hints.extend([
            FooterHint::new("x", "Clear completed"),
            FooterHint::new("n", "New"),
        ]);
    }

    hints.extend([
        FooterHint::new("j/k", "Navigate"),
        FooterHint::new("Tab", "Pane"),
        FooterHint::new("v", "Import Clipboard"),
        FooterHint::new("a", "Torrent"),
        FooterHint::new("q", "Quit"),
    ]);
}

fn category_hints(hints: &mut Vec<FooterHint>) {
    hints.extend([
        FooterHint::new("j/k", "Navigate"),
        FooterHint::new("Tab", "Pane"),
        FooterHint::new("v", "Import Clipboard"),
        FooterHint::new("a", "Torrent"),
        FooterHint::new("q", "Quit"),
    ]);
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
