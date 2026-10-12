//! Geometry recorded by the renderer is the source of truth for pointer input.
use crate::{
    app::{App, Focus},
    modal::{Ctx, Modal},
    msg::{Action, Msg},
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::Span,
    widgets::{List, ListState, TableState},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    mem::Discriminant,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollTarget {
    Queues,
    Categories,
    Downloads,
    Urls,
    Fields,
    Common,
    Scheduler,
    Items,
    Help,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Save,
    Start,
    Later,
    Cancel,
    Confirm,
    All,
    None,
    Accept,
    Discard,
    Erase,
    NextTab,
    PrevTab,
    ItemUp,
    ItemDown,
    Search,
    PageUp,
    PageDown,
    Home,
    End,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalIntent {
    Tab(usize),
    Field(usize),
    Adjust { field: Option<usize>, forward: bool },
    ToggleUrl(usize),
    Day(usize),
    Text,
    Queue,
    Navigate(bool),
    HelpScroll(bool),
    Command(Command),
}
#[derive(Clone, Debug)]
pub enum Target {
    Focus(Focus),
    Row(Focus, usize, Option<i64>),
    Action(Action),
    Modal(ModalIntent),
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub area: Rect,
    pub target: Target,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowIdentity {
    Queue(i64),
    Download(i64),
}
#[derive(Debug)]
pub struct Click {
    identity: RowIdentity,
    at: Instant,
}
#[derive(Default, Debug)]
pub struct Interactions {
    pub hits: Vec<Hit>,
    pub scrolls: Vec<(Rect, ScrollTarget)>,
    pub lists: HashMap<ScrollTarget, ListState>,
    pub table: TableState,
    pub last_click: Option<Click>,
    modal: Option<Discriminant<Modal>>,
}
impl Interactions {
    pub fn begin_frame(&mut self, modal: Option<Discriminant<Modal>>) {
        self.hits.clear();
        self.scrolls.clear();
        if self.modal != modal {
            self.last_click = None;
            self.lists
                .retain(|key, _| matches!(key, ScrollTarget::Queues | ScrollTarget::Categories));
        }
        self.modal = modal;
    }
    pub fn add(&mut self, area: Rect, target: Target) {
        if !area.is_empty() {
            self.hits.push(Hit { area, target });
        }
    }
    pub fn scroll(&mut self, area: Rect, target: ScrollTarget) {
        if !area.is_empty() {
            self.scrolls.push((area, target));
        }
    }
    fn double_click(&mut self, identity: RowIdentity, now: Instant) -> bool {
        let double = self.last_click.as_ref().is_some_and(|click| {
            click.identity == identity
                && now.saturating_duration_since(click.at) <= Duration::from_millis(400)
        });
        self.last_click = if double {
            None
        } else {
            Some(Click { identity, at: now })
        };
        double
    }
}

/// Register spans using display-cell widths and exactly the renderer's clipping.
pub fn spans(
    map: &RefCell<Interactions>,
    area: Rect,
    spans: &[Span<'_>],
    targets: &[(usize, Target)],
) {
    if area.is_empty() {
        return;
    }
    let mut x = area.x;
    for (index, span) in spans.iter().enumerate() {
        let remaining = area.right().saturating_sub(x) as usize;
        let mut width = 0usize;
        let mut truncated = false;
        for grapheme in span.content.graphemes(true) {
            let cells = grapheme.width();
            if cells > area.width as usize {
                continue;
            }
            if width + cells > remaining {
                truncated = true;
                break;
            }
            width += cells;
        }
        let width = width as u16;
        if let Some((_, target)) = targets.iter().find(|(i, _)| *i == index) {
            map.borrow_mut()
                .add(Rect::new(x, area.y, width, 1), target.clone());
        }
        x = x.saturating_add(width);
        if truncated {
            break;
        }
    }
}
pub fn modal_target(intent: ModalIntent) -> Target {
    Target::Modal(intent)
}
pub fn command(command: Command) -> Target {
    modal_target(ModalIntent::Command(command))
}

/// Render a one-line-per-item list and expose its final, selection-adjusted offset.
pub fn list(
    f: &mut Frame,
    map: &RefCell<Interactions>,
    key: ScrollTarget,
    widget: List<'_>,
    areas: (Rect, Rect),
    selected: usize,
    len: usize,
) -> usize {
    let (area, inner) = areas;
    let mut map = map.borrow_mut();
    let state = map.lists.entry(key).or_default();
    state.select((len > 0).then_some(selected.min(len.saturating_sub(1))));
    *state.offset_mut() = state.offset().min(len.saturating_sub(1));
    f.render_stateful_widget(widget, area, state);
    let offset = state.offset();
    map.scroll(inner, key);
    offset
}
/// Field body selects; only the visible arrow cells adjust values.
pub fn field(map: &RefCell<Interactions>, area: Rect, index: usize, text: &str) {
    map.borrow_mut()
        .add(area, modal_target(ModalIntent::Field(index)));
    for (symbol, forward) in [('◀', false), ('▶', true)] {
        if let Some(byte) = text.find(symbol) {
            let x = text[..byte].width();
            if x < area.width as usize {
                map.borrow_mut().add(
                    Rect::new(area.x + x as u16, area.y, 1, area.height.min(1)),
                    modal_target(ModalIntent::Adjust {
                        field: Some(index),
                        forward,
                    }),
                );
            }
        }
    }
}

pub fn route_mouse(app: &mut App, event: MouseEvent) -> Option<Msg> {
    route_mouse_at(app, event, Instant::now())
}
pub fn route_mouse_at(app: &mut App, event: MouseEvent, now: Instant) -> Option<Msg> {
    let position = Position::new(event.column, event.row);
    match event.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let forward = event.kind == MouseEventKind::ScrollDown;
            let target = {
                let mut map = app.interactions.borrow_mut();
                map.last_click = None;
                map.scrolls
                    .iter()
                    .rev()
                    .find(|(area, _)| area.contains(position))
                    .map(|(_, key)| *key)
            }?;
            if app.modal.is_some() {
                match target {
                    ScrollTarget::Help => dispatch(app, ModalIntent::HelpScroll(forward)),
                    ScrollTarget::Urls
                    | ScrollTarget::Fields
                    | ScrollTarget::Common
                    | ScrollTarget::Scheduler
                    | ScrollTarget::Items => dispatch(app, ModalIntent::Navigate(forward)),
                    _ => None,
                }
            } else {
                app.focus = match target {
                    ScrollTarget::Queues => Focus::Queues,
                    ScrollTarget::Categories => Focus::Categories,
                    ScrollTarget::Downloads => Focus::Downloads,
                    _ => return None,
                };
                Some(Msg::Action(if forward {
                    Action::SelectNext
                } else {
                    Action::SelectPrev
                }))
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let target = app
                .interactions
                .borrow()
                .hits
                .iter()
                .rev()
                .find(|hit| hit.area.contains(position))
                .map(|hit| hit.target.clone())?;
            if app.modal.is_some() {
                app.interactions.borrow_mut().last_click = None;
                return match target {
                    Target::Modal(intent) => dispatch(app, intent),
                    _ => None,
                };
            }
            match target {
                Target::Focus(focus) => {
                    app.interactions.borrow_mut().last_click = None;
                    Some(Msg::Action(Action::Focus(focus)))
                }
                Target::Action(action) => {
                    app.interactions.borrow_mut().last_click = None;
                    app.action_available(&action).then_some(Msg::Action(action))
                }
                Target::Row(focus, index, id) => {
                    if app.focus != focus {
                        app.interactions.borrow_mut().last_click = None;
                    }
                    app.focus = focus;
                    // Resolve stable identities again in case a refresh changed the data.
                    let index = match (focus, id) {
                        (Focus::Downloads, Some(id)) => {
                            app.downloads.iter().position(|d| d.download.id == id)?
                        }
                        (Focus::Queues, Some(id)) => {
                            app.queues.iter().position(|q| q.id == id)? + 1
                        }
                        _ => index,
                    };
                    let identity = match (focus, id) {
                        (Focus::Queues, Some(id)) => Some(RowIdentity::Queue(id)),
                        (Focus::Downloads, Some(id)) => Some(RowIdentity::Download(id)),
                        _ => None,
                    };
                    let double = identity
                        .is_some_and(|id| app.interactions.borrow_mut().double_click(id, now));
                    if identity.is_none() {
                        app.interactions.borrow_mut().last_click = None;
                    }
                    if double {
                        return Some(Msg::Action(match focus {
                            Focus::Queues => Action::OpenEditQueue,
                            Focus::Downloads => Action::ActivateDownload,
                            _ => return None,
                        }));
                    }
                    Some(Msg::Action(Action::SelectAt { focus, index }))
                }
                Target::Modal(_) => None,
            }
        }
        _ => None,
    }
}
fn dispatch(app: &mut App, intent: ModalIntent) -> Option<Msg> {
    let App {
        modal,
        interactions,
        queues,
        selected_queue,
        aria2_global_options,
        theme,
        icons,
        ..
    } = app;
    let ctx = Ctx {
        interactions,
        queues,
        selected_queue: *selected_queue,
        aria2_global_options: aria2_global_options.as_ref(),
        theme,
        icons,
    };
    let outcome = modal.as_mut()?.handle_intent(intent, &ctx);
    crate::keymap::finish_modal_outcome(modal, outcome)
}

pub fn fields(
    f: &mut Frame,
    ctx: &Ctx<'_>,
    key: ScrollTarget,
    area: Rect,
    selected: usize,
    rows: &[String],
    title: &str,
) -> (Rect, usize) {
    use ratatui::{
        style::Style,
        widgets::{Block, Borders, ListItem},
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ctx.theme.border))
        .title(title.to_owned());
    let inner = block.inner(area);
    let items = rows
        .iter()
        .enumerate()
        .map(|(index, text)| {
            ListItem::new(text.clone()).style(crate::ui::field_style(ctx.theme, index == selected))
        })
        .collect::<Vec<_>>();
    let offset = list(
        f,
        ctx.interactions,
        key,
        List::new(items).block(block),
        (area, inner),
        selected,
        rows.len(),
    );
    for y in 0..inner.height {
        let index = offset + y as usize;
        let Some(text) = rows.get(index) else {
            break;
        };
        let area = Rect::new(inner.x, inner.y + y, inner.width, 1);
        if key == ScrollTarget::Items {
            ctx.interactions
                .borrow_mut()
                .add(area, modal_target(ModalIntent::Field(index)));
        } else {
            field(ctx.interactions, area, index, text);
        }
    }
    (inner, offset)
}

/// Footer labels can contain aliases or paired commands; aliases share a target,
/// while navigation/adjustment pairs get separate visible controls.
pub fn modal_hint(key: &str, modal: &Modal) -> Option<ModalIntent> {
    let editing = match modal {
        Modal::Queue(m) => m.name_input.editing,
        Modal::TorrentFile(m) => m.path_input.editing,
        Modal::Help(m) => m.editing_search,
        _ => false,
    };
    Some(match key {
        "s" => ModalIntent::Command(match modal {
            Modal::ClipboardImport(_) | Modal::TorrentFile(_) => Command::Start,
            _ => Command::Save,
        }),
        "w" => ModalIntent::Command(Command::Later),
        "c/Esc" | "Esc/n/c" | "Esc/q/?" => ModalIntent::Command(Command::Cancel),
        "Enter/y" => ModalIntent::Command(Command::Confirm),
        "Tab" => ModalIntent::Command(Command::NextTab),
        "Shift+Tab" => ModalIntent::Command(Command::PrevTab),
        "j" => ModalIntent::Navigate(true),
        "k" => ModalIntent::Navigate(false),
        "h" => ModalIntent::Adjust {
            field: None,
            forward: false,
        },
        "l" => ModalIntent::Adjust {
            field: None,
            forward: true,
        },
        "J" => ModalIntent::Command(Command::ItemDown),
        "K" => ModalIntent::Command(Command::ItemUp),
        "/" => ModalIntent::Command(Command::Search),
        "Enter" => ModalIntent::Command(Command::Accept),
        "Esc" if editing => ModalIntent::Command(Command::Discard),
        "Backspace" => ModalIntent::Command(Command::Erase),
        "Esc" => ModalIntent::Command(Command::Cancel),
        _ => return None,
    })
}

pub fn selector(map: &RefCell<Interactions>, area: Rect, text: &str) {
    map.borrow_mut().add(area, modal_target(ModalIntent::Queue));
    for (symbol, forward) in [('◀', false), ('▶', true)] {
        if let Some(byte) = text.find(symbol) {
            let x = text[..byte].width();
            if x < area.width as usize {
                map.borrow_mut().add(
                    Rect::new(area.x + x as u16, area.y, 1, area.height.min(1)),
                    modal_target(ModalIntent::Adjust {
                        field: None,
                        forward,
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod tests;
