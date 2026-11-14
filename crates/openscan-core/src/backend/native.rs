//! Adapter for the existing direct Rust WIA backend and injected host backends.
use super::*;
use crate::settings::{ColorMode, PaperSize, Sides, Source};
pub struct NativeBackend(pub Box<dyn scan_module::ScannerBackend>);
impl ScannerBackend for NativeBackend {
    fn name(&self) -> &'static str {
        "Native"
    }
    fn list_devices(&self) -> Result<Vec<Device>> {
        Ok(self
            .0
            .devices()?
            .into_iter()
            .map(|d| Device {
                backend: "Native",
                id: d.id,
                name: d.name,
            })
            .collect())
    }
    fn scan(&self, request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>> {
        let s = request.settings;
        let paper = match s.paper_size {
            PaperSize::Auto => None,
            PaperSize::Letter => Some(scan_module::Paper::Letter),
            PaperSize::Legal => Some(scan_module::Paper::Legal),
            PaperSize::A4 => Some(scan_module::Paper::A4),
            PaperSize::A5 => Some(scan_module::Paper::A5),
            PaperSize::B5 => bail!("B5 is not supported by this native adapter"),
        };
        let request = scan_module::ScanRequest {
            device_id: Some(request.device.id.clone()),
            output_dir: request.work_dir.into(),
            settings: scan_module::ScanSettings {
                dpi: Some(s.resolution),
                color: Some(match s.color_mode {
                    ColorMode::Color | ColorMode::Auto => scan_module::ColorMode::Color,
                    _ => scan_module::ColorMode::Gray,
                }),
                source: match s.source {
                    Source::Auto => None,
                    Source::Flatbed => Some("Flatbed".into()),
                    Source::Feeder => Some("Feeder".into()),
                },
                duplex: Some(s.sides == Sides::Both),
                paper,
                max_pages: 250,
                native_interface: s.use_native_ui,
            },
        };
        let batch = self.0.acquire(
            &request,
            &scan_module::CancellationToken::default(),
            &mut |p| log(format!("{p:?}")),
        )?;
        if let Some(warning) = batch.warning {
            log(warning);
        }
        batch
            .pages
            .iter()
            .map(|p| image::open(p).with_context(|| format!("reading {}", p.display())))
            .collect()
    }
}
