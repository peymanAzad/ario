use crate::{
    app::{App, Focus, queues::queue_start_message},
    effects::{ApiRequest, Effect},
    msg::{Action, ApiResult, Msg},
    toast::ToastLevel,
};

pub fn update(app: &mut App, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Tick => app.refresh(),
        Msg::Paste(text) => {
            crate::app::keys::paste(app, &text);
            vec![]
        }
        Msg::Action(action) => update_action(app, action),
        Msg::Lifecycle(state) => {
            app.apply_lifecycle(state);
            vec![]
        }
        Msg::Api(result) => update_api(app, result),
        Msg::Toast { message, level } => {
            app.apply_toast(message, level);
            vec![]
        }
    }
}

fn update_action(app: &mut App, action: Action) -> Vec<Effect> {
    match action {
        Action::Quit => {
            app.quit();
            vec![]
        }
        Action::FocusNext => {
            app.focus = app.focus.next();
            vec![]
        }
        Action::FocusPrev => {
            app.focus = app.focus.prev();
            vec![]
        }
        Action::Focus(focus) => {
            app.focus = focus;
            vec![]
        }
        Action::SelectNext => match app.focus {
            Focus::Queues => app.select_next_queue(),
            Focus::Categories => app.select_next_category(),
            Focus::Downloads => {
                app.select_next_download();
                vec![]
            }
        },
        Action::SelectPrev => match app.focus {
            Focus::Queues => app.select_prev_queue(),
            Focus::Categories => app.select_prev_category(),
            Focus::Downloads => {
                app.select_prev_download();
                vec![]
            }
        },
        Action::OpenHelp => {
            app.open_help_modal();
            vec![]
        }
        Action::OpenClipboardImport => {
            app.open_clipboard_import();
            vec![]
        }
        Action::OpenTorrentFile => {
            app.open_torrent_file_modal();
            vec![]
        }
        Action::OpenCreateQueue => {
            app.open_create_queue_modal();
            vec![]
        }
        Action::OpenEditQueue => app.open_edit_queue_modal(),
        Action::PauseDownload => app.pause_selected(),
        Action::ResumeDownload => app.resume_selected(),
        Action::DeleteDownload => app.delete_selected(),
        Action::DeleteDownloadFiles => {
            app.request_delete_selected_files();
            vec![]
        }
        Action::OpenDownloadFolder => app.open_selected_download_folder(),
        Action::ActivateDownload => app.activate_selected_download(),
        Action::PauseQueue => app.pause_selected_queue(),
        Action::ResumeQueue => app.resume_selected_queue(),
        Action::DeleteQueue => app.request_delete_selected_queue(),
        Action::RemoveCompleted => app.remove_completed_downloads(),
        Action::CloseModal | Action::CancelModal => {
            close_open_modal(app);
            vec![]
        }
        Action::SubmitDownloads(request) => {
            let mut effects = vec![Effect::Api(ApiRequest::AddDownloads(request))];
            effects.extend(app.refresh());
            effects
        }
        Action::SubmitTorrent { path, metadata } => {
            vec![Effect::Api(ApiRequest::AddTorrent { path, metadata })]
        }
        Action::SaveQueue {
            mode,
            create,
            update,
            ordered_ids,
        } => match mode {
            crate::app::queue_modal::QueueModalMode::Create => create
                .map(|request| vec![Effect::Api(ApiRequest::CreateQueue(request))])
                .unwrap_or_default(),
            crate::app::queue_modal::QueueModalMode::Edit { .. } => update
                .map(|(id, request)| {
                    vec![Effect::Api(ApiRequest::UpdateQueue {
                        id,
                        request,
                        ordered_ids,
                    })]
                })
                .unwrap_or_default(),
        },
        Action::SaveDownloadEdit {
            id,
            finetune,
            queue_id,
            original_queue_id,
        } => {
            let mut effects = Vec::new();
            if queue_id != original_queue_id {
                effects.push(Effect::Api(ApiRequest::MoveDownloadQueue { id, queue_id }));
            }
            effects.push(Effect::Api(ApiRequest::UpdateFinetune { id, finetune }));
            effects.extend(app.refresh());
            effects
        }
        Action::Confirm(pending) => app.execute_confirmation(pending),
        Action::ModalHandled => vec![],
    }
}

fn close_open_modal(app: &mut App) {
    use crate::modal::Modal;
    if matches!(app.modal, Some(Modal::Confirmation { .. })) {
        app.cancel_confirmation();
    } else if matches!(app.modal, Some(Modal::Help(_))) {
        app.modal = None;
    } else if matches!(app.modal, Some(Modal::Queue(_))) {
        app.cancel_queue_modal();
    } else if matches!(app.modal, Some(Modal::TorrentFile(_))) {
        app.cancel_torrent_file_modal();
    } else if matches!(app.modal, Some(Modal::ClipboardImport(_))) {
        app.cancel_modal();
    } else if matches!(app.modal, Some(Modal::DownloadEdit(_))) {
        app.cancel_download_modal();
    }
}

fn update_api(app: &mut App, result: ApiResult) -> Vec<Effect> {
    match result {
        ApiResult::Refreshed {
            downloads,
            queues,
            server_reachable,
            aria2_reachable,
            download_speed,
            active_downloads,
            aria2_global_options,
            lifecycle_revision,
        } => {
            app.apply_refresh(
                downloads,
                queues,
                server_reachable,
                aria2_reachable,
                download_speed,
                active_downloads,
                aria2_global_options,
                lifecycle_revision,
            );
            vec![]
        }
        ApiResult::QueueDownloadsLoaded { queue_id, result } => {
            app.apply_queue_downloads_loaded(queue_id, result);
            vec![]
        }
        ApiResult::QueueSaved(result) => app.apply_queue_saved(result),
        ApiResult::DownloadPaused {
            download_id,
            result,
        } => {
            app.apply_download_paused(download_id, result);
            vec![]
        }
        ApiResult::DownloadFilesDeleted(result) => app.apply_download_files_deleted(result),
        ApiResult::TorrentAdded(result) => app.apply_torrent_added(result),
        ApiResult::QueueDeleteResolved {
            queue_id,
            queue_name,
            result,
        } => app.apply_queue_delete_result(queue_id, queue_name, result),
        ApiResult::QueueResumed {
            queue_id: _,
            result,
        } => match result {
            Ok(stop) => {
                app.apply_toast(queue_start_message(stop), ToastLevel::Success);
                vec![]
            }
            Err(error) => {
                app.apply_toast(error.to_string(), ToastLevel::Error);
                vec![]
            }
        },
        ApiResult::Failed { error } => {
            app.apply_toast(error, ToastLevel::Error);
            vec![]
        }
        ApiResult::Done => app.refresh(),
    }
}
