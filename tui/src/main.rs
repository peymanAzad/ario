mod api;
mod app;
mod event;
mod tui;
mod ui;
mod update;

use app::App;
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use tui::Tui;
use update::update;

const API_BASE: &str = "http://127.0.0.1:47812";
const TICK_RATE_MS: u64 = 500;

fn main() -> anyhow::Result<()> {
    let mut app = App::new(API_BASE.to_string());

    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(TICK_RATE_MS);
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    while !app.should_quit {
        tui.draw(&mut app)?;

        match tui.events.next()? {
            Event::Tick => app.refresh(),
            Event::Key(key_event) => update(&mut app, key_event),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
        }
    }

    tui.exit()?;
    Ok(())
}
