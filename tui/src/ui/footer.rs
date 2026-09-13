use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

use crate::app::{App, Focus, downloads_table::DownloadAction};

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.focus {
        Focus::Downloads => {
            let rp = downloads_rp_hint(app.current_download_action());
            format!(
                "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   Enter: open/edit   f: open folder{rp}   d: delete   D: delete + files   v: import clipboard   q: quit"
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

fn downloads_rp_hint(action: Option<DownloadAction>) -> &'static str {
    action.map(DownloadAction::hint).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::downloads_rp_hint;
    use crate::app::downloads_table::DownloadAction;

    #[test]
    fn downloads_rp_hint_matches_each_action() {
        assert_eq!(downloads_rp_hint(None), "");
        assert_eq!(
            downloads_rp_hint(Some(DownloadAction::Start)),
            "   r: start"
        );
        assert_eq!(
            downloads_rp_hint(Some(DownloadAction::Resume)),
            "   r: resume"
        );
        assert_eq!(
            downloads_rp_hint(Some(DownloadAction::Pause)),
            "   p: pause"
        );
        assert_eq!(
            downloads_rp_hint(Some(DownloadAction::Retry)),
            "   r: retry"
        );
        assert_eq!(
            downloads_rp_hint(Some(DownloadAction::Restart)),
            "   r: restart"
        );
    }
}
