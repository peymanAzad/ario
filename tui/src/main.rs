mod api;
mod app;
mod clipboard;
mod config;
mod event;
mod icons;
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
use crate::config::{ManagedExitAction, StopManagedDaemon};

const TICK_RATE_MS: u64 = 500;

fn main() -> anyhow::Result<()> {
    let tui_config = config::load_or_create()?;
    let cli_glyph_mode = icons::parse_glyph_args(std::env::args().skip(1))?;
    let lc_all = std::env::var("LC_ALL").ok();
    let lc_ctype = std::env::var("LC_CTYPE").ok();
    let lang = std::env::var("LANG").ok();
    let (glyph_mode, glyph_warning) = config::resolve_glyph_mode(
        cli_glyph_mode,
        tui_config.glyph_mode.as_deref(),
        lc_all.as_deref(),
        lc_ctype.as_deref(),
        lang.as_deref(),
    );
    if let Some(warning) = glyph_warning {
        eprintln!("{warning}");
    }

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

    let server_process = if let Some(target) = managed_target {
        let log_path = config::config_dir()?.join("server.log");
        let process = Arc::new(ServerProcess::new(ServerProcessConfig {
            binary_path: resolve_binary_path(&tui_config.server_binary),
            api_base: api_base.clone(),
            target,
            log_path,
        }));
        Some(process)
    } else {
        None
    };

    let backend = CrosstermBackend::new(std::io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new(TICK_RATE_MS);
    let mut app = App::new(
        api_base,
        resolved_theme,
        icons::IconSet::new(glyph_mode),
        events.sender(),
        managed,
    );

    let mut tui = Tui::new(terminal, events);
    if let Err(error) = tui.enter() {
        finish_server_process(
            server_process.as_ref(),
            &app.api_base,
            tui_config.on_app_exit.stop_managed_daemon,
        );
        return Err(error);
    }

    let run_result = (|| -> anyhow::Result<()> {
        tui.draw(&mut app)?;
        if let Some(process) = &server_process {
            process.start_supervisor(tui.events.sender());
        }
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
                    download_speed,
                    aria2_global_options,
                    lifecycle_revision,
                }) => app.apply_refresh(
                    downloads,
                    queues,
                    server_reachable,
                    aria2_reachable,
                    download_speed,
                    aria2_global_options,
                    lifecycle_revision,
                ),
                Event::App(AppEvent::Lifecycle(state)) => app.apply_lifecycle(state),
                Event::App(AppEvent::QueueDownloadsLoaded { queue_id, result }) => {
                    app.apply_queue_downloads_loaded(queue_id, result)
                }
                Event::App(AppEvent::QueueSaved(result)) => app.apply_queue_saved(result),
                Event::App(AppEvent::Toast { message, level }) => app.apply_toast(message, level),
                Event::App(AppEvent::DownloadFilesDeleted(result)) => {
                    app.apply_download_files_deleted(result)
                }
                Event::App(AppEvent::QueueDeleteResolved {
                    queue_id,
                    queue_name,
                    result,
                }) => app.apply_queue_delete_result(queue_id, queue_name, result),
            }
        }
        Ok(())
    })();

    let exit_result = tui.exit();

    finish_server_process(
        server_process.as_ref(),
        &app.api_base,
        tui_config.on_app_exit.stop_managed_daemon,
    );

    run_result?;
    exit_result
}

fn finish_server_process(
    process: Option<&Arc<ServerProcess>>,
    api_base: &str,
    policy: StopManagedDaemon,
) {
    let Some(process) = process else { return };

    // Prevent a graceful daemon exit from racing with the respawner.
    process.stop_supervisor();
    if policy == StopManagedDaemon::Never {
        process.detach();
        return;
    }

    let health = api::health(api_base).ok();
    let tui_managed = health.as_ref().map(|h| h.tui_managed);
    let owns_child = process.owns_process() && process.has_child();
    match config::managed_exit_action(policy, tui_managed, owns_child) {
        ManagedExitAction::Detach => {
            if health.is_none() {
                eprintln!("ario_daemon kept running: health check failed");
            }
            process.detach();
        }
        ManagedExitAction::TerminateOwned => process.terminate_owned(),
        ManagedExitAction::ShutdownIfIdle => {
            apply_shutdown_result(process, api::shutdown_if_idle(api_base), false)
        }
        ManagedExitAction::ForceShutdown => {
            apply_shutdown_result(process, api::shutdown(api_base), true)
        }
    }
}

fn apply_shutdown_result(
    process: &ServerProcess,
    result: anyhow::Result<common::lifecycle::ShutdownIfIdleResponse>,
    force: bool,
) {
    match result {
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
        Err(e) if force && process.owns_process() && process.has_child() => {
            eprintln!("ario_daemon shutdown request failed: {e}; terminating owned process");
            process.terminate_owned();
        }
        Err(e) => {
            eprintln!("ario_daemon kept running: shutdown check failed: {e}");
            process.detach();
        }
    }
}
