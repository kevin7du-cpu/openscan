// SPDX-License-Identifier: MIT OR Apache-2.0
//! A stand-in scanner that reads image files. It is always available, which makes the whole
//! pipeline usable and testable on a computer with no scanner attached.

use anyhow::{Context, Result, bail};
use image::DynamicImage;

use super::{Device, ScanRequest, ScannerBackend};

/// Name of this back end, used by the interface to know it must ask for files.
pub const FILE_IMPORT_BACKEND: &str = "files";

pub struct FileImportBackend;

impl ScannerBackend for FileImportBackend {
    fn name(&self) -> &'static str {
        FILE_IMPORT_BACKEND
    }

    fn list_devices(&self) -> Result<Vec<Device>> {
        Ok(vec![Device {
            backend: FILE_IMPORT_BACKEND,
            id: "import".to_owned(),
            name: "Image files (import)".to_owned(),
        }])
    }

    fn scan(&self, request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>> {
        if request.input_files.is_empty() {
            bail!("no image files were chosen");
        }
        request
            .input_files
            .iter()
            .map(|path| {
                log(format!("Reading {}", path.display()));
                image::open(path).with_context(|| format!("opening {}", path.display()))
            })
            .collect()
    }
}
