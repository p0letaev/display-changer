use serde::Serialize;

use crate::model::Mode;

pub type Result<T> = std::result::Result<T, Error>;

/// Serialized as `{ kind, detail }` so the frontend can branch on `kind`.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum Error {
    #[error("invalid layout: {}", format_issues(.0))]
    InvalidLayout(Vec<LayoutIssue>),
    #[error("not supported by this backend: {0}")]
    Unsupported(String),
    #[error("backend error: {0}")]
    Backend(String),
}

/// A single reason why a layout cannot be applied.
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutIssue {
    #[error("no enabled outputs")]
    NoEnabledOutputs,
    #[error("expected exactly one primary output, found {count}")]
    PrimaryCount { count: usize },
    #[error("{connector}: monitor is not connected")]
    UnknownMonitor { connector: String },
    #[error("{connector}: duplicate output entry")]
    DuplicateOutput { connector: String },
    #[error("{connector}: mode {}x{}@{} mHz is not supported", .mode.width, .mode.height, .mode.refresh_mhz)]
    UnsupportedMode { connector: String, mode: Mode },
    #[error("{connector}: scale {scale} is not allowed")]
    InvalidScale { connector: String, scale: f64 },
    #[error("{connector}: rotation is not supported")]
    RotationUnsupported { connector: String },
    #[error("{connector}: cannot be disabled")]
    DisableUnsupported { connector: String },
    #[error("{a} overlaps {b}")]
    Overlap { a: String, b: String },
    #[error("{connector}: not adjacent to the rest of the layout")]
    Detached { connector: String },
}

fn format_issues(issues: &[LayoutIssue]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
