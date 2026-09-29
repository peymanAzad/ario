use crossterm::event::{KeyCode, KeyEvent};
use common::enums::{DownloadStatus, QueueStatus};

use crate::app::{App, Focus, downloads::DownloadAction};
use crate::msg::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Pane(Focus),
}

pub struct Binding {
    pub keys: &'static [KeyCode],
    /// Key column in help (e.g. `"j / Down"`, `"q / Esc"`).
    pub keys_label: &'static str,
    pub scope: Scope,
    pub action: Action,
    /// Default short footer label; contextual overrides in `footer_hints` may replace it.
    #[allow(dead_code)]
    pub label: &'static str,
    /// Help description.
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterHint {
    pub key: &'static str,
    pub label: &'static str,
}

impl FooterHint {
    pub const fn new(key: &'static str, label: &'static str) -> Self {
        Self { key, label }
    }

    pub fn width(self) -> usize {
        use unicode_width::UnicodeWidthStr;
        self.key.width() + 1 + self.label.width()
    }
}

pub static BINDINGS: &[Binding] = &[
    // Global
    Binding {
        keys: &[KeyCode::Char('?')],
        keys_label: "?",
        scope: Scope::Global,
        action: Action::OpenHelp,
        label: "Help",
        description: "Open keybindings help (main screen only)",
    },
    Binding {
        keys: &[KeyCode::Char('1')],
        keys_label: "1",
        scope: Scope::Global,
        action: Action::Focus(Focus::Queues),
        label: "Queues",
        description: "Focus Queues pane",
    },
    Binding {
        keys: &[KeyCode::Char('2')],
        keys_label: "2",
        scope: Scope::Global,
        action: Action::Focus(Focus::Categories),
        label: "Categories",
        description: "Focus Categories pane",
    },
    Binding {
        keys: &[KeyCode::Char('3')],
        keys_label: "3",
        scope: Scope::Global,
        action: Action::Focus(Focus::Downloads),
        label: "Downloads",
        description: "Focus Downloads pane",
    },
    Binding {
        keys: &[KeyCode::Tab],
        keys_label: "Tab",
        scope: Scope::Global,
        action: Action::FocusNext,
        label: "Pane",
        description: "Focus next pane",
    },
    Binding {
        keys: &[KeyCode::BackTab],
        keys_label: "Shift+Tab",
        scope: Scope::Global,
        action: Action::FocusPrev,
        label: "Pane",
        description: "Focus previous pane",
    },
    Binding {
        keys: &[KeyCode::Char('a')],
        keys_label: "a",
        scope: Scope::Global,
        action: Action::OpenTorrentFile,
        label: "Torrent",
        description: "Add a local .torrent file",
    },
    Binding {
        keys: &[KeyCode::Char('v')],
        keys_label: "v",
        scope: Scope::Global,
        action: Action::OpenClipboardImport,
        label: "Import Clipboard",
        description: "Import URLs and magnet links from clipboard",
    },
    Binding {
        keys: &[KeyCode::Esc, KeyCode::Char('q')],
        keys_label: "q / Esc",
        scope: Scope::Global,
        action: Action::Quit,
        label: "Quit",
        description: "Quit from the main screen",
    },
    // Downloads
    Binding {
        keys: &[KeyCode::Down, KeyCode::Char('j')],
        keys_label: "j / Down",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::SelectNext,
        label: "Navigate",
        description: "Select next download",
    },
    Binding {
        keys: &[KeyCode::Up, KeyCode::Char('k')],
        keys_label: "k / Up",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::SelectPrev,
        label: "Navigate",
        description: "Select previous download",
    },
    Binding {
        keys: &[KeyCode::Enter],
        keys_label: "Enter",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::ActivateDownload,
        label: "Edit",
        description: "Open completed file; edit other downloads",
    },
    Binding {
        keys: &[KeyCode::Char('f')],
        keys_label: "f",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::OpenDownloadFolder,
        label: "Folder",
        description: "Open folder of completed download",
    },
    Binding {
        keys: &[KeyCode::Char('p')],
        keys_label: "p",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::PauseDownload,
        label: "Pause",
        description: "Pause active download",
    },
    Binding {
        keys: &[KeyCode::Char('r')],
        keys_label: "r",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::ResumeDownload,
        label: "Resume",
        description:
            "Start pending, resume paused, retry failed/removed, or restart completed download",
    },
    Binding {
        keys: &[KeyCode::Char('d')],
        keys_label: "d",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::DeleteDownload,
        label: "Delete",
        description: "Delete download, keeping downloaded files",
    },
    Binding {
        keys: &[KeyCode::Char('D')],
        keys_label: "D",
        scope: Scope::Pane(Focus::Downloads),
        action: Action::DeleteDownloadFiles,
        label: "Delete+files",
        description: "Delete download and files after confirmation",
    },
    // Queues
    Binding {
        keys: &[KeyCode::Down, KeyCode::Char('j')],
        keys_label: "j / Down",
        scope: Scope::Pane(Focus::Queues),
        action: Action::SelectNext,
        label: "Navigate",
        description: "Select next queue",
    },
    Binding {
        keys: &[KeyCode::Up, KeyCode::Char('k')],
        keys_label: "k / Up",
        scope: Scope::Pane(Focus::Queues),
        action: Action::SelectPrev,
        label: "Navigate",
        description: "Select previous queue",
    },
    Binding {
        keys: &[KeyCode::Char('n')],
        keys_label: "n",
        scope: Scope::Pane(Focus::Queues),
        action: Action::OpenCreateQueue,
        label: "New",
        description: "Create a queue",
    },
    Binding {
        keys: &[KeyCode::Enter],
        keys_label: "Enter",
        scope: Scope::Pane(Focus::Queues),
        action: Action::OpenEditQueue,
        label: "Edit",
        description: "Edit selected queue (except All)",
    },
    Binding {
        keys: &[KeyCode::Char('p')],
        keys_label: "p",
        scope: Scope::Pane(Focus::Queues),
        action: Action::PauseQueue,
        label: "Pause",
        description: "Pause selected queue (except All)",
    },
    Binding {
        keys: &[KeyCode::Char('r')],
        keys_label: "r",
        scope: Scope::Pane(Focus::Queues),
        action: Action::ResumeQueue,
        label: "Resume",
        description: "Resume selected queue (except All)",
    },
    Binding {
        keys: &[KeyCode::Char('x')],
        keys_label: "x",
        scope: Scope::Pane(Focus::Queues),
        action: Action::RemoveCompleted,
        label: "Clear completed",
        description:
            "Remove completed downloads from selected queue, or all queues in All",
    },
    Binding {
        keys: &[KeyCode::Char('d')],
        keys_label: "d",
        scope: Scope::Pane(Focus::Queues),
        action: Action::DeleteQueue,
        label: "Delete",
        description:
            "Delete selected queue (except All and Main Queue); confirm if it contains downloads",
    },
    // Categories
    Binding {
        keys: &[KeyCode::Down, KeyCode::Char('j')],
        keys_label: "j / Down",
        scope: Scope::Pane(Focus::Categories),
        action: Action::SelectNext,
        label: "Navigate",
        description: "Select next category",
    },
    Binding {
        keys: &[KeyCode::Up, KeyCode::Char('k')],
        keys_label: "k / Up",
        scope: Scope::Pane(Focus::Categories),
        action: Action::SelectPrev,
        label: "Navigate",
        description: "Select previous category",
    },
];

/// Resolve a main-screen key for the current focus. Pane-scoped bindings only
/// match when `focus` equals their pane; global bindings always match.
pub fn lookup(focus: Focus, key: KeyEvent) -> Option<Action> {
    let code = key.code;
    // Prefer pane-scoped bindings so focus-specific keys (p/r/d) win over globals.
    for binding in BINDINGS {
        if !keys_match(binding.keys, code) {
            continue;
        }
        match binding.scope {
            Scope::Pane(pane) if pane == focus => return Some(binding.action.clone()),
            Scope::Global => {}
            Scope::Pane(_) => {}
        }
    }
    for binding in BINDINGS {
        if matches!(binding.scope, Scope::Global) && keys_match(binding.keys, code) {
            return Some(binding.action.clone());
        }
    }
    None
}

fn keys_match(keys: &[KeyCode], code: KeyCode) -> bool {
    keys.contains(&code)
}

/// Help sections for the main screen, grouped by scope/pane.
pub fn main_help_sections() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let mut sections = Vec::new();
    push_scope_section(&mut sections, "Main Navigation", Scope::Global);
    // Preserve the historical Ctrl+C note that is special-cased in route_key.
    if let Some((_, bindings)) = sections.last_mut() {
        bindings.push((
            "Ctrl+C",
            "Quit, except in confirmations where it cancels",
        ));
    }
    push_scope_section(&mut sections, "Downloads", Scope::Pane(Focus::Downloads));
    push_scope_section(&mut sections, "Queues", Scope::Pane(Focus::Queues));
    // Merge pause/resume into one help row to match historical wording.
    if let Some((_, bindings)) = sections.iter_mut().find(|(title, _)| *title == "Queues") {
        let pause_idx = bindings.iter().position(|(k, _)| *k == "p");
        let resume_idx = bindings.iter().position(|(k, _)| *k == "r");
        if let (Some(p), Some(r)) = (pause_idx, resume_idx) {
            let (first, second) = if p < r { (p, r) } else { (r, p) };
            bindings.remove(second);
            bindings[first] = ("p / r", "Pause / resume selected queue (except All)");
        }
    }
    push_scope_section(
        &mut sections,
        "Categories",
        Scope::Pane(Focus::Categories),
    );
    // Merge focus 1/2/3 and Tab/BackTab into historical combined rows.
    if let Some((_, bindings)) = sections.iter_mut().find(|(title, _)| *title == "Main Navigation")
    {
        coalesce_focus_rows(bindings);
    }
    sections
}

