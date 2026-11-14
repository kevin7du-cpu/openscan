// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scanning on Linux through `scanimage`, the command-line tool that ships with SANE.
//!
//! Scanner drivers name their paper sources and colour modes differently ("ADF", "ADF Front",
//! "Automatic Document Feeder"), so each scan first asks the scanner what it offers.

use anyhow::{Result, bail};
use image::DynamicImage;

use super::{Device, ScanRequest, ScannerBackend, find_program, load_images_sorted, run};
use crate::settings::{ColorMode, Sides, Source};

const NAME: &str = "sane";

pub struct SaneBackend;

impl ScannerBackend for SaneBackend {
    fn name(&self) -> &'static str {
        NAME
    }

    fn list_devices(&self) -> Result<Vec<Device>> {
        let Some(exe) = sane_program() else {
            return Ok(Vec::new());
        };
        // One line per device: its SANE name, a tab, then maker and model.
        let output = run(super::command(exe).args(["--formatted-device-list", "%d\t%v %m%n"]))?;
        Ok(parse_devices(&output))
    }

    fn scan(&self, request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>> {
        if !valid_device_id(&request.device.id) {
            bail!("Invalid scanner identifier. Refresh scanners and choose the scanner again.");
        }
        let Some(exe) = sane_program() else {
            bail!("scanimage is not installed");
        };
        let settings = request.settings;
        // Each scanner driver has its own names for paper sources and colour modes, so ask.
        let options =
            run(super::command(&exe).args(["--device-name", &request.device.id, "--all-options"]))
                .map(|text| DeviceOptions::parse(&text))
                .unwrap_or_default();

        let mut command = super::command(exe);
        command
            .args(["--device-name", &request.device.id, "--format=png"])
            .args(["--resolution", &settings.resolution.to_string()]);

        if let Some(mode) = options.mode_for(settings.color_mode) {
            command.args(["--mode", &mode]);
        }
        if let Some((width, height)) = settings.paper_size.size_mm() {
            command.args(["-x", &width.to_string(), "-y", &height.to_string()]);
        }

        let source = match options.source_for(settings.sides, settings.source) {
            Ok(source) => source,
            Err(problem) => {
                bail!("{}: {problem}", request.device.name);
            }
        };
        if let Some(source) = &source {
            command.args(["--source", source]);
        }
        // A feeder gives a page per sheet until it is empty; a flatbed gives one page.
        let feeding = match &source {
            Some(source) => is_feeder(source),
            None => options.default_source.as_deref().is_some_and(is_feeder),
        };
        if feeding {
            let pattern = request.work_dir.join("page%04d.png");
            command.arg(format!("--batch={}", pattern.display()));
        } else {
            command
                .arg("--output-file")
                .arg(request.work_dir.join("page0001.png"));
        }

        log(format!("Scanning with {}", request.device.name));
        // A feeder running out of paper ends the batch with an error status, which is normal,
        // so success is judged by whether any pages arrived.
        let outcome = run(&mut command);
        let pages = load_images_sorted(request.work_dir)?;
        if pages.is_empty() {
            outcome?;
            bail!("the scanner returned no pages");
        }
        Ok(pages)
    }
}

/// Some SANE backends emit binary diagnostics before the formatted device list.
/// Adapted from PaperDesk's Rust OpenScan discovery guard (MIT OR Apache-2.0).
/// Never show those bytes as scanner names or pass them back as device IDs.
fn parse_devices(output: &str) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    for line in output.lines() {
        let Some((raw_id, raw_name)) = line.rsplit_once('\t') else {
            continue;
        };
        let name = raw_name.trim();
        if name.is_empty()
            || name.chars().count() > 256
            || name.chars().any(char::is_control)
            || name.contains('\u{fffd}')
        {
            continue;
        }
        let raw_id = raw_id.trim();
        let id = if valid_device_id(raw_id) {
            // Keep complete custom/network IDs, even when they contain a known prefix.
            raw_id
        } else {
            [
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
            ]
            .iter()
            .filter_map(|prefix| raw_id.rfind(prefix).map(|at| raw_id[at..].trim()))
            .filter(|candidate| valid_device_id(candidate))
            .max_by_key(|candidate| candidate.len())
            .unwrap_or("")
        };
        if valid_device_id(id) && !devices.iter().any(|device| device.id == id) {
            devices.push(Device {
                backend: NAME,
                id: id.to_owned(),
                name: format!("{name} (SANE)"),
            });
        }
    }
    devices
}

fn valid_device_id(id: &str) -> bool {
    let Some((backend, rest)) = id.split_once(':') else {
        return false;
    };
    !backend.is_empty()
        && !rest.is_empty()
        && id.len() <= 512
        && backend
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && !id.chars().any(char::is_control)
        && !id.contains('\u{fffd}')
}

/// The paper sources and colour modes one scanner offers, under its driver's own names.
#[derive(Debug, Default, PartialEq, Eq)]
struct DeviceOptions {
    sources: Vec<String>,
    /// The source the scanner uses when none is asked for.
    default_source: Option<String>,
    modes: Vec<String>,
}

