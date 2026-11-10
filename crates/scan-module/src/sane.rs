use super::*;
use std::process::{Command, Stdio};

fn scanimage_command() -> Command {
    if let Some(path) = std::env::var_os("OPENSCAN_SCANIMAGE") {
        return Command::new(path);
    }
    // Finder-launched macOS applications often lack the package manager's PATH.
    #[cfg(target_os = "macos")]
    for path in [
        "/opt/homebrew/bin/scanimage",
        "/usr/local/bin/scanimage",
        "/opt/local/bin/scanimage",
    ] {
        if std::path::Path::new(path).is_file() {
            return Command::new(path);
        }
    }
    Command::new("scanimage")
}

fn scanimage_error(error: std::io::Error) -> ScanError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ScanError::Unsupported(
            "SANE scanimage was not found. Install sane-utils on Debian/Ubuntu or sane-backends on macOS; use OPENSCAN_SCANIMAGE for a custom executable path. You can also import scans from another application.".into(),
        )
    } else {
        ScanError::Backend(format!("Could not run SANE scanimage: {error}"))
    }
}

// Some scanner backends incorrectly write binary diagnostics to stdout before
// the formatted SANE device list. Never expose those bytes as device IDs.
fn parse_devices(stdout: &[u8]) -> Vec<Device> {
    let output = String::from_utf8_lossy(stdout);
    let mut devices: Vec<Device> = Vec::new();
    for line in output.lines() {
        // Device formats are "<backend>:<identifier>\t<display name>".
        // Use the LAST tab: backend noise can contain other tabs.
        let Some((raw_id, raw_name)) = line.rsplit_once('\t') else {
            continue;
        };
        let name = raw_name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            continue;
        }

        // Recover a valid ID even when binary garbage precedes it on a line.
        // Known prefixes also cover scanners already seen on this machine.
        let prefixes = [
            "fujitsu:",
            "escl:",
            "hpaio:",
            "airscan:",
            "genesys:",
            "epson:",
            "epson2:",
            "pixma:",
            "brother:",
            "xerox:",
            "canon:",
            "net:",
            "avision:",
            "hp:",
            "gt68xx:",
            "plustek:",
            "snapscan:",
        ];
        let raw_id = raw_id.trim();
        // Valid IDs from other SANE backends may themselves contain a known
        // prefix (for example a network device). Preserve the complete ID.
        let id = if discovery_id_is_valid(raw_id) {
            raw_id
        } else {
            prefixes
                .iter()
                .filter_map(|prefix| raw_id.rfind(prefix).map(|start| &raw_id[start..]))
                .max_by_key(|candidate| candidate.len())
                .unwrap_or(raw_id)
                .trim()
        };
        if !discovery_id_is_valid(id) {
            continue;
        }
        if !devices.iter().any(|d| d.id == id) {
            devices.push(Device {
                id: id.into(),
                name: name.into(),
            });
        }
    }
    devices
}

fn discovery_id_is_valid(id: &str) -> bool {
    let Some((backend, rest)) = id.split_once(':') else {
        return false;
    };
    !backend.is_empty()
        && !rest.is_empty()
        && backend
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !id.chars().any(char::is_control)
        && !id.contains('\u{fffd}')
        && id.len() <= 512
}

fn validate_device_id(id: &str) -> Result<()> {
    if id.contains('\0') || id.chars().any(char::is_control) || id.contains('\u{fffd}') {
        return Err(ScanError::Backend(
            "Invalid SANE scanner identifier (binary/control characters). Refresh scanners and select the scanner again.".into()
        ));
    }
    Ok(())
}

