//! Shutdown / detach policy when the TUI exits while supervising a daemon.

use std::sync::Arc;

use crate::api;
use crate::config::{ManagedExitAction, StopManagedDaemon};

use super::ServerProcess;

pub fn finish_server_process(
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
    match crate::config::managed_exit_action(policy, tui_managed, owns_child) {
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