impl DeviceOptions {
    /// Reads the output of `scanimage --all-options`, which has lines such as
    /// `--source ADF Front|ADF Back|ADF Duplex [ADF Front]`.
    fn parse(text: &str) -> Self {
        let mut options = Self::default();
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("--source ") {
                let (choices, current) = split_choices(rest);
                options.sources = choices;
                options.default_source = current;
            } else if let Some(rest) = line.strip_prefix("--mode ") {
                options.modes = split_choices(rest).0;
            }
        }
        options
    }

    /// The driver's name for the wanted paper source. `Ok(None)` leaves the choice to the
    /// scanner; an error says what this scanner cannot do.
    fn source_for(
        &self,
        sides: Sides,
        source: Source,
    ) -> std::result::Result<Option<String>, String> {
        let find = |wanted: &dyn Fn(&str) -> bool| {
            self.sources
                .iter()
                .find(|name| wanted(&name.to_lowercase()))
                .cloned()
        };
        if sides == Sides::Both {
            return match find(&|name| name.contains("duplex")) {
                Some(name) => Ok(Some(name)),
                // Nothing was reported: try the usual name and let the scanner decide.
                None if self.sources.is_empty() => Ok(Some("ADF Duplex".to_owned())),
                None => Err(format!(
                    "this scanner cannot scan both sides (it offers: {})",
                    self.sources.join(", ")
                )),
            };
        }
        match source {
            Source::Auto => Ok(None),
            Source::Feeder => match find(&|name| {
                is_feeder(name) && !name.contains("back") && !name.contains("duplex")
            }) {
                Some(name) => Ok(Some(name)),
                None if self.sources.is_empty() => Ok(Some("ADF".to_owned())),
                None => Err(format!(
                    "this scanner has no document feeder (it offers: {})",
                    self.sources.join(", ")
                )),
            },
            Source::Flatbed => {
                match find(&|name| name.contains("flatbed") || name.contains("platen")) {
                    Some(name) => Ok(Some(name)),
                    None if self.sources.is_empty() => Ok(Some("Flatbed".to_owned())),
                    None => Err(format!(
                        "this scanner has no flatbed (it offers: {})",
                        self.sources.join(", ")
                    )),
                }
            }
        }
    }

    /// The driver's name for the wanted colour mode, or `None` to leave it to the scanner.
    fn mode_for(&self, mode: ColorMode) -> Option<String> {
        let (words, usual): (&[&str], &str) = match mode {
            ColorMode::Auto => return None,
            ColorMode::Color => (&["color", "colour"], "Color"),
            ColorMode::Grayscale => (&["gray", "grey"], "Gray"),
            ColorMode::BlackWhite => (&["lineart", "black", "binary", "mono"], "Lineart"),
        };
        if self.modes.is_empty() {
            return Some(usual.to_owned());
        }
        // A mode the scanner does not have is left out, so it scans in its own default.
        self.modes
            .iter()
            .find(|name| words.iter().any(|word| name.to_lowercase().contains(word)))
            .cloned()
    }
}

/// Splits `A|B|C [B]` into the choices and the current one. A scanner that cannot change the
/// option reports it as `[inactive]`, which is no choice at all.
fn split_choices(rest: &str) -> (Vec<String>, Option<String>) {
    let (list, current) = match rest.rfind('[') {
        Some(at) => (
            rest.get(..at).unwrap_or_default(),
            rest.get(at + 1..)
                .map(|current| current.trim_end_matches(']').trim().to_owned()),
        ),
        None => (rest, None),
    };
    let choices = list
        .split('|')
        .map(str::trim)
        .filter(|choice| !choice.is_empty())
        .map(str::to_owned)
        .collect();
    (choices, current.filter(|name| name != "inactive"))
}

/// Whether a paper source name means a sheet feeder.
fn is_feeder(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("adf") || name.contains("feeder")
}

