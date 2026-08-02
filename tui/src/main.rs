mod api;
mod app;
mod config;
mod event;
mod theme;
mod tui;
mod ui;
mod update;

use app::App;
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use theme::Theme;
use tui::Tui;
use update::update;

use crate::app::AppEvent;

const TICK_RATE_MS: u64 = 500;

fn main() -> anyhow::Result<()> {
    let tui_config = config::load_or_create()?;

    for warning in theme::validate(&tui_config.custom_theme) {
        eprintln!("{warning}");
    }

    let resolved_theme =
        Theme::from_name(&tui_config.theme).apply_overrides(&tui_config.custom_theme);

    let events = EventHandler::new(TICK_RATE_MS);
    let mut app = App::new(tui_config.server_url, resolved_theme, events.sender());

    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    tui.enter()?;

    while !app.should_quit {
        tui.draw(&mut app)?;

        match tui.events.next()? {
            Event::Tick => app.refresh(),
            Event::Key(key_event) => update(&mut app, key_event),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            Event::App(AppEvent::Refreshed {
                downloads,
                queues,
                aria2_reachable,
            }) => app.apply_refresh(downloads, queues, aria2_reachable),
        }
    }

    tui.exit()?;
    Ok(())
}
