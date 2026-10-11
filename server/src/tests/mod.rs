//! Workflow integration tests grouped by subject; only aria2 is mocked.
mod client;
mod download;
mod lifecycle;
mod queue;
mod scheduler;
mod sources;
mod support;
mod validation;

#[allow(dead_code)]
#[path = "../../../tui/src/api.rs"]
mod tui_api;
