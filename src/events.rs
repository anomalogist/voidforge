use std::path::PathBuf;

use crate::models::{GitSnapshot, SystemMetrics};

#[derive(Clone, Debug)]
pub enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    BuildLog {
        build_id: i64,
        line: String,
    },
    BuildFinished {
        build_id: i64,
        status: String,
        code: Option<i32>,
        duration_seconds: i64,
        log_path: PathBuf,
    },
    Metrics(SystemMetrics),
    Notice {
        level: NoticeLevel,
        message: String,
    },
    GitSnapshot(GitSnapshot),
}
