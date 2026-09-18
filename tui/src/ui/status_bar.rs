use crate::app::{ALL_CATEGORIES, App, LifecycleState};
use common::enums::DownloadStatus;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let status = server_status(app);
    let server_background = match status {
        ServerStatus::Up => theme.status_ok,
        ServerStatus::Starting | ServerStatus::Retrying => theme.status_warning,
        ServerStatus::Down | ServerStatus::Failed => theme.status_error,
    };
    let server_indicator = Span::styled(
        format!(" server: {} ", status.label()),
        Style::default().fg(theme.selected_fg).bg(server_background),
    );

    let aria2_indicator = if app.aria2_reachable {
        Span::styled(
            " aria2: up ",
            Style::default().fg(theme.selected_fg).bg(theme.status_ok),
        )
    } else {
        Span::styled(
            " aria2: down ",
            Style::default().fg(theme.foreground).bg(theme.status_error),
        )
    };

    let mut spans = vec![
        Span::styled(
            " Ario ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        server_indicator,
        aria2_indicator,
    ];

    if let Some(label) = filter_indicator_label(app) {
        spans.push(Span::styled(
            format!(" {label} "),
            Style::default().fg(theme.selected_fg).bg(theme.accent),
        ));
    }

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!("  {err}"),
            Style::default().fg(theme.status_error),
        ));
    }

    if let Some(DownloadStatus::Error(message)) = app
        .downloads
        .get(app.selected_download)
        .map(|download| &download.download.status)
    {
        spans.push(Span::styled(
            format!("  Download: {message}"),
            Style::default().fg(theme.status_error),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerStatus {
    Starting,
    Retrying,
    Up,
    Down,
    Failed,
}

impl ServerStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Retrying => "retrying",
            Self::Up => "up",
            Self::Down => "down",
            Self::Failed => "failed",
        }
    }
}

fn server_status(app: &App) -> ServerStatus {
    if !app.manages_server() {
        return if app.server_reachable {
            ServerStatus::Up
        } else {
            ServerStatus::Down
        };
    }

    match &app.lifecycle {
        LifecycleState::Starting => ServerStatus::Starting,
        LifecycleState::Retrying => ServerStatus::Retrying,
        LifecycleState::Connected if app.server_reachable => ServerStatus::Up,
        LifecycleState::Connected => ServerStatus::Down,
        LifecycleState::Failed(_) => ServerStatus::Failed,
    }
}

fn filter_indicator_label(app: &App) -> Option<String> {
    let queue_part = if app.selected_queue != 0 {
        app.queues
            .get(app.selected_queue - 1)
            .map(|q| q.name.clone())
    } else {
        None
    };

    let category_part = if app.selected_category != 0 {
        ALL_CATEGORIES
            .get(app.selected_category - 1)
            .map(super::category_label)
    } else {
        None
    };

    match (queue_part, category_part) {
        (None, None) => None,
        (Some(q), None) => Some(format!("Filter: {q}")),
        (None, Some(c)) => Some(format!("Filter: {c}")),
        (Some(q), Some(c)) => Some(format!("Filter: {q} · {c}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        icons::{GlyphMode, IconSet},
        theme::Theme,
    };
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::mpsc;

    fn app(managed: bool) -> App {
        let (sender, _receiver) = mpsc::channel();
        App::new(
            "http://127.0.0.1:1".into(),
            Theme::default_dark(),
            IconSet::new(GlyphMode::Ascii),
            sender,
            managed,
        )
    }

    #[test]
    fn managed_server_indicator_follows_lifecycle_and_current_health() {
        let mut app = app(true);
        assert_eq!(server_status(&app), ServerStatus::Starting);
        app.apply_lifecycle(LifecycleState::Retrying);
        assert_eq!(server_status(&app), ServerStatus::Retrying);
        app.apply_lifecycle(LifecycleState::Failed("startup failed".into()));
        assert_eq!(server_status(&app), ServerStatus::Failed);
        app.apply_lifecycle(LifecycleState::Connected);
        assert_eq!(server_status(&app), ServerStatus::Up);
        assert!(app.server_reachable);
        assert!(!app.aria2_reachable);
        app.apply_refresh(
            Err(anyhow::anyhow!("server unreachable")),
            Err(anyhow::anyhow!("server unreachable")),
            false,
            false,
            3, // Retrying, Failed, then Connected.
        );
        assert_eq!(server_status(&app), ServerStatus::Down);
    }

    #[test]
    fn unmanaged_server_uses_reachability_and_aria2_stays_separate() {
        let mut unmanaged = app(false);
        assert_eq!(server_status(&unmanaged), ServerStatus::Down);
        unmanaged.server_reachable = true;
        assert_eq!(server_status(&unmanaged), ServerStatus::Up);

        let mut managed = app(true);
        managed.apply_lifecycle(LifecycleState::Connected);
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal
            .draw(|frame| draw_status_bar(frame, &managed, frame.area()))
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("server: up"));
        assert!(rendered.contains("aria2: down"));
        assert!(!rendered.contains("connected"));
    }
}
