use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Error,
    Info,
    Success,
}

pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    created_at: Instant,
}

const TOAST_LIFETIME: Duration = Duration::from_secs(4);

impl Toast {
    fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            message: message.into(),
            level,
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= TOAST_LIFETIME
    }
}

pub struct ToastStack {
    toasts: Vec<Toast>,
}

const MAX_TOASTS: usize = 4;

impl ToastStack {
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    pub fn push(&mut self, message: impl Into<String>, level: ToastLevel) {
        self.toasts.push(Toast::new(message, level));
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    pub fn prune(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }
}
