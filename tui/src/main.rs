mod api;
mod app;
mod clipboard;
mod config;
mod event;
mod server_process;
mod theme;
mod toast;
mod tui;
mod ui;
mod update;

use std::sync::Arc;
use std::thread;

use app::App;
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use server_process::{ServerProcess, ServerProcessConfig, port_from_url, resolve_binary_path, should_manage};
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

    let managed = should_manage(tui_config.auto_start_server, &tui_config.server_url);
    let server_process = if managed {
        let log_path = config::config_dir()?.join("server.log");
        let process = Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: resolve_binary_path(&tui_config.server_binary),
            api_base: tui_config.server_url.clone(),
            port: port_from_url(&tui_config.server_url),
            log_path,
        }));
        if let Err(e) = process.ensure_started() {
            eprintln!("failed to start ario_daemon: {e}");
        }
        let supervise = Arc::clone(&process);
        thread::spawn(move || supervise.supervise());
        Some(process)
    } else {
        None
    };

    let events = EventHandler::new(TICK_RATE_MS);
    let mut app = App::new(
        tui_config.server_url,
        resolved_theme,
        events.sender(),
        managed,
        server_process.clone(),
    );

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
                server_reachable,
                aria2_reachable,
            }) => app.apply_refresh(downloads, queues, server_reachable, aria2_reachable),
            Event::App(AppEvent::QueueDownloadsLoaded(result)) => {
                app.apply_queue_downloads_loaded(result)
            }
            Event::App(AppEvent::ActionFailed(msg)) => app.apply_action_failed(msg),
        }
    }

    tui.exit()?;

    if let Some(process) = server_process {
        process.shutdown();
    }

    Ok(())
}
