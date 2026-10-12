use crate::app::App;
use crate::event::EventHandler;
use crate::ui;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::panic;

pub type CrosstermTerminal = Terminal<CrosstermBackend<std::io::Stderr>>;

pub struct Tui {
    terminal: CrosstermTerminal,
    pub events: EventHandler,
    mouse_enabled: bool,
}

impl Tui {
    pub fn new(terminal: CrosstermTerminal, events: EventHandler, mouse_enabled: bool) -> Self {
        Self {
            terminal,
            events,
            mouse_enabled,
        }
    }

    pub fn enter(&mut self) -> anyhow::Result<()> {
        if let Err(error) = self.enter_inner() {
            let _ = Self::reset();
            return Err(error);
        }
        Ok(())
    }

    fn enter_inner(&mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen, EnableBracketedPaste)?;
        if self.mouse_enabled {
            execute!(io::stderr(), EnableMouseCapture)?;
        }

        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = Self::reset();
            panic_hook(info);
        }));

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    fn reset() -> anyhow::Result<()> {
        let raw_result = disable_raw_mode();
        let screen_result = execute!(
            io::stderr(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        raw_result?;
        screen_result?;
        Ok(())
    }

    pub fn exit(&mut self) -> anyhow::Result<()> {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    pub fn draw(&mut self, app: &mut App) -> anyhow::Result<()> {
        self.terminal.draw(|frame| ui::render(app, frame))?;
        Ok(())
    }
}
