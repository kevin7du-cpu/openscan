//! OpenScan 0.3: GUI-independent scanning, cleanup, OCR and PDF output.
//! Blocking functions belong on a worker. Images remain owned by the caller.
//! Default backends require no .NET runtime. NAPS2 is an explicit optional feature.
#![forbid(unsafe_code)]
pub mod backend;
pub mod cleanup;
pub mod ocr;
pub mod pdf;
pub mod pipeline;
pub mod session;
pub mod settings;
pub use scan_module::{CancellationToken, session::ScanSession};
