use common::enums::{DownloadStatus, QueueStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, downloads_table::DownloadAction};

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
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use chrono::Utc;
    use common::{
        download::{Download, DownloadLiveStatus},
        enums::{FileCategory, Recurrence, SourceType},
        finetune::FineTune,
        queue::{Queue, QueueSettings},
        scheduler::Scheduler,
    };
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    use std::sync::mpsc;

    fn app() -> App {
        let (sender, _receiver) = mpsc::channel();
        App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(GlyphMode::Unicode),
            sender,
            false,
        )
    }

    fn add_download(app: &mut App, status: DownloadStatus) {
        app.downloads.push(DownloadLiveStatus {
            download: Download {
                id: 1,
                aria2_gid: None,
                url: "https://example.test/file".into(),
                filename: Some("file.bin".into()),
                destination_path: "/tmp".into(),
                source_type: SourceType::Http,
                category: FileCategory::Other,
                status,
                paused_by_scheduler: false,
                manually_started: false,
                size: Some(100),
                completed_length: Some(0),
                queue_id: 1,
                position_in_queue: 0,
                finetune: FineTune::default(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            },
            completed_length: 0,
            download_speed: 0,
            eta_seconds: None,
        });
    }

    fn add_queue(app: &mut App, id: i64, status: QueueStatus) {
        app.queues.push(Queue {
            scheduled_stop_at: None,
            id,
            name: format!("Queue {id}"),
            position: 0,
            settings: QueueSettings {
                max_concurrent_downloads: 1,
                max_retries: 0,
                retry_wait_seconds: 0,
                default_finetune: FineTune::default(),
            },
            scheduler: Scheduler {
                enabled: false,
                recurrence: Recurrence::Once {
                    start: Utc::now(),
                    end: Utc::now(),
                },
                run_missed_on_startup: false,
            },
            created_at: Utc::now(),
            status,
        });
    }

    fn render(app: &App, width: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| draw_footer(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buffer: &Buffer) -> String {
        buffer.content().iter().map(|cell| cell.symbol()).collect()
    }

    fn labels(app: &App) -> Vec<(&'static str, &'static str)> {
        footer_hints(app)
            .into_iter()
            .map(|hint| (hint.key, hint.label))
            .collect()
    }

    #[test]
    fn download_hints_prioritize_clipboard_torrent_and_combine_delete_actions() {
        let mut app = app();
        add_download(&mut app, DownloadStatus::Error("failed".into()));

        assert_eq!(
            labels(&app),
            vec![
                ("?", "Help"),
                ("v", "Import Clipboard"),
                ("a", "Torrent"),
                ("r", "Retry"),
                ("Enter", "Edit"),
                ("d/D", "Delete/Delete+files"),
                ("j/k", "Navigate"),
                ("Tab", "Pane"),
                ("q", "Quit"),
            ]
        );
    }

    #[test]
    fn download_hints_follow_status_and_selection() {
        for (status, expected) in [
            (DownloadStatus::Pending, ("r", "Start")),
            (DownloadStatus::Paused, ("r", "Resume")),
            (DownloadStatus::Active, ("p", "Pause")),
            (DownloadStatus::Removed, ("r", "Retry")),
        ] {
            let mut app = app();
            add_download(&mut app, status);
            assert!(labels(&app).contains(&expected));
        }

        let mut completed = app();
        add_download(&mut completed, DownloadStatus::Completed);
        let completed = labels(&completed);
        assert!(completed.contains(&("r", "Restart")));
        assert!(completed.contains(&("Enter", "Open")));
        assert!(completed.contains(&("f", "Folder")));

        let empty = labels(&app());
        assert!(
            !empty
                .iter()
                .any(|(key, _)| ["r", "p", "Enter", "d/D"].contains(key))
        );
    }

    #[test]
    fn queue_hints_prioritize_valid_queue_actions() {
        let mut app = app();
        app.focus = Focus::Queues;
        assert_eq!(
            &labels(&app)[..3],
            &[("?", "Help"), ("x", "Clear completed"), ("n", "New")]
        );

        add_queue(&mut app, 1, QueueStatus::Paused);
        app.selected_queue = 1;
        let main = labels(&app);
        assert_eq!(
            &main[..5],
            &[
                ("?", "Help"),
                ("r", "Resume"),
                ("Enter", "Edit"),
                ("n", "New"),
                ("x", "Clear completed"),
            ]
        );
        assert!(!main.contains(&("d", "Delete")));

        add_queue(&mut app, 2, QueueStatus::Active);
        app.selected_queue = 2;
        let deletable = labels(&app);
        assert_eq!(deletable[1], ("p", "Pause"));
        assert!(deletable.contains(&("d", "Delete")));
        let import = deletable
            .iter()
            .position(|hint| *hint == ("v", "Import Clipboard"))
            .unwrap();
        let navigate = deletable
            .iter()
            .position(|hint| *hint == ("j/k", "Navigate"))
            .unwrap();
        assert!(navigate < import);
    }

    #[test]
    fn category_hints_put_navigation_before_global_actions() {
        let mut app = app();
        app.focus = Focus::Categories;
        assert_eq!(
            labels(&app),
            vec![
                ("?", "Help"),
                ("j/k", "Navigate"),
                ("Tab", "Pane"),
                ("v", "Import Clipboard"),
                ("a", "Torrent"),
                ("q", "Quit"),
            ]
        );
    }

    #[test]
    fn responsive_rendering_keeps_only_complete_hints() {
        let app = app();
        assert_eq!(text(&render(&app, 6)), "?:Help");
        assert_eq!(text(&render(&app, 9)), "?:Help   ");
        assert_eq!(text(&render(&app, 27)), "?:Help   v:Import Clipboard");
        assert!(!text(&render(&app, 30)).contains("Torrent"));
        assert!(text(&render(&app, 39)).contains("a:Torrent"));
    }

    #[test]
    fn footer_styles_keys_and_labels_differently() {
        let app = app();
        let buffer = render(&app, 30);
        assert_eq!(buffer[(0, 0)].fg, app.theme.accent);
        assert_eq!(buffer[(1, 0)].fg, app.theme.text_muted);
        assert_eq!(buffer[(2, 0)].fg, app.theme.text_muted);
        assert_eq!(buffer[(9, 0)].fg, app.theme.accent);
    }
}
