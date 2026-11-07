//! Windows' scanner API, through Microsoft's safe windows-rs projection.
//! No COM pointers or unsafe code enter the scanner engine or its host.
use super::*;
use ::windows::{
    Devices::{Enumeration::DeviceInformation, Scanners::*},
    Foundation::Rect,
    Storage::StorageFolder,
    core::{HSTRING, Interface},
};
use std::time::{Duration, Instant};
use windows_future::AsyncOperationWithProgressCompletedHandler;
pub struct Windows;
impl From<::windows::core::Error> for Error {
    fn from(e: ::windows::core::Error) -> Self {
        error(format!("Windows scanner: {e}"))
    }
}

fn open(id: &str) -> Result<ImageScanner> {
    let id = id
        .strip_prefix("wia:")
        .ok_or_else(|| error("Invalid Windows scanner identifier."))?;
    Ok(ImageScanner::FromIdAsync(&HSTRING::from(id))?.join()?)
}
fn configuration(
    scanner: &ImageScanner,
    source: Source,
) -> Result<IImageScannerSourceConfiguration> {
    Ok(match source {
        Source::Flatbed => scanner.FlatbedConfiguration()?.cast()?,
        Source::Feeder | Source::Duplex => scanner.FeederConfiguration()?.cast()?,
    })
}
fn transfer_format(
    mut supported: impl FnMut(ImageScannerFormat) -> Result<bool>,
) -> Result<ImageScannerFormat> {
    for candidate in [
        ImageScannerFormat::Png,
        ImageScannerFormat::DeviceIndependentBitmap,
        ImageScannerFormat::Jpeg,
    ] {
        if supported(candidate)? {
            return Ok(candidate);
        }
    }
    Err(error(
        "The scanner driver supports none of PNG, BMP or JPEG transfer.",
    ))
}
impl Backend for Windows {
    fn devices(&self, cancel: &Cancellation) -> Result<Vec<Device>> {
        cancel.check()?;
        let all = DeviceInformation::FindAllAsyncAqsFilter(&ImageScanner::GetDeviceSelector()?)?
            .join()?;
        let mut out = Vec::new();
        for i in 0..all.Size()?.min(256) {
            cancel.check()?;
            let d = all.GetAt(i)?;
            out.push(Device {
                id: format!("wia:{}", d.Id()?),
                name: d.Name()?.to_string(),
                driver: "WIA".into(),
            });
        }
        Ok(out)
    }
    fn capabilities(&self, id: &str) -> Result<Capabilities> {
        let scanner = open(id)?;
        let mut sources = Vec::new();
        for (source, native) in [
            (Source::Flatbed, ImageScannerScanSource::Flatbed),
            (Source::Feeder, ImageScannerScanSource::Feeder),
            (Source::Duplex, ImageScannerScanSource::Feeder),
        ] {
            if !scanner.IsScanSourceSupported(native)? {
                continue;
            }
            if source == Source::Duplex && !scanner.FeederConfiguration()?.CanScanDuplex()? {
                continue;
            }
            let c = configuration(&scanner, source)?;
            let min = c.MinResolution()?.DpiX.max(75.).ceil() as u32;
            let max = c.MaxResolution()?.DpiX.min(600.).floor() as u32;
            sources.push(SourceCaps {
                source,
                dpi: [75, 100, 150, 200, 300, 400, 600]
                    .into_iter()
                    .filter(|d| *d >= min && *d <= max)
                    .collect(),
                min_dpi: min,
                max_dpi: max,
            });
        }
        Ok(Capabilities {
            device_id: id.into(),
            sources,
            note: "Settings are reported by the installed Windows scanner driver.".into(),
        })
    }
    fn acquire(
        &self,
        p: &Profile,
        directory: &Path,
        cancel: &Cancellation,
        on_page: &mut dyn FnMut(RawPage) -> Result<()>,
    ) -> Result<()> {
        p.validate()?;
        cancel.check()?;
        if p.native_interface {
            return Err(error(
                "Use the OpenScan settings; the native driver dialog is not exposed by Windows' scanner API.",
            ));
        }
        let scanner = open(
            p.device_id
                .as_deref()
                .ok_or_else(|| error("Select a scanner."))?,
        )?;
        let native = if p.source == Source::Flatbed {
            ImageScannerScanSource::Flatbed
        } else {
            ImageScannerScanSource::Feeder
        };
        if !scanner.IsScanSourceSupported(native)? {
            return Err(error("The scanner does not support this source."));
        }
        let c = configuration(&scanner, p.source)?;
        c.SetDesiredResolution(ImageScannerResolution {
            DpiX: p.dpi as f32,
            DpiY: p.dpi as f32,
        })?;
        // Acquire grayscale for software black/white so the adaptive threshold has tonal data.
        c.SetColorMode(if p.color == Color::Color {
            ImageScannerColorMode::Color
        } else {
            ImageScannerColorMode::Grayscale
        })?;
        c.SetFormat(transfer_format(|format| Ok(c.IsFormatSupported(format)?))?)?;
        let (width, height) = p.paper.millimeters();
        let max = c.MaxScanArea()?;
        if width / 25.4 > max.Width + 0.02 || height / 25.4 > max.Height + 0.02 {
            return Err(error(
                "The selected paper size exceeds this source's scan area. Choose a supported paper size.",
            ));
        }
        c.SetSelectedScanRegion(Rect {
            X: 0.,
            Y: 0.,
            Width: (width / 25.4).min(max.Width),
            Height: (height / 25.4).min(max.Height),
        })?;
        if p.source != Source::Flatbed {
            let feeder = scanner.FeederConfiguration()?;
            if p.source == Source::Duplex && !feeder.CanScanDuplex()? {
                return Err(error("This scanner does not support duplex scanning."));
            }
            if feeder.CanScanDuplex()? {
                feeder.SetDuplex(p.source == Source::Duplex)?;
            }
            feeder.SetMaxNumberOfPages(p.max_pages)?;
        }
        std::fs::create_dir_all(directory)?;
        let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(
            directory
                .canonicalize()?
                .to_string_lossy()
                .trim_start_matches("\\\\?\\"),
        ))?
        .join()
        .map_err(|e| error(format!("Cannot open the Windows scan folder: {e}")))?;
        let operation = scanner
            .ScanFilesToFolderAsync(native, &folder)
            .map_err(|e| error(format!("Windows could not start the scan: {e}")))?;
        // Register completion before inspecting Status. Some WIA drivers start the
        // WinRT operation only after a completion delegate has been installed.
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        operation
            .SetCompleted(&AsyncOperationWithProgressCompletedHandler::new(
                move |_, status| {
                    let _ = send.try_send(status);
                    Ok(())
                },
            ))
            .map_err(|e| error(format!("Cannot monitor the Windows scan: {e}")))?;
        let start = Instant::now();
        let acquired = loop {
            match receive.recv_timeout(Duration::from_millis(50)) {
                Ok(_) => break operation.GetResults().map_err(Error::from),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(error("The Windows scan completion channel closed."));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            if cancel.cancelled() || start.elapsed() > Duration::from_secs(900) {
                let _ = operation.Cancel();
                break Err(error(
                    "Scan cancelled or timed out. The driver may take a few seconds to stop; received images remain in the session.",
                ));
            }
        };
        let actual = c.ActualResolution()?.DpiX.round() as u32;
        let dpi = if (75..=600).contains(&actual) {
            actual
        } else {
            p.dpi
        };
        // Windows may leave completed pages even when a feeder jams or the user cancels.
        // Deliver these before returning the failure so the host can recover them.
        let mut paths: Vec<_> = std::fs::read_dir(directory)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|s| {
                    ["png", "bmp", "jpg", "jpeg"]
                        .iter()
                        .any(|ext| s.eq_ignore_ascii_case(ext))
                })
            })
            .collect();
        if let Ok(result) = &acquired {
            let files = result.ScannedFiles()?;
            paths.clear();
            let root = directory.canonicalize()?;
            for i in 0..files.Size()?.min(p.max_pages) {
                let path = std::path::PathBuf::from(files.GetAt(i)?.Path()?.to_string());
                if !path.canonicalize()?.starts_with(&root) {
                    return Err(error("Windows returned a scan outside the session folder."));
                }
                paths.push(path);
            }
        } else {
            paths.sort();
        }
        let received = paths.len();
        for path in paths.into_iter().take(p.max_pages as usize) {
            on_page(RawPage { path, dpi })?;
        }
        acquired?;
        cancel.check()?;
        if received == 0 {
            return Err(error(
                "No pages received. Load paper in the feeder and check the selected source.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bmp_only_drivers_do_not_require_png_transfer() {
        assert_eq!(
            transfer_format(|format| Ok(format == ImageScannerFormat::DeviceIndependentBitmap))
                .expect("BMP"),
            ImageScannerFormat::DeviceIndependentBitmap
        );
        assert!(transfer_format(|_| Ok(false)).is_err());
    }
}