fn sane_program() -> Option<std::path::PathBuf> {
    std::env::var_os("OPENSCAN_SCANIMAGE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            find_program(&[
                "scanimage",
                "/opt/homebrew/bin/scanimage",
                "/usr/local/bin/scanimage",
                "/opt/local/bin/scanimage",
            ])
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_recovers_real_scanners_from_binary_driver_noise() {
        let output = String::from_utf8_lossy(
            b"\xff\0junk\t\x01noise\n\xfe\0prefix\tfujitsu:fi-7030:test\tFUJITSU fi-7030\n\
              escl:https://scanner.local/eSCL\tNetwork Scanner\n",
        );
        let devices = parse_devices(&output);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "fujitsu:fi-7030:test");
        assert_eq!(devices[0].name, "FUJITSU fi-7030 (SANE)");
        assert_eq!(devices[1].id, "escl:https://scanner.local/eSCL");
    }

    #[test]
    fn discovery_preserves_custom_network_ids_and_removes_duplicates() {
        let devices = parse_devices(
            "custom_backend:server:hp:device\tCustom scanner\n\
             net:server:fujitsu:scanner\tShared scanner\n\
             net:server:fujitsu:scanner\tShared scanner\n",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "custom_backend:server:hp:device");
        assert_eq!(devices[1].id, "net:server:fujitsu:scanner");
    }

    #[test]
    fn discovery_rejects_corrupted_names_and_identifiers() {
        let mut output = "diagnostic\tNot a device\n:empty\tBad\nfujitsu:\tEmpty\n\
            fujitsu:bad\0id\tBad\ncustom:bad\u{fffd}\tBad\ncustom:ok\tBad\u{fffd}name\n\
            custom:ok\tBad\x01name\ncustom:empty\t\n"
            .to_owned();
        output.push_str(&format!("custom:long\t{}\n", "x".repeat(257)));
        assert!(parse_devices(&output).is_empty());
        for id in [
            "no-backend",
            "fujitsu:bad\0id",
            "custom:bad\u{fffd}",
            "fujitsu:bad\tid",
        ] {
            assert!(!valid_device_id(id));
        }
    }

    /// What a Fujitsu sheet-fed scanner reports (shortened).
    const FUJITSU: &str = "Options specific to device `fujitsu:fi-7030:123456':
  Standard:
    --source ADF Front|ADF Back|ADF Duplex [ADF Front]
        Selects the scan source (such as a document-feeder).
    --mode Lineart|Halftone|Gray|Color [Lineart]
        Selects the scan mode (e.g., lineart, monochrome, or color).
    --resolution 50..600dpi (in steps of 1) [600]
";

    /// A flatbed with a feeder, as many multifunction printers report.
    const FLATBED: &str = "    --mode Black & White|Grayscale|24bit Color [24bit Color]
    --source Flatbed|Automatic Document Feeder [Flatbed]
";

    #[test]
    fn a_sheet_fed_scanner_gets_its_own_source_names() {
        let options = DeviceOptions::parse(FUJITSU);
        assert_eq!(options.sources, ["ADF Front", "ADF Back", "ADF Duplex"]);
        assert_eq!(options.default_source.as_deref(), Some("ADF Front"));
        assert_eq!(
            options.source_for(Sides::Front, Source::Feeder),
            Ok(Some("ADF Front".to_owned()))
        );
        assert_eq!(
            options.source_for(Sides::Both, Source::Auto),
            Ok(Some("ADF Duplex".to_owned()))
        );
        assert_eq!(options.source_for(Sides::Front, Source::Auto), Ok(None));
        // It has no glass to lay a page on, and says so instead of failing inside scanimage.
        let problem = options
            .source_for(Sides::Front, Source::Flatbed)
            .unwrap_err();
        assert!(
            problem.contains("no flatbed") && problem.contains("ADF Duplex"),
            "{problem}"
        );
        // Left to itself it feeds sheets, so every sheet must be collected.
        assert!(options.default_source.as_deref().is_some_and(is_feeder));
    }

    #[test]
    fn colour_modes_use_the_names_the_scanner_has() {
        let fujitsu = DeviceOptions::parse(FUJITSU);
        assert_eq!(
            fujitsu.mode_for(ColorMode::Grayscale).as_deref(),
            Some("Gray")
        );
        assert_eq!(
            fujitsu.mode_for(ColorMode::BlackWhite).as_deref(),
            Some("Lineart")
        );
        assert_eq!(fujitsu.mode_for(ColorMode::Auto), None);
        let flatbed = DeviceOptions::parse(FLATBED);
        assert_eq!(
            flatbed.mode_for(ColorMode::Color).as_deref(),
            Some("24bit Color")
        );
        assert_eq!(
            flatbed.mode_for(ColorMode::Grayscale).as_deref(),
            Some("Grayscale")
        );
        assert_eq!(
            flatbed.mode_for(ColorMode::BlackWhite).as_deref(),
            Some("Black & White")
        );
    }

    #[test]
    fn a_flatbed_with_a_feeder_offers_both() {
        let options = DeviceOptions::parse(FLATBED);
        assert_eq!(
            options.source_for(Sides::Front, Source::Flatbed),
            Ok(Some("Flatbed".to_owned()))
        );
        assert_eq!(
            options.source_for(Sides::Front, Source::Feeder),
            Ok(Some("Automatic Document Feeder".to_owned()))
        );
        assert!(options.source_for(Sides::Both, Source::Auto).is_err());
        assert!(!options.default_source.as_deref().is_some_and(is_feeder));
    }

    #[test]
    fn a_scanner_that_reports_nothing_gets_the_usual_names() {
        let options = DeviceOptions::parse("scanimage: nothing useful\n    --source [inactive]\n");
        assert_eq!(options, DeviceOptions::default());
        assert_eq!(
            options.source_for(Sides::Front, Source::Feeder),
            Ok(Some("ADF".to_owned()))
        );
        assert_eq!(options.mode_for(ColorMode::Color).as_deref(), Some("Color"));
    }
}
