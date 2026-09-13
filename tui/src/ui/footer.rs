use common::enums::DownloadStatus;
use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

use crate::app::{App, Focus};

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.focus {
        Focus::Downloads => {
            let rp = downloads_rp_hint(app.current_download().map(|d| &d.download.status));
            format!(
                "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   Enter: open/edit   f: open folder{rp}   d: delete   v: import clipboard   q: quit"
            )
        }
        Focus::Queues => {
            if app.selected_queue == 0 {
                "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   x: remove completed   n: new queue   Enter: edit queue   v: import clipboard   q: quit".to_string()
            } else {
                "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   x: remove completed   r/p: resume/pause   n: new queue   Enter: edit queue   v: import clipboard   q: quit".to_string()
            }
        }
        Focus::Categories => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   v: import clipboard   q: quit"
                .to_string()
        }
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(app.theme.text_muted)),
        area,
    );
}

fn downloads_rp_hint(status: Option<&DownloadStatus>) -> &'static str {
    match status {
        None => "",
        Some(DownloadStatus::Pending | DownloadStatus::Paused) => "   r/p: resume/pause",
        Some(DownloadStatus::Active) => "   p: pause",
        Some(DownloadStatus::Error(_)) => "   r: retry",
        Some(DownloadStatus::Completed) => "   r: restart",
        Some(DownloadStatus::Removed) => "   r: retry",
    }
}

#[cfg(test)]
mod tests {
    use super::downloads_rp_hint;
    use common::enums::DownloadStatus;

    #[test]
    fn downloads_rp_hint_matches_selected_status() {
        assert_eq!(downloads_rp_hint(None), "");
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Pending)),
            "   r/p: resume/pause"
        );
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Paused)),
            "   r/p: resume/pause"
        );
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Active)),
            "   p: pause"
        );
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Error("boom".into()))),
            "   r: retry"
        );
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Completed)),
            "   r: restart"
        );
        assert_eq!(
            downloads_rp_hint(Some(&DownloadStatus::Removed)),
            "   r: retry"
        );
    }
}
