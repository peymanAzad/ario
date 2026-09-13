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

use app::App;
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use server_process::{
    ServerProcess, ServerProcessConfig, managed_server_target, resolve_binary_path,
};
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

    let managed_target =
        managed_server_target(tui_config.auto_start_server, &tui_config.server_url);
    let managed = managed_target.is_some();
    let api_base = managed_target
        .map(|target| target.api_base())
        .unwrap_or_else(|| tui_config.server_url.clone());

    let events = EventHandler::new(TICK_RATE_MS);
    let server_process = if let Some(target) = managed_target {
        let log_path = config::config_dir()?.join("server.log");
        let process = Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: resolve_binary_path(&tui_config.server_binary),
            api_base: api_base.clone(),
            target,
            log_path,
        }));
        match process.ensure_started() {
            Ok(()) if process.owns_process() => {
                eprintln!("ario_daemon started (managed)");
            }
            Ok(()) => {
                eprintln!("ario_daemon already running");
            }
            Err(e) => {
                eprintln!("failed to start ario_daemon: {e}");
            }
        }
        process.start_supervisor(events.sender());
        Some(process)
    } else {
        None
    };

    let mut app = App::new(api_base, resolved_theme, events.sender(), managed);

    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let mut tui = Tui::new(terminal, events);
    if let Err(error) = tui.enter() {
        finish_server_process(server_process.as_ref(), &app.api_base);
        return Err(error);
    }

    let run_result = (|| -> anyhow::Result<()> {
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
                Event::App(AppEvent::Toast { message, level }) => app.apply_toast(message, level),
            }
        }
        Ok(())
    })();

    let exit_result = tui.exit();

    finish_server_process(server_process.as_ref(), &app.api_base);

    run_result?;
    exit_result
}

fn finish_server_process(process: Option<&Arc<ServerProcess>>, api_base: &str) {
    let Some(process) = process else { return };

    // Prevent a graceful daemon exit from racing with the respawner.
    process.stop_supervisor();
    match api::health(api_base) {
        Ok(health) if health.tui_managed => match api::shutdown_if_idle(api_base) {
            Ok(result) => match result.outcome {
                common::lifecycle::ShutdownOutcome::ShuttingDown => process.wait_for_shutdown(),
                common::lifecycle::ShutdownOutcome::KeptRunning => {
                    eprintln!(
                        "ario_daemon kept running: {} active download(s), {} scheduled queue(s)",
                        result.active_downloads, result.scheduled_queues
                    );
                    process.detach();
                }
                common::lifecycle::ShutdownOutcome::NotManaged => process.detach(),
            },
            Err(e) => {
                eprintln!("ario_daemon kept running: shutdown check failed: {e}");
                process.detach();
            }
        },
        Ok(_) => process.detach(),
        Err(e) => {
            eprintln!("ario_daemon kept running: health check failed: {e}");
            process.detach();
        }
    }
}
