use crate::{
    app::help_modal::{HelpModal, filtered_sections},
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Pre-wrap into physical lines so navigation and resize clamping use exactly
/// the same line count as rendering, including narrow terminal layouts.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.width() + 1 + word.width() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        for grapheme in word.graphemes(true) {
            if !line.is_empty() && line.width() + grapheme.width() > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push_str(grapheme);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn help_lines(query: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let sections = filtered_sections(query);
    if sections.is_empty() {
        return wrap_text("No matching keybindings", width)
            .into_iter()
            .map(Line::from)
            .collect();
    }
    let key_style = Style::default().fg(theme.accent);
    let mut lines = Vec::new();
    for (title, bindings) in sections {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.extend(
            wrap_text(title, width)
                .into_iter()
                .map(|line| Line::from(line).style(key_style.add_modifier(Modifier::BOLD))),
        );
        lines.push(Line::from("─".repeat(width)).style(Style::default().fg(theme.border)));
        for (keys, description) in bindings {
            if width < 32 {
                lines.extend(
                    wrap_text(keys, width)
                        .into_iter()
                        .map(|line| Line::from(line).style(key_style)),
                );
                lines.extend(wrap_text(description, width).into_iter().map(Line::from));
                lines.push(Line::default());
            } else {
                let key_width = 22.min(width / 2);
                let key_lines = wrap_text(keys, key_width);
                let description_lines = wrap_text(description, width - key_width - 2);
                for index in 0..key_lines.len().max(description_lines.len()) {
                    let key = key_lines.get(index).cloned().unwrap_or_default();
                    let padding = " ".repeat(key_width - key.width() + 2);
                    lines.push(Line::from(vec![
                        Span::styled(key, key_style),
                        Span::raw(padding),
                        Span::raw(description_lines.get(index).cloned().unwrap_or_default()),
                    ]));
                }
            }
        }
    }
    lines
}

pub fn draw_help_modal(f: &mut Frame, modal: &mut HelpModal, theme: &Theme, body: Rect) {
    let width = if body.width > 40 {
        body.width.saturating_sub(4).min(100)
    } else {
        body.width
    };
    let height = if body.height > 12 {
        body.height.saturating_sub(2).min(40)
    } else {
        body.height
    };
    let area = Rect::new(
        body.x + (body.width - width) / 2,
        body.y + (body.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " Keybindings ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    let lines = help_lines(&modal.query, layout[1].width as usize, theme);
    modal.set_dimensions(lines.len(), layout[1].height as usize);
    let end = (modal.scroll + modal.viewport_height).min(lines.len());
    let first = if end == 0 { 0 } else { modal.scroll + 1 };
    f.render_widget(
        block.title_bottom(format!(" {first}-{end}/{} ", lines.len())),
        area,
    );
    f.render_widget(
        Paragraph::new(lines[modal.scroll..end].to_vec())
            .style(Style::default().fg(theme.foreground)),
        layout[1],
    );

    // Keep the end of a long Unicode query and the input cursor visible.
    let search = format!("/ {}", modal.query);
    let mut visible = search.as_str();
    let available = layout[0]
        .width
        .saturating_sub(u16::from(modal.editing_search)) as usize;
    while visible.width() > available {
        let Some(first) = visible.graphemes(true).next() else {
            break;
        };
        visible = &visible[first.len()..];
    }
    let search_style = Style::default().fg(if modal.editing_search {
        theme.accent
    } else {
        theme.text_muted
    });
    f.render_widget(
        Paragraph::new(if modal.query.is_empty() && !modal.editing_search {
            "/: search keybindings"
        } else {
            visible
        })
        .style(search_style),
        layout[0],
    );
    if modal.editing_search && layout[0].width > 0 && layout[0].height > 0 {
        f.set_cursor_position((layout[0].x + visible.width() as u16, layout[0].y));
    }
    let hints = if modal.editing_search {
        "Enter: keep  Esc: clear  Backspace: erase"
    } else {
        "Esc/q/?: close  /: search  j/k: scroll  PgUp/PgDn  Home/End"
    };
    f.render_widget(
        Paragraph::new(hints).style(Style::default().fg(theme.text_muted)),
        layout[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, Focus},
        icons::{GlyphMode, IconSet},
    };
    use ratatui::{Terminal, backend::TestBackend};
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

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(app, frame))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn help_renders_sections_dividers_and_filtered_descriptions() {
        let mut app = app();
        app.open_help_modal();
        let output = render(&mut app, 100, 30);
        assert!(output.contains("Keybindings"));
        assert!(output.contains("Main Navigation"));
        assert!(output.contains("Queues"));
        assert!(output.contains(&"─".repeat(30)));
        assert!(output.contains("/: search keybindings"));
        assert!(output.lines().last().unwrap().starts_with("?: Help"));

        app.help_modal.as_mut().unwrap().query = "keeping downloaded files".into();
        let output = render(&mut app, 60, 24);
        assert!(output.contains("Delete download, keeping"));
        assert!(output.contains("downloaded files"));
        assert!(!output.contains("Main Navigation"));
        assert!(!output.contains("Queue Editor"));

        app.help_modal.as_mut().unwrap().query = "nonexistent shortcut".into();
        let output = render(&mut app, 60, 24);
        assert!(output.contains("No matching keybindings"));
        assert_eq!(app.help_modal.as_ref().unwrap().scroll, 0);
    }

    #[test]
    fn resizing_and_filtering_clamp_to_the_rendered_line_count() {
        let mut app = app();
        app.open_help_modal();
        render(&mut app, 40, 12);
        app.help_modal.as_mut().unwrap().scroll_down(usize::MAX);
        let narrow_end = app.help_modal.as_ref().unwrap().scroll;
        let output = render(&mut app, 120, 50);
        let modal = app.help_modal.as_ref().unwrap();
        assert!(modal.scroll < narrow_end);
        assert_eq!(modal.scroll, modal.max_scroll());
        assert!(output.contains("Close help"));
        app.help_modal.as_mut().unwrap().query = "keeping downloaded files".into();
        render(&mut app, 120, 50);
        assert_eq!(app.help_modal.as_ref().unwrap().scroll, 0);
    }

    #[test]
    fn footer_remains_visible_in_all_panes_and_modal_states() {
        for focus in [Focus::Queues, Focus::Categories, Focus::Downloads] {
            let mut app = app();
            app.focus = focus;
            for width in [7, 30, 80, 120] {
                let output = render(&mut app, width, 24);
                assert!(output.lines().last().unwrap().starts_with("?: Help"));
            }
            app.open_create_queue_modal();
            let output = render(&mut app, 40, 12);
            assert!(output.lines().last().unwrap().starts_with("?: Help"));
            app.cancel_queue_modal();
            app.open_help_modal();
            for (width, height) in [(7, 4), (30, 12), (80, 24), (120, 40)] {
                let output = render(&mut app, width, height);
                assert!(output.lines().last().unwrap().starts_with("?: Help"));
            }
        }
    }

    #[test]
    fn tiny_help_and_long_unicode_search_render_without_overflow() {
        let theme = Theme::default_dark();
        let mut modal = HelpModal::default();
        modal.query = "é界👩‍💻".repeat(30);
        modal.editing_search = true;
        for (width, height) in [(1, 1), (2, 2), (8, 5), (30, 12), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    draw_help_modal(frame, &mut modal, &theme, frame.area());
                })
                .unwrap();
            assert!(modal.scroll <= modal.max_scroll());
        }
    }

    #[test]
    fn catalog_wraps_without_losing_descriptions() {
        for width in [12, 30, 32, 60, 96] {
            let lines = help_lines("", width, &Theme::default_dark());
            assert!(lines.iter().all(|line| line.width() <= width));
            let text = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("─"));
            assert!(text.contains("Confirmations") || width < 13);
        }
        assert_eq!(
            wrap_text("keep downloaded files", 10),
            ["keep", "downloaded", "files"]
        );
    }
}
