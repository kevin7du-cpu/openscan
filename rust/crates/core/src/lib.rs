//! Reusable scanner engine. No UI, .NET, server, or automatic scan at startup.
#![forbid(unsafe_code)]

pub mod backend;
pub mod correction;
mod import;
pub mod model;
pub mod ocr;
pub mod pdf;
pub mod storage;

pub use model::*;
pub use storage::{PresetStore, Session};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn error(message: impl Into<String>) -> Error {
    Error::Message(message.into())
}

/// A host owns this flag. Cancellation is checked during acquisition and correction.
#[derive(Clone, Default)]
pub struct Cancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled() {
            Err(error(
                "Scan cancelled. Received pages remain in the session.",
            ))
        } else {
            Ok(())
        }
    }
}
