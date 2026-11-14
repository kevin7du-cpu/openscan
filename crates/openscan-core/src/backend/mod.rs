// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scanner back ends.
//!
//! Each back end knows one way of reaching scanners. The rest of the program only uses the
//! [`ScannerBackend`] trait, so adding another (a direct TWAIN or eSCL implementation, say)
//! means adding one file and one line in [`all_backends`].

mod file_import;
#[cfg(feature = "naps2")]
mod naps2;
pub mod native;
mod sane;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use image::DynamicImage;

use crate::settings::ScanSettings;

pub use file_import::FILE_IMPORT_BACKEND;

/// A scanner, or scanner-like source, the user can pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Name of the back end that found it.
    pub backend: &'static str,
    /// Identifier the back end uses to open it.
    pub id: String,
    /// Name shown in the Scanner list.
    pub name: String,
}

impl Device {
    /// Stable text identifying this device in saved settings.
    pub fn key(&self) -> String {
        format!("{}|{}", self.backend, self.id)
    }
}

/// Everything a back end needs for one scan.
pub struct ScanRequest<'a> {
    pub device: &'a Device,
    pub settings: &'a ScanSettings,
    /// Image files to read, used only by the file import back end.
    pub input_files: &'a [PathBuf],
    /// Empty folder the back end may write temporary files to.
    pub work_dir: &'a Path,
}

/// One way of finding and driving scanners.
pub trait ScannerBackend: Send + Sync {
    /// Short fixed name, also stored in [`Device::backend`].
    fn name(&self) -> &'static str;

    /// Lists the devices this back end can reach. An empty list is not an error.
    fn list_devices(&self) -> Result<Vec<Device>>;

    /// Scans and returns the pages in order. `log` receives progress messages.
    fn scan(&self, request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>>;
}

/// Every back end built into the program.
pub fn all_backends() -> Vec<Box<dyn ScannerBackend>> {
    vec![
        #[cfg(feature = "naps2")]
        Box::new(naps2::Naps2Backend),
        #[cfg(windows)]
        Box::new(native::NativeBackend(scan_module::native_backend())),
        Box::new(sane::SaneBackend),
        Box::new(file_import::FileImportBackend),
    ]
}

/// Asks every back end for its devices. Problems are returned as messages, not errors,
/// so one broken driver does not hide the scanners that work.
pub fn discover() -> (Vec<Device>, Vec<String>) {
    let mut devices = Vec::new();
    let mut problems = Vec::new();
    for backend in all_backends() {
        match backend.list_devices() {
            Ok(found) => devices.extend(found),
            Err(error) => problems.push(format!("{}: {error:#}", backend.name())),
        }
    }
    (devices, problems)
}

/// Runs a scan on the back end that owns `request.device`.
pub fn scan(request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>> {
    let backend = all_backends()
        .into_iter()
        .find(|b| b.name() == request.device.backend)
        .with_context(|| format!("no back end named {}", request.device.backend))?;
    backend.scan(request, log)
}

/// Finds a program by trying each candidate, either as a full path or on the `PATH`.
fn find_program(candidates: &[&str]) -> Option<PathBuf> {
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.is_absolute() {
            if path.exists() {
                return Some(path.to_path_buf());
            }
            continue;
        }
        let Some(search) = std::env::var_os("PATH") else {
            continue;
        };
        for dir in std::env::split_paths(&search) {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// Starts a command for a helper program such as a scanner tool or Tesseract.
///
/// On Windows this also stops a black console window flashing up each time one runs.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Windows flag: do not create a console window for the new process.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Runs a command and returns its standard output, failing with the error text otherwise.
fn run(command: &mut Command) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("starting {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{:?} failed ({}): {}",
            command.get_program(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Loads every image in `dir`, ordered by file name.
fn load_images_sorted(dir: &Path) -> Result<Vec<DynamicImage>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| image::open(path).with_context(|| format!("opening {}", path.display())))
        .collect()
}