pub struct SaneBackend;
impl ScannerBackend for SaneBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "SANE scanimage",
            driver_dialog: false,
            programmatic_settings: true,
            cancellation_note: "Cancellation terminates the scanimage child; partial files remain caller-owned",
        }
    }
    fn devices(&self) -> Result<Vec<Device>> {
        let output = scanimage_command()
            .args(["-f", "%d\t%v %m%n"])
            .output()
            .map_err(scanimage_error)?;
        if !output.status.success() {
            return Err(ScanError::Backend(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Ok(parse_devices(&output.stdout))
    }
    fn device_capabilities(&self, id: &str) -> Result<DeviceCapabilities> {
        let options = device_options(id)?;
        let mut sources = Vec::new();
        for source in [ScanSource::Flatbed, ScanSource::Feeder, ScanSource::Duplex] {
            if source_name(&options, source).is_some() {
                sources.push(SourceCapabilities {
                    source,
                    resolutions: vec![75, 100, 150, 200, 300, 400, 600],
                });
            }
        }
        Ok(DeviceCapabilities{device_id:id.into(),sources,note:"SANE-reported sources; listed DPI values are requests and are validated by scanimage. Vendor dialogs are unavailable.".into()})
    }
    fn acquire(
        &self,
        r: &ScanRequest,
        c: &CancellationToken,
        p: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch> {
        c.check()?;
        validate_request(r)?;
        if r.settings.native_interface && r.settings != ScanSettings::default() {
            return Err(ScanError::Unsupported(
                "SANE has no vendor dialog; turn off Use scanner's interface".into(),
            ));
        }
        let requested = if r.settings.duplex == Some(true) {
            ScanSource::Duplex
        } else if r.settings.source.as_deref() == Some("Feeder") {
            ScanSource::Feeder
        } else {
            ScanSource::Flatbed
        };
        let feeder = requested != ScanSource::Flatbed;
        let path = r.output_dir.join("scan.png");
        let errors = r.output_dir.join("scanner-stderr.txt");
        let mut command = scanimage_command();
        command.arg("--format=png");
        if feeder {
            command
                .arg(format!(
                    "--batch={}",
                    r.output_dir.join("scan-%04d.png").display()
                ))
                .arg(format!("--batch-count={}", r.settings.max_pages));
        } else {
            command.arg("--output-file").arg(&path);
        }
        if let Some(id) = &r.device_id {
            validate_device_id(id)?;
            command.arg("--device-name").arg(id);
        }
        if let Some(dpi) = r.settings.dpi {
            command.arg("--resolution").arg(dpi.to_string());
        }
        if let Some(color) = r.settings.color {
            command.arg("--mode").arg(if color == ColorMode::Color {
                "Color"
            } else {
                "Gray"
            });
        }
        if let Some(id) = &r.device_id
            && r.settings.source.is_some()
        {
            let options = device_options(id)?;
            let source = source_name(&options, requested).ok_or_else(|| {
                ScanError::Unsupported("Requested source is unavailable in this SANE driver".into())
            })?;
            command.arg("--source").arg(source);
        }
        if let Some(paper) = r.settings.paper {
            let (width, height) = paper.inches();
            command
                .arg("-x")
                .arg((width * 25.4).to_string())
                .arg("-y")
                .arg((height * 25.4).to_string());
        }
        p(Progress::Connecting);
        let mut child = command
            .stdout(Stdio::null())
            .stderr(fs::File::create(&errors)?)
            .spawn()
            .map_err(scanimage_error)?;
        p(Progress::Acquiring);
        let warning = loop {
            if c.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                break Some("Scanning cancelled; completed pages were retained".into());
            }
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    break Some(fs::read_to_string(&errors).unwrap_or_default());
                }
                break None;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        };
        let mut pages = Vec::new();
        for entry in fs::read_dir(&r.output_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "png") && processing::load(&path).is_ok() {
                pages.push(path);
            }
        }
        pages.sort();
        for path in &pages {
            p(Progress::PageReady(path.clone()));
        }
        if pages.is_empty() {
            return Err(if c.is_cancelled() {
                ScanError::Cancelled
            } else {
                ScanError::Backend(warning.unwrap_or_else(|| "No complete pages received".into()))
            });
        }
        Ok(ScanBatch {
            pages,
            cancelled: c.is_cancelled(),
            warning,
            dpi: r.settings.dpi,
        })
    }
}
fn device_options(id: &str) -> Result<String> {
    validate_device_id(id)?;
    let output = scanimage_command()
        .arg("--device-name")
        .arg(id)
        .arg("--all-options")
        .output()
        .map_err(scanimage_error)?;
    if !output.status.success() {
        return Err(ScanError::Backend(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn source_name(options: &str, source: ScanSource) -> Option<String> {
    let line = options
        .lines()
        .find(|l| l.trim_start().starts_with("--source "))?;
    let names = line
        .trim()
        .strip_prefix("--source ")?
        .split('[')
        .next()?
        .trim();
    names
        .split('|')
        .map(str::trim)
        .find(|name| {
            let name = name.to_lowercase();
            match source {
                ScanSource::Flatbed => name.contains("flatbed"),
                ScanSource::Feeder => {
                    (name.contains("feeder") || name.contains("adf")) && !name.contains("duplex")
                }
                ScanSource::Duplex => name.contains("duplex"),
            }
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_discovery_ignores_binary_prefix_and_finds_both_scanners() {
        let data = b"\x1f\x8b\x08\x00\x00\x01\x00\tgarbage\n\xff\0fujitsu:fi-7030:TEST1234\tFUJITSU fi-7030\nescl:https://192.0.2.147:443\tHP Smart Tank 7600\n";
        let devices = parse_devices(data);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "fujitsu:fi-7030:TEST1234");
        assert_eq!(devices[1].id, "escl:https://192.0.2.147:443");
    }
    #[test]
    fn nul_device_id_is_rejected_before_process_launch() {
        assert!(validate_device_id("fujitsu:fi-7030:TEST1234\0").is_err());
        assert!(validate_device_id("fujitsu:fi-7030:TEST1234").is_ok());
    }
    #[test]
    fn discovery_preserves_complete_valid_ids_and_deduplicates() {
        let data = b"custom_backend:server:hp:device\tCustom scanner\nnet:server:fujitsu:fi-7030\tNetwork scanner\ncustom_backend:server:hp:device\tDuplicate\n\xff\0net:server:fujitsu:fi-7030\tNoisy duplicate\n";
        let devices = parse_devices(data);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "custom_backend:server:hp:device");
        assert_eq!(devices[1].id, "net:server:fujitsu:fi-7030");
    }
    #[test]
    fn discovery_rejects_malformed_and_binary_identifiers() {
        let data = b"diagnostics\tNoise\n:missing-backend\tBad\nfujitsu:\tEmpty\nfujitsu:bad\0id\tBad\ncustom:\xff\tBad\ncustom:valid\t\ncustom:valid\tBad\x01name\n";
        assert!(parse_devices(data).is_empty());
        for id in [
            "fujitsu:bad\0id",
            "fujitsu:bad\tid",
            "fujitsu:bad\nid",
            "fujitsu:bad\u{fffd}id",
        ] {
            assert!(
                device_options(id)
                    .unwrap_err()
                    .to_string()
                    .contains("Invalid SANE scanner identifier")
            );
        }
    }
    #[test]
    fn missing_sane_has_installation_and_import_guidance() {
        let error = scanimage_error(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(matches!(error, ScanError::Unsupported(_)));
        let message = error.to_string();
        assert!(message.contains("sane-utils"));
        assert!(message.contains("macOS"));
        assert!(message.contains("import"));
    }
    #[test]
    fn preserves_sane_driver_source_names_and_reports_no_vendor_dialog() {
        let options = "  --source Flatbed|ADF Front|ADF Duplex [Flatbed]\n";
        assert_eq!(
            source_name(options, ScanSource::Flatbed).as_deref(),
            Some("Flatbed")
        );
        assert_eq!(
            source_name(options, ScanSource::Feeder).as_deref(),
            Some("ADF Front")
        );
        assert_eq!(
            source_name(options, ScanSource::Duplex).as_deref(),
            Some("ADF Duplex")
        );
        assert!(source_name("--source Flatbed [Flatbed]", ScanSource::Duplex).is_none());
        assert!(!SaneBackend.capabilities().driver_dialog);
    }
}
