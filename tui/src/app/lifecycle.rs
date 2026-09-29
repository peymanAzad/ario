#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    Starting,
    Retrying,
    Connected,
    Failed(String),
}

/// A failed refresh started before a newer lifecycle event must not undo it.
pub(crate) fn is_stale_unreachable_refresh(
    manages_server: bool,
    server_reachable: bool,
    request_revision: u64,
    current_revision: u64,
) -> bool {
    manages_server && !server_reachable && request_revision != current_revision
}

pub(crate) fn clears_last_error(state: &LifecycleState) -> bool {
    matches!(
        state,
        LifecycleState::Starting | LifecycleState::Retrying | LifecycleState::Connected
    )
}

pub(crate) fn marks_server_reachable(state: &LifecycleState) -> bool {
    matches!(state, LifecycleState::Connected)
}
