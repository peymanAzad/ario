use super::*;
use crate::app::{App, Focus};
use common::{download::DownloadLiveStatus, enums::DownloadStatus};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use unicode_width::UnicodeWidthStr;

fn download_name(d: &DownloadLiveStatus) -> &str {
    d.download.filename.as_deref().unwrap_or(&d.download.url)
}

fn progress(d: &DownloadLiveStatus) -> Option<String> {
    if d.download.status == DownloadStatus::Completed {
        return None;
    }
    d.download.size.filter(|&size| size > 0).map(|size| {
        let percent = (d.completed_length as f64 / size as f64 * 100.0).clamp(0.0, 100.0);
        format!("{percent:.1}%")
    })
}

// Explicit widths keep filename truncation and the table's actual cell layout in agreement.
fn column_widths(width: u16) -> Vec<u16> {
    let mut metrics = vec![9, 10, 12, 9];
    while metrics.len() > 1 && width < 12 + metrics.iter().sum::<u16>() + metrics.len() as u16 {
        metrics.pop();
    }
    if width < 10 {
        return vec![0, width.saturating_sub(1)];
    }
    let name = width.saturating_sub(metrics.iter().sum::<u16>() + metrics.len() as u16);
    let mut widths = vec![name];
    widths.extend(metrics);
    widths
}


fn download_detail_lines(
    download: &DownloadLiveStatus,
    width: usize,
    height: usize,
    ellipsis: &str,
) -> (Vec<String>, Vec<String>) {
    if height == 0 {
        return (Vec::new(), Vec::new());
    }

    let error = match &download.download.status {
        DownloadStatus::Error(message) if height >= 2 => Some(format!("Error: {message}")),
        _ => None,
    };
    let filename_height = height.saturating_sub(usize::from(error.is_some()));
    let filename = detail_lines(download_name(download), width, filename_height, ellipsis);
    let error_height = height.saturating_sub(filename.len());
    let error = error.map_or_else(Vec::new, |message| {
        crate::modal::help::wrap_text(&message, width)
            .into_iter()
            .take(error_height)
            .collect()
    });

    (filename, error)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmptyDownloadsState {
    Unfiltered,
    Filtered,
    Unavailable,
}

fn empty_downloads_state(app: &App) -> EmptyDownloadsState {
    if app.last_error.is_some() {
        EmptyDownloadsState::Unavailable
    } else if app.selected_queue > 0 || app.selected_category > 0 {
        EmptyDownloadsState::Filtered
    } else {
        EmptyDownloadsState::Unfiltered
    }
}

fn draw_empty_downloads(f: &mut Frame, app: &App, table_area: Rect) {
    // The table header owns the first row. Empty-state content may only use what remains.
    let body = Rect::new(
        table_area.x,
        table_area.y.saturating_add(1),
        table_area.width,
        table_area.height.saturating_sub(1),
    );
    if body.is_empty() {
        return;
    }

    let state = empty_downloads_state(app);
    let primary = match state {
        EmptyDownloadsState::Unfiltered => "No downloads yet",
        EmptyDownloadsState::Filtered => "No downloads match this view",
        EmptyDownloadsState::Unavailable => "Downloads unavailable",
    };
    let primary_color = if state == EmptyDownloadsState::Unavailable {
        app.theme.status_error
    } else {
        app.theme.foreground
    };
    let mut lines = vec![Line::styled(primary, Style::default().fg(primary_color))];

    if body.height >= 2 {
        let muted = Style::default().fg(app.theme.text_muted);
        match state {
            EmptyDownloadsState::Unfiltered => lines.push(Line::from(vec![
                Span::styled("v", Style::default().fg(app.theme.accent)),
                Span::styled(":Import Clipboard or ", muted),
                Span::styled("a", Style::default().fg(app.theme.accent)),
                Span::styled(":Import Torrent File", muted),
            ])),
            EmptyDownloadsState::Filtered => lines.push(Line::styled(
                "Select All queues and categories to view everything",
                muted,
            )),
            EmptyDownloadsState::Unavailable => {}
        }
    }

    let height = lines.len() as u16;
    let message_area = Rect::new(
        body.x,
        body.y + body.height.saturating_sub(height) / 2,
        body.width,
        height.min(body.height),
    );
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        message_area,
    );
}

