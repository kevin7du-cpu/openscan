//! Reusable scanner acquisition independent of any GUI, OCR engine, or catalog.
//!
//! Native calls are blocking. Use [`spawn_scan`] or your application's worker.
//! The caller owns the output directory and every returned page.
#![doc = include_str!("../README.md")]
use std::{
    fmt, fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

#[cfg(feature = "documents")]
pub mod documents;
pub mod history;
pub mod presets;
pub mod processing;
mod profile;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod sane;
pub mod session;
#[cfg(windows)]
mod wia;
pub use profile::{
    CleanupOptions, DeviceCapabilities, Paper, ScanColor, ScanFileFormat, ScanProfile, ScanSource,
    SourceCapabilities,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    Cancelled,
    Unsupported(String),
    InvalidRequest(String),
    Backend(String),
}
impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Scan cancelled"),
            Self::Unsupported(s) => write!(f, "Unsupported: {s}"),
            Self::InvalidRequest(s) => write!(f, "Invalid scan request: {s}"),
            Self::Backend(s) => write!(f, "Scanner error: {s}"),
        }
    }
}
impl std::error::Error for ScanError {}
impl From<std::io::Error> for ScanError {
    fn from(e: std::io::Error) -> Self {
        Self::Backend(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, ScanError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Gray,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanSettings {
    /// None lets the driver choose. Unsupported explicit settings are rejected.
    pub dpi: Option<u32>,
    pub color: Option<ColorMode>,
    pub source: Option<String>,
    pub duplex: Option<bool>,
    pub paper: Option<Paper>,
    pub max_pages: u32,
    pub native_interface: bool,
}
impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            dpi: None,
            color: None,
            source: None,
            duplex: None,
            paper: None,
            max_pages: 100,
            native_interface: true,
        }
    }
}
impl From<&ScanProfile> for ScanSettings {
    fn from(profile: &ScanProfile) -> Self {
        Self {
            dpi: Some(profile.dpi),
            color: Some(if profile.color == ScanColor::Color {
                ColorMode::Color
            } else {
                ColorMode::Gray
            }),
            source: Some(
                match profile.source {
                    ScanSource::Flatbed => "Flatbed",
                    ScanSource::Feeder => "Feeder",
                    ScanSource::Duplex => "Duplex",
                }
                .into(),
            ),
            duplex: Some(profile.source == ScanSource::Duplex),
            paper: Some(profile.paper),
            max_pages: profile.max_pages,
            native_interface: profile.native_interface,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ScanRequest {
    pub device_id: Option<String>,
    pub output_dir: PathBuf,
    pub settings: ScanSettings,
}
#[derive(Clone, Debug)]
pub struct ScanBatch {
    pub pages: Vec<PathBuf>,
    pub cancelled: bool,
    pub warning: Option<String>,
    /// The driver-accepted resolution, or None for a vendor dialog.
    pub dpi: Option<u32>,
}
#[derive(Clone, Debug)]
pub enum Progress {
    Connecting,
    Acquiring,
    PageReady(PathBuf),
}
#[derive(Clone, Debug)]
pub struct Capabilities {
    pub name: &'static str,
    pub driver_dialog: bool,
    pub programmatic_settings: bool,
    pub cancellation_note: &'static str,
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    /// Share an existing application worker's cancellation flag.
    pub fn from_shared_flag(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(ScanError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub trait ScannerBackend: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn devices(&self) -> Result<Vec<Device>>;
    fn device_capabilities(&self, device_id: &str) -> Result<DeviceCapabilities> {
        Ok(DeviceCapabilities {
            device_id: device_id.into(),
            sources: Vec::new(),
            note: "Use the scanner interface; source discovery is unavailable for this backend"
                .into(),
        })
    }
    fn acquire(
        &self,
        request: &ScanRequest,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch>;
}

pub fn validate_request(request: &ScanRequest) -> Result<()> {
    if !(1..=250).contains(&request.settings.max_pages) {
        return Err(ScanError::InvalidRequest(
            "Maximum pages must be 1–250".into(),
        ));
    }
    if request
        .settings
        .dpi
        .is_some_and(|dpi| !(50..=1200).contains(&dpi))
    {
        return Err(ScanError::InvalidRequest(
            "DPI must be between 50 and 1200".into(),
        ));
    }
    fs::create_dir_all(&request.output_dir)?;
    if fs::read_dir(&request.output_dir)?.next().is_some() {
        return Err(ScanError::InvalidRequest(
            "Use an empty output directory to avoid overwriting existing scans".into(),
        ));
    }
    Ok(())
}

pub fn native_backend() -> Box<dyn ScannerBackend> {
    #[cfg(windows)]
    {
        Box::new(wia::WiaBackend)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        Box::new(sane::SaneBackend)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Box::new(UnsupportedBackend)
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
struct UnsupportedBackend;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
impl ScannerBackend for UnsupportedBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "External scan-to-file",
            driver_dialog: false,
            programmatic_settings: false,
            cancellation_note: "Import images from the platform's scanner application",
        }
    }
    fn devices(&self) -> Result<Vec<Device>> {
        Err(ScanError::Unsupported(
            "No scanner backend is implemented for this platform; import scanned image files"
                .into(),
        ))
    }
    fn acquire(
        &self,
        _: &ScanRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch> {
        Err(ScanError::Unsupported(
            "Use your platform's scanner application, then import the image files".into(),
        ))
    }
}

pub enum ScanEvent {
    Progress(Progress),
    Finished(Result<ScanBatch>),
}
pub struct ScanJob {
    pub events: mpsc::Receiver<ScanEvent>,
    pub cancellation: CancellationToken,
}
pub fn spawn_scan(backend: Box<dyn ScannerBackend>, request: ScanRequest) -> ScanJob {
    let (tx, events) = mpsc::channel();
    let cancellation = CancellationToken::default();
    let token = cancellation.clone();
    thread::spawn(move || {
        let result = backend.acquire(&request, &token, &mut |p| {
            let _ = tx.send(ScanEvent::Progress(p));
        });
        let _ = tx.send(ScanEvent::Finished(result));
    });
    ScanJob {
        events,
        cancellation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock;
    impl ScannerBackend for Mock {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                name: "Mock",
                driver_dialog: false,
                programmatic_settings: true,
                cancellation_note: "Immediate",
            }
        }
        fn devices(&self) -> Result<Vec<Device>> {
            Ok(vec![Device {
                id: "mock-1".into(),
                name: "Synthetic scanner".into(),
            }])
        }
        fn acquire(
            &self,
            r: &ScanRequest,
            c: &CancellationToken,
            p: &mut dyn FnMut(Progress),
        ) -> Result<ScanBatch> {
            c.check()?;
            validate_request(r)?;
            p(Progress::Acquiring);
            let path = r.output_dir.join("page.png");
            fs::write(&path, b"fixture")?;
            p(Progress::PageReady(path.clone()));
            Ok(ScanBatch {
                pages: vec![path],
                cancelled: false,
                warning: None,
                dpi: r.settings.dpi,
            })
        }
    }
    #[test]
    fn cancelled_request_does_not_write() {
        let temp = tempfile::tempdir().unwrap();
        let c = CancellationToken::default();
        c.cancel();
        let r = ScanRequest {
            device_id: None,
            output_dir: temp.path().into(),
            settings: Default::default(),
        };
        assert!(matches!(
            Mock.acquire(&r, &c, &mut |_| {}),
            Err(ScanError::Cancelled)
        ));
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    #[test]
    fn existing_files_are_never_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("existing"), "keep").unwrap();
        let r = ScanRequest {
            device_id: None,
            output_dir: temp.path().into(),
            settings: Default::default(),
        };
        assert!(validate_request(&r).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("existing")).unwrap(),
            "keep"
        );
    }
    #[test]
    fn background_api_emits_page_and_result() {
        let temp = tempfile::tempdir().unwrap();
        let job = spawn_scan(
            Box::new(Mock),
            ScanRequest {
                device_id: None,
                output_dir: temp.path().into(),
                settings: Default::default(),
            },
        );
        let mut page_seen = false;
        loop {
            match job
                .events
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
            {
                ScanEvent::Progress(Progress::PageReady(_)) => page_seen = true,
                ScanEvent::Finished(result) => {
                    assert_eq!(result.unwrap().pages.len(), 1);
                    break;
                }
                _ => (),
            }
        }
        assert!(page_seen);
    }
    #[test]
    fn invalid_resolution_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let r = ScanRequest {
            device_id: None,
            output_dir: temp.path().into(),
            settings: ScanSettings {
                dpi: Some(0),
                ..Default::default()
            },
        };
        assert!(matches!(
            validate_request(&r),
            Err(ScanError::InvalidRequest(_))
        ));
    }
    #[test]
    fn host_and_scanner_share_cancellation() {
        let flag = Arc::new(AtomicBool::new(false));
        let token = CancellationToken::from_shared_flag(flag.clone());
        flag.store(true, Ordering::Relaxed);
        assert!(matches!(token.check(), Err(ScanError::Cancelled)));
        flag.store(false, Ordering::Relaxed);
        token.cancel();
        assert!(flag.load(Ordering::Relaxed));
    }
}