fn push_scope_section(
    sections: &mut Vec<(&'static str, Vec<(&'static str, &'static str)>)>,
    title: &'static str,
    scope: Scope,
) {
    let bindings: Vec<_> = BINDINGS
        .iter()
        .filter(|b| b.scope == scope)
        .map(|b| (b.keys_label, b.description))
        .collect();
    if !bindings.is_empty() {
        sections.push((title, bindings));
    }
}

fn coalesce_focus_rows(bindings: &mut Vec<(&'static str, &'static str)>) {
    let ones: Vec<usize> = bindings
        .iter()
        .enumerate()
        .filter(|(_, (k, _))| matches!(*k, "1" | "2" | "3"))
        .map(|(i, _)| i)
        .collect();
    if ones.len() == 3 {
        let insert_at = ones[0];
        for &i in ones.iter().rev() {
            bindings.remove(i);
        }
        bindings.insert(
            insert_at,
            ("1 / 2 / 3", "Focus Queues / Categories / Downloads"),
        );
    }
    let tab = bindings.iter().position(|(k, _)| *k == "Tab");
    let back = bindings.iter().position(|(k, _)| *k == "Shift+Tab");
    if let (Some(t), Some(b)) = (tab, back) {
        let (first, second) = if t < b { (t, b) } else { (b, t) };
        bindings.remove(second);
        bindings[first] = ("Tab / Shift+Tab", "Focus next / previous pane");
    }
}

/// Contextual footer hints for the main screen (no modal).
pub fn footer_hints(app: &App) -> Vec<FooterHint> {
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

    if app.action_available(&Action::DeleteDownload) {
        if let Some(action) = app.current_download_action() {
            hints.push(download_action_hint(action));
        }
        let enter_label = match app.current_download() {
            Some(d) if d.download.status == DownloadStatus::Completed => "Open",
            _ => "Edit",
        };
        hints.push(FooterHint::new("Enter", enter_label));
        if app.action_available(&Action::OpenDownloadFolder) {
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
    } else if app.current_queue().is_some() {
        if app.action_available(&Action::PauseQueue) {
            hints.push(FooterHint::new("p", "Pause"));
        } else if app.action_available(&Action::ResumeQueue) {
            hints.push(FooterHint::new("r", "Resume"));
        }
        hints.extend([
            FooterHint::new("Enter", "Edit"),
            FooterHint::new("n", "New"),
            FooterHint::new("x", "Clear completed"),
        ]);
        if app.action_available(&Action::DeleteQueue) {
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

impl App {
    /// Whether a main-screen action is currently meaningful for footer hints.
    pub fn action_available(&self, action: &Action) -> bool {
        match action {
            Action::OpenHelp
            | Action::Quit
            | Action::FocusNext
            | Action::FocusPrev
            | Action::Focus(_)
            | Action::SelectNext
            | Action::SelectPrev
            | Action::OpenClipboardImport
            | Action::OpenTorrentFile
            | Action::OpenCreateQueue
            | Action::RemoveCompleted => true,
            Action::PauseDownload => {
                self.current_download_action() == Some(DownloadAction::Pause)
            }
            Action::ResumeDownload => self
                .current_download_action()
                .is_some_and(|action| action.key() == 'r'),
            Action::ActivateDownload | Action::DeleteDownload | Action::DeleteDownloadFiles => {
                self.current_download().is_some()
            }
            Action::OpenDownloadFolder => self.current_download().is_some_and(|download| {
                download.download.status == DownloadStatus::Completed
            }),
            Action::OpenEditQueue => self.selected_queue != 0 && self.current_queue().is_some(),
            Action::PauseQueue => self
                .current_queue()
                .is_some_and(|queue| queue.status == QueueStatus::Active),
            Action::ResumeQueue => self
                .current_queue()
                .is_some_and(|queue| queue.status == QueueStatus::Paused),
            Action::DeleteQueue => self.can_delete_selected_queue(),
            _ => false,
        }
    }
}
