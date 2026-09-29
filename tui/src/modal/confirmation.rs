use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, PendingConfirmationAction};
use crate::effects::Effect;
use crate::modal::{Component, Ctx, Modal, ModalOutcome};
use crate::msg::Action;
use crate::ui::centered_rect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationModal {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action: PendingConfirmationAction,
}

impl ConfirmationModal {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
        action: PendingConfirmationAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            action,
        }
    }
}

impl Component for ConfirmationModal {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &Ctx<'_>) -> ModalOutcome {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                ModalOutcome::Emit(Action::Confirm(self.action.clone()))
            }
            KeyCode::Esc
            | KeyCode::Char('n')
            | KeyCode::Char('N')
            | KeyCode::Char('c')
            | KeyCode::Char('C') => ModalOutcome::Close,
            _ => ModalOutcome::Continue,
        }
    }

    fn render(&mut self, f: &mut Frame, _area: Rect, ctx: &Ctx<'_>) {
        let theme = ctx.theme;
        let area = centered_rect(58, 28, f.area());
        f.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.status_error))
            .title(Span::styled(
                format!(" {} ", self.title),
                Style::default()
                    .fg(theme.status_error)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        f.render_widget(
            Paragraph::new(self.message.as_str())
                .style(Style::default().fg(theme.foreground))
                .wrap(Wrap { trim: true }),
            layout[0],
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" Enter/y: {} ", self.confirm_label),
                    Style::default()
                        .fg(theme.status_error)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("Esc/n/c: {}", self.cancel_label),
                    Style::default().fg(theme.text_muted),
                ),
            ])),
            layout[1],
        );
    }

    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("Enter/y", "Confirm"),
            ("Esc/n/c", "Cancel"),
        ]
    }
}

impl App {
    pub fn open_confirmation(&mut self, modal: ConfirmationModal) {
        self.modal = Some(Modal::Confirmation(modal));
    }

    pub fn cancel_confirmation(&mut self) {
        if matches!(self.modal, Some(Modal::Confirmation(_))) {
            self.modal = None;
        }
    }

    pub fn execute_confirmation(&mut self, action: PendingConfirmationAction) -> Vec<Effect> {
        match action {
            PendingConfirmationAction::DeleteDownloadFiles { download_id } => {
                self.delete_download_files(download_id)
            }
            PendingConfirmationAction::DeleteQueue { queue_id } => {
                self.confirm_delete_queue(queue_id)
            }
        }
    }
}
