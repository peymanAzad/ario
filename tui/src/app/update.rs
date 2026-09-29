use crate::{
    app::{App, Focus, queues::queue_start_message},
    effects::{ApiRequest, Effect},
    modal::{ClipboardImportModal, HelpModal, Modal, QueueModal, TorrentFileModal},
    msg::{Action, ApiResult, Msg, QueueSaveRequest},
    toast::ToastLevel,
};

pub fn update(app: &mut App, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Tick => app.refresh(),
        Msg::Paste(text) => {
            crate::keymap::paste(app, &text);
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
            if app.modal.is_none() {
                app.modal = Some(Modal::Help(HelpModal::default()));
            }
            vec![]
        }
        Action::OpenClipboardImport => open_clipboard_import(app),
        Action::OpenTorrentFile => {
            if app.modal.is_none() {
                app.modal = Some(Modal::TorrentFile(TorrentFileModal::open(
                    app.selected_queue,
                    &app.queues,
                )));
            }
            vec![]
        }
        Action::OpenCreateQueue => {
            if app.modal.is_none() {
                app.modal = Some(Modal::Queue(QueueModal::create()));
            }
            vec![]
        }
        Action::OpenEditQueue => open_edit_queue(app),
        Action::CloseModal => {
            app.modal = None;
            vec![]
        }
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
        Action::SubmitDownloads(request) => {
            let mut effects = vec![Effect::Api(ApiRequest::AddDownloads(request))];
            effects.extend(app.refresh());
            effects
        }
        Action::SubmitTorrent { path, metadata } => {
            app.apply_toast("adding torrent…".into(), ToastLevel::Info);
            vec![Effect::Api(ApiRequest::AddTorrent { path, metadata })]
        }
        Action::SaveQueue {
            mode,
            request,
            ordered_ids,
        } => match (mode, request) {
            (crate::modal::QueueModalMode::Create, QueueSaveRequest::Create(request)) => {
                vec![Effect::Api(ApiRequest::CreateQueue(request))]
            }
            (
                crate::modal::QueueModalMode::Edit { .. },
                QueueSaveRequest::Update { id, request },
            ) => vec![Effect::Api(ApiRequest::UpdateQueue {
                id,
                request,
                ordered_ids,
            })],
            _ => vec![],
        },
        Action::SaveDownloadEdit {
            id,
            finetune,
            queue_id,
        } => {
            let mut effects = Vec::new();
            let current_queue_id = app
                .downloads
                .iter()
                .find(|download| download.download.id == id)
                .map(|download| download.download.queue_id);
            if current_queue_id != Some(queue_id) {
                effects.push(Effect::Api(ApiRequest::MoveDownloadQueue { id, queue_id }));
            }
            effects.push(Effect::Api(ApiRequest::UpdateFinetune { id, finetune }));
            effects.extend(app.refresh());
            effects
        }
        Action::Confirm(pending) => match pending {
            crate::app::PendingConfirmationAction::DeleteDownloadFiles { download_id } => {
                app.delete_download_files(download_id)
            }
            crate::app::PendingConfirmationAction::DeleteQueue { queue_id } => {
                app.confirm_delete_queue(queue_id)
            }
        },
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
            if let Some(Modal::Queue(modal)) = &mut app.modal {
                modal.apply_loaded_downloads(queue_id, result);
            }
            vec![]
        }
        ApiResult::QueueSaved(result) => {
            if let Err(error) = result {
                app.apply_toast(error.to_string(), ToastLevel::Error);
            }
            app.refresh()
        }
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
        ApiResult::Failed { context, error } => {
            app.apply_toast(format!("{context}: {error}"), ToastLevel::Error);
            vec![]
        }
        ApiResult::Done => app.refresh(),
    }
}

fn open_clipboard_import(app: &mut App) -> Vec<Effect> {
    if app.modal.is_some() {
        return vec![];
    }
    let urls = crate::clipboard::scan_clipboard_for_urls();
    if urls.is_empty() {
        app.apply_toast("clipboard is empty".into(), ToastLevel::Info);
        return vec![];
    }
    app.modal = Some(Modal::ClipboardImport(ClipboardImportModal::from_urls(
        urls,
        app.selected_queue,
        &app.queues,
    )));
    vec![]
}

fn open_edit_queue(app: &mut App) -> Vec<Effect> {
    if app.modal.is_some() || app.selected_queue == 0 {
        return vec![];
    }
    let Some(queue) = app.queues.get(app.selected_queue - 1).cloned() else {
        return vec![];
    };
    let queue_id = queue.id;
    app.modal = Some(Modal::Queue(QueueModal::edit(&queue)));
    vec![Effect::Api(ApiRequest::ListQueueDownloads { queue_id })]
}
