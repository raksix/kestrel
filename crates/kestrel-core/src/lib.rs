//! Kestrel domain core.
//!
//! This crate is deliberately free of any UI, Tauri or platform dependency so
//! that the CLI, the desktop app and the test suite all share one source of
//! truth. Platform work lives in `kestrel-capture` and friends.

pub mod model;
pub mod name_pattern;
pub mod rpc;

pub use model::{
    default_workflows, AfterCaptureTask, AfterUploadTask, CaptureMethod, DestinationKinds,
    ImageFormat, TaskSettings, Workflow,
};
pub use name_pattern::{expand, expand_sanitized, Locale, NameContext};
pub use rpc::{Endpoint, Envelope, Request, Response};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