pub fn draw_downloads_table(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Downloads;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(theme, focused))
        .title(Span::styled(
            " [3] Downloads ",
            Style::default().fg(theme.foreground),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let ellipsis = app.icons.ellipsis();
    let max_detail_height = (inner.height / 3).min(inner.height.saturating_sub(3));
    let (filename_details, error_details) = app.current_download().map_or_else(
        || (Vec::new(), Vec::new()),
        |download| {
            download_detail_lines(
                download,
                inner.width as usize,
                max_detail_height as usize,
                ellipsis,
            )
        },
    );
    let detail_height = (filename_details.len() + error_details.len()) as u16;
    let table_area = Rect {
        height: inner.height.saturating_sub(if detail_height > 0 {
            detail_height + 1
        } else {
            0
        }),
        ..inner
    };
    if detail_height > 0 {
        let divider = Rect::new(inner.x, table_area.bottom(), inner.width, 1);
        // ASCII mode also uses an ASCII separator in the detail area.
        let separator = if ellipsis == "..." { "-" } else { "─" };
        f.render_widget(
            Paragraph::new(separator.repeat(inner.width as usize))
                .style(border_style(theme, focused)),
            divider,
        );
        let detail_area = Rect::new(inner.x, divider.bottom(), inner.width, detail_height);
        let details = filename_details
            .into_iter()
            .map(|line| Line::from(line).style(Style::default().fg(theme.text_muted)))
            .chain(
                error_details
                    .into_iter()
                    .map(|line| Line::from(line).style(Style::default().fg(theme.status_error))),
            )
            .collect::<Vec<_>>();
        f.render_widget(Paragraph::new(details), detail_area);
    }

    let widths = column_widths(inner.width);
    let header = Row::new(
        ["Name", "Status", "Size", "Speed", "ETA"]
            .into_iter()
            .take(widths.len()),
    )
    .style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    let rows: Vec<Row> = app
        .downloads
        .iter()
        .map(|d| {
            let prefix = format!("{} ", app.icons.category(&d.download.category));
            let name_width = widths[0] as usize;
            let name = if name_width < prefix.width() {
                middle_truncate(&prefix, name_width, ellipsis)
            } else {
                format!(
                    "{prefix}{}",
                    middle_truncate(download_name(d), name_width - prefix.width(), ellipsis)
                )
            };
            let pausing = app.is_download_pausing(d.download.id);
            let mut status = if pausing {
                let marker = app.icons.ellipsis().chars().next().unwrap_or('.');
                vec![Span::styled(
                    format!("pausing{marker}"),
                    Style::default().fg(theme.status_warning),
                )]
            } else {
                vec![Span::styled(
                    app.icons.download_status(&d.download.status),
                    Style::default().fg(app.icons.download_status_color(&d.download.status, theme)),
                )]
            };
            if !pausing && let Some(percent) = progress(d) {
                status.push(Span::raw(format!(" {percent}")));
            }
            let completed = d.download.status == DownloadStatus::Completed;
            let mut cells = vec![
                Cell::from(name),
                Cell::from(Line::from(status)),
                Cell::from(
                    d.download
                        .size
                        .map(format_bytes)
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(if completed {
                    "-".to_string()
                } else {
                    format_speed(d.download_speed)
                }),
                Cell::from(if completed {
                    "-".to_string()
                } else {
                    d.eta_seconds
                        .map(format_eta)
                        .unwrap_or_else(|| "-".to_string())
                }),
            ];
            cells.truncate(widths.len());
            Row::new(cells)
        })
        .collect();

    let mut state = TableState::default();
    if app.current_download().is_some() {
        state.select(Some(app.selected_download));
    }
    let table = Table::new(rows, widths.into_iter().map(Constraint::Length))
        .column_spacing(1)
        .header(header)
        .row_highlight_style(highlight_style(theme, focused));
    f.render_stateful_widget(table, table_area, &mut state);
    if app.downloads.is_empty() {
        draw_empty_downloads(f, app, table_area);
    }
}

#[cfg(test)]
#[path = "downloads_table_tests.rs"]
mod tests;
