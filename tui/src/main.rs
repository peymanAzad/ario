mod api;
mod app;
mod clipboard;
mod config;
mod daemon;
mod effects;
mod event;
mod icons;
mod keymap;
mod modal;
mod msg;
mod runtime;
mod theme;
mod toast;
mod tui;
mod ui;

use std::sync::Arc;

use app::App;
use daemon::{
    ServerProcess, ServerProcessConfig, finish_server_process, managed_server_target,
    resolve_binary_path,
};
use event::EventHandler;
use ratatui::{Terminal, backend::CrosstermBackend};
use theme::Theme;
use tui::Tui;

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
    let mut app = App::new(resolved_theme, icons::IconSet::new(glyph_mode), managed);

    let mut tui = Tui::new(terminal, events);
    if let Err(error) = tui.enter() {
        finish_server_process(
            server_process.as_ref(),
            &api_base,
            tui_config.on_app_exit.stop_managed_daemon,
        );
        return Err(error);
    }

    let run_result = (|| -> anyhow::Result<()> {
        tui.draw(&mut app)?;
        if let Some(process) = &server_process {
            process.start_supervisor(tui.events.sender());
        }
        runtime::run(&mut tui, &mut app, &api_base)
    })();

    let exit_result = tui.exit();

    finish_server_process(
        server_process.as_ref(),
        &api_base,
        tui_config.on_app_exit.stop_managed_daemon,
    );

    run_result?;
    exit_result
}
