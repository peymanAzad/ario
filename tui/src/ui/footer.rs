use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

use crate::app::{App, Focus};

pub fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let help = match app.focus {
        Focus::Downloads => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   p: pause   r: resume   d: delete   v: import clipboard   q: quit"
        }
        Focus::Queues => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   n: new queue   Enter: edit queue   v: import clipboard   q: quit"
        }
        Focus::Categories => {
            "1/2/3 or Tab: switch pane   j/k ↑/↓: navigate   v: import clipboard   q: quit"
        }
    };
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(app.theme.text_muted)),
        area,
    );
}
