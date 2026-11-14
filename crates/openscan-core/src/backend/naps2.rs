// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scanning through the command-line interface of NAPS2, a free and open-source scanning
//! program. NAPS2 already speaks TWAIN and WIA on Windows, Apple's scanner API on macOS and
//! SANE on Linux, plus driverless network scanning (eSCL) everywhere, so one back end here
//! covers real hardware on all three systems.
//!
//! NAPS2 runs as a separate program; nothing from it is linked into this one.
//!
//! The options used are those in NAPS2's command-line documentation
//! (<https://www.naps2.com/doc/command-line>): `--listdevices`, `--noprofile`, `--driver`,
//! `--device`, `--source`, `--dpi`, `--pagesize`, `--bitdepth`.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, bail};
use image::DynamicImage;

use super::{Device, ScanRequest, ScannerBackend, find_program, load_images_sorted, run};
use crate::settings::{ColorMode, PaperSize, Sides, Source};

const NAME: &str = "naps2";

pub struct Naps2Backend;

/// Where NAPS2's console program is, and any leading argument it needs.
fn console() -> Option<(PathBuf, Option<&'static str>)> {
    if let Some(exe) = find_program(&[
        "NAPS2.Console.exe",
        "naps2.console",
        r"C:\Program Files\NAPS2\NAPS2.Console.exe",
        r"C:\Program Files (x86)\NAPS2\NAPS2.Console.exe",
    ]) {
        return Some((exe, None));
    }
    // On Linux and macOS the one program takes "console" as its first argument.
    find_program(&["naps2", "/Applications/NAPS2.app/Contents/MacOS/NAPS2"])
        .map(|exe| (exe, Some("console")))
}

fn command() -> Option<Command> {
    let (exe, first_arg) = console()?;
    let mut command = super::command(exe);
    if let Some(arg) = first_arg {
        command.arg(arg);
    }
    Some(command)
}

/// Driver names NAPS2 accepts on this operating system, most capable first.
fn drivers() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["twain", "wia", "escl"]
    } else if cfg!(target_os = "macos") {
        &["apple", "escl"]
    } else {
        &["sane", "escl"]
    }
}

impl ScannerBackend for Naps2Backend {
    fn name(&self) -> &'static str {
        NAME
    }

    fn list_devices(&self) -> Result<Vec<Device>> {
        let mut devices = Vec::new();
        for driver in drivers() {
            let Some(mut command) = command() else {
                return Ok(devices);
            };
            // A driver with no scanners, or one that is not installed, is not worth reporting.
            let Ok(output) = run(command.args(["--listdevices", "--driver", driver])) else {
                continue;
            };
            for line in output
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                // Newer versions add an identifier in brackets after the name; the name alone
                // is what `--device` matches.
                let name = line
                    .rsplit_once(" [")
                    .filter(|(_, id)| id.ends_with(']'))
                    .map_or(line, |(name, _)| name);
                devices.push(Device {
                    backend: NAME,
                    id: format!("{driver}:{name}"),
                    name: format!("{name} ({})", driver.to_uppercase()),
                });
            }
        }
        Ok(devices)
    }

    fn scan(&self, request: &ScanRequest, log: &dyn Fn(String)) -> Result<Vec<DynamicImage>> {
        let Some(mut command) = command() else {
            bail!("NAPS2 is not installed")
        };
        let Some((driver, device_name)) = request.device.id.split_once(':') else {
            bail!("unrecognised device id {}", request.device.id);
        };
        let settings = request.settings;

        // "$(nnnn)" is NAPS2's placeholder for a four-digit page number.
        let output = request.work_dir.join("page$(nnnn).png");
        command.arg("-o").arg(output).args([
            "--noprofile",
            "--force",
            "--driver",
            driver,
            "--device",
            device_name,
        ]);

        if settings.use_native_ui {
            log("This back end cannot show the scanner's own window; using the settings chosen here.".to_owned());
        }
        command.args(["--dpi", &settings.resolution.to_string()]);

        let source = match (settings.sides, settings.source) {
            (Sides::Both, _) => Some("duplex"),
            (Sides::Front, Source::Feeder) => Some("feeder"),
            (Sides::Front, Source::Flatbed) => Some("glass"),
            (Sides::Front, Source::Auto) => None,
        };
        if let Some(source) = source {
            command.args(["--source", source]);
        }
        let depth = match settings.color_mode {
            ColorMode::Auto => None,
            ColorMode::Color => Some("color"),
            ColorMode::Grayscale => Some("gray"),
            ColorMode::BlackWhite => Some("bw"),
        };
        if let Some(depth) = depth {
            command.args(["--bitdepth", depth]);
        }
        let page_size = match settings.paper_size {
            PaperSize::Auto => None,
            PaperSize::Letter => Some("letter"),
            PaperSize::Legal => Some("legal"),
            PaperSize::A4 => Some("a4"),
            PaperSize::A5 => Some("a5"),
            PaperSize::B5 => Some("b5"),
        };
        if let Some(page_size) = page_size {
            command.args(["--pagesize", page_size]);
        }

        log(format!("Scanning with {}", request.device.name));
        run(&mut command)?;
        let pages = load_images_sorted(request.work_dir)?;
        if pages.is_empty() {
            bail!("the scanner returned no pages");
        }
        Ok(pages)
    }
}
