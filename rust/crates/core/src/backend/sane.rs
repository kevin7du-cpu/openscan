//! SANE's installed scanimage driver bridge. The adapter and all image processing are Rust.
//! No shell, scripts, embedded SANE library, or application UI dependency is used.
use super::*;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
pub struct Sane {
    pub executable: PathBuf,
}
impl Sane {
    pub fn system() -> Result<Self> {
        if let Some(path) = std::env::var_os("OPENSCAN_SCANIMAGE") {
            let path = PathBuf::from(path);
            if !path.is_absolute() || !path.is_file() {
                return Err(error(
                    "OPENSCAN_SCANIMAGE must name an absolute scanimage executable.",
                ));
            }
            return Ok(Self { executable: path });
        }
        for path in [
            "/usr/bin/scanimage",
            "/usr/local/bin/scanimage",
            "/opt/homebrew/bin/scanimage",
            "/opt/local/bin/scanimage",
        ] {
            if Path::new(path).is_file() {
                return Ok(Self {
                    executable: PathBuf::from(path),
                });
            }
        }
        Err(error(
            "Install your platform's SANE scanner drivers and scanimage (sane-utils on Linux or sane-backends on macOS), or select an AirScan network scanner.",
        ))
    }
    fn run(
        &self,
        args: &[String],
        cancel: &Cancellation,
        timeout: Duration,
    ) -> Result<(ExitStatus, String)> {
        let mut child = Command::new(&self.executable)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| error("Missing scanner output pipe."))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| error("Missing scanner error pipe."))?;
        let out = std::thread::spawn(move || read_bounded(stdout));
        let err = std::thread::spawn(move || read_bounded(stderr));
        let start = Instant::now();
        let mut stopped = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancel.cancelled() || start.elapsed() > timeout {
                stopped = true;
                let _ = child.kill();
                break child.wait()?;
            }
            std::thread::sleep(Duration::from_millis(40));
        };
        let stdout = out
            .join()
            .map_err(|_| error("Scanner output reader stopped."))??;
        let stderr = err
            .join()
            .map_err(|_| error("Scanner error reader stopped."))??;
        if stopped {
            return Err(error(
                "Scanner operation cancelled or timed out. Completed images remain in the session.",
            ));
        }
        Ok((status, format!("{stdout}\n{stderr}")))
    }
    fn options(&self, id: &str, configured: &[String], cancel: &Cancellation) -> Result<String> {
        let native = id
            .strip_prefix("sane:")
            .ok_or_else(|| error("Invalid SANE identifier."))?;
        let mut args = vec![format!("--device-name={native}")];
        args.extend_from_slice(configured);
        // SANE processes --help after the settings, without acquiring a page.
        // Source and mode can change the available resolution and scan area.
        args.push("--help".into());
        cancel.check()?;
        let (status, text) = self.run(&args, cancel, Duration::from_secs(30))?;
        if !status.success() {
            return Err(error(format!(
                "SANE could not read scanner options: {text}"
            )));
        }
        Ok(text)
    }
}
fn read_bounded(reader: impl Read) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(error("SANE response exceeds 1 MB."));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
fn values(text: &str, option: &str) -> Vec<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(option))
        .map(|s| {
            s.split('[')
                .next()
                .unwrap_or("")
                .trim()
                .split('|')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn source_name(text: &str, source: Source) -> Result<String> {
    let options = values(text, "--source ");
    // Many flatbed drivers have no source selector at all.
    if options.is_empty() && source == Source::Flatbed {
        return Ok(String::new());
    }
    options
        .into_iter()
        .find(|v| {
            let v = v.to_ascii_lowercase();
            let feeder = v.contains("adf") || v.contains("feeder");
            let duplex = v.contains("duplex") || v.contains("both");
            match source {
                Source::Flatbed => v.contains("flatbed") || v == "flat",
                Source::Feeder => feeder && !duplex,
                Source::Duplex => feeder && duplex,
            }
        })
        .ok_or_else(|| {
            error(format!(
                "The SANE driver does not advertise {source:?} scanning."
            ))
        })
}
fn option_line<'a>(text: &'a str, option: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(option))
}

fn leading_number(text: &str) -> Option<f64> {
    let number = text
        .trim_start()
        .split(|c: char| !c.is_ascii_digit() && !['.', '-', '+'].contains(&c))
        .next()?
        .parse::<f64>()
        .ok()?;
    number.is_finite().then_some(number)
}

fn numeric_range(text: &str) -> Option<(f64, f64, f64)> {
    let (start, remainder) = text.split_once("..")?;
    let min = leading_number(start)?;
    let max = leading_number(remainder)?;
    let step = match text.split_once("in steps of ") {
        Some((_, step)) => leading_number(step)?,
        None => 0.0,
    };
    (min <= max && step >= 0.0).then_some((min, max, step))
}

fn supported_dpi(text: &str) -> Result<Vec<u32>> {
    let line = option_line(text, "--resolution ")
        .filter(|line| !line.contains("[inactive]"))
        .ok_or_else(|| error("The SANE driver does not expose an active resolution option."))?;
    let specification = line.split('[').next().unwrap_or("");
    let mut supported = if specification.contains("..") {
        let (min, max, step) = numeric_range(specification)
            .ok_or_else(|| error("The SANE driver returned an invalid resolution range."))?;
        // Enumerate only our bounded integer DPI domain. The step is a constraint,
        // never another candidate DPI or a lower endpoint.
        (75..=600)
            .filter(|dpi| {
                let value = f64::from(*dpi);
                value >= min
                    && value <= max
                    && (step == 0.0 || {
                        let steps = (value - min) / step;
                        steps.is_finite() && (steps - steps.round()).abs() < 0.0001
                    })
            })
            .collect::<Vec<_>>()
    } else {
        specification
            .split('|')
            .filter_map(leading_number)
            .filter(|value| (75.0..=600.0).contains(value) && value.fract() == 0.0)
            .map(|value| value as u32)
            .collect()
    };
    supported.sort_unstable();
    supported.dedup();
    if supported.is_empty() {
        return Err(error(
            "The SANE driver has no supported resolution within 75–600 DPI.",
        ));
    }
    Ok(supported)
}

fn dpi_range(text: &str) -> Result<(u32, u32, Vec<u32>)> {
    let supported = supported_dpi(text)?;
    let min = supported
        .first()
        .copied()
        .ok_or_else(|| error("Missing minimum DPI."))?;
    let max = supported
        .last()
        .copied()
        .ok_or_else(|| error("Missing maximum DPI."))?;
    let list = if values(text, "--resolution ").join("|").contains("..") {
        supported
            .into_iter()
            .filter(|dpi| {
                *dpi == min || *dpi == max || [75, 100, 150, 200, 300, 400, 600].contains(dpi)
            })
            .collect()
    } else {
        supported
    };
    Ok((min, max, list))
}

fn scan_area_arguments(options: &str, paper: Paper) -> Result<Vec<String>> {
    let (width, height) = paper.millimeters();
    let mut args = Vec::new();
    for (option, length) in [("-x ", width), ("-y ", height)] {
        let Some(line) = option_line(options, option) else {
            return Err(error(
                "The SANE driver does not expose a configurable paper size.",
            ));
        };
        let (min, max, step) = numeric_range(line)
            .filter(|_| line.contains("mm") && !line.contains("[inactive]"))
            .ok_or_else(|| {
                error("The SANE driver does not report a usable scan area in millimeters.")
            })?;
        // Drivers can silently clamp oversized paper. Reject it before acquisition
        // instead of presenting a cropped document as a complete page.
        if f64::from(length) < min - 0.01 || f64::from(length) > max + 0.01 {
            return Err(error(format!(
                "The selected paper size exceeds the scanner's {} range ({min}–{max} mm).",
                option.trim()
            )));
        }
        if step > 0.0 {
            let steps = (f64::from(length) - min) / step;
            let rounded = min + steps.round() * step;
            if !rounded.is_finite() || (rounded - f64::from(length)).abs() > 0.01 {
                return Err(error(
                    "The selected paper size does not match the SANE driver's scan-area increments.",
                ));
            }
        }
        args.extend([option.trim().into(), length.to_string()]);
    }
    Ok(args)
}
impl Backend for Sane {
    fn devices(&self, cancel: &Cancellation) -> Result<Vec<Device>> {
        let (status, text) = self.run(
            &["--formatted-device-list=%d\t%v %m%n".into()],
            cancel,
            Duration::from_secs(30),
        )?;
        if !status.success() {
            return Err(error(format!("SANE discovery failed: {text}")));
        }
        Ok(text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .take(256)
            .map(|(id, name)| Device {
                id: format!("sane:{id}"),
                name: name.trim().into(),
                driver: "SANE".into(),
            })
            .collect())
    }
    fn capabilities(&self, id: &str) -> Result<Capabilities> {
        let cancel = Cancellation::default();
        let text = self.options(id, &[], &cancel)?;
        let mut sources = Vec::new();
        for source in [Source::Flatbed, Source::Feeder, Source::Duplex] {
            let Ok(name) = source_name(&text, source) else {
                continue;
            };
            let options = if name.is_empty() {
                text.clone()
            } else {
                self.options(id, &[format!("--source={name}")], &cancel)?
            };
            let (min_dpi, max_dpi, dpi) = dpi_range(&options)?;
            sources.push(SourceCaps {
                source,
                dpi,
                min_dpi,
                max_dpi,
            });
        }
        Ok(Capabilities{device_id:id.into(),sources,note:"Options reported by the installed SANE driver. Driver availability varies by scanner.".into()})
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
        let id = p
            .device_id
            .as_deref()
            .ok_or_else(|| error("Select a scanner."))?;
        let native = id
            .strip_prefix("sane:")
            .ok_or_else(|| error("Invalid SANE identifier."))?;
        let options = self.options(id, &[], cancel)?;
        let source = source_name(&options, p.source)?;
        let mut configured = Vec::new();
        let options = if source.is_empty() {
            options
        } else {
            configured.push(format!("--source={source}"));
            self.options(id, &configured, cancel)?
        };
        let modes = values(&options, "--mode ");
        let mode = modes
            .into_iter()
            .find(|v| {
                if p.color == Color::Color {
                    v.eq_ignore_ascii_case("color")
                } else {
                    v.eq_ignore_ascii_case("gray") || v.eq_ignore_ascii_case("grayscale")
                }
            })
            .ok_or_else(|| {
                error("The scanner driver does not advertise the requested color mode.")
            })?;
        configured.push(format!("--mode={mode}"));
        let options = self.options(id, &configured, cancel)?;
        if !supported_dpi(&options)?.contains(&p.dpi) {
            return Err(error(format!(
                "The selected source and color mode do not support {} DPI.",
                p.dpi
            )));
        }
        configured.push(format!("--resolution={}", p.dpi));
        let options = self.options(id, &configured, cancel)?;
        configured.extend(scan_area_arguments(&options, p.paper)?);
        fs::create_dir_all(directory)?;
        if fs::read_dir(directory)?.next().is_some() {
            return Err(error(
                "Acquisition folder must be empty to preserve earlier pages.",
            ));
        }
        let count = if p.source == Source::Flatbed {
            1
        } else {
            p.max_pages
        };
        let mut args = vec![format!("--device-name={native}")];
        // Set source before mode/DPI/area, since later source changes may reset them.
        args.extend(configured);
        args.extend([
            "--format=png".into(),
            format!("--batch={}", directory.join("raw-%04d.png").display()),
            format!("--batch-count={count}"),
            "--batch-start=1".into(),
        ]);
        let result = self.run(&args, cancel, Duration::from_secs(900));
        let mut received = 0;
        for i in 1..=count {
            let path = directory.join(format!("raw-{i:04}.png"));
            if path.is_file() && correction::read(&path).is_ok() {
                on_page(RawPage { path, dpi: p.dpi })?;
                received += 1;
            }
        }
        let (status, text) = result?;
        if !status.success() && !(received > 0 && status.code() == Some(7)) {
            return Err(error(format!("SANE scan failed: {text}")));
        }
        if received == 0 {
            return Err(error("No pages received. Check the feeder and scanner."));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const HELP: &str = "    --source Flatbed|ADF Front|ADF Duplex [Flatbed]\n    --mode Color|Gray|Lineart [Color]\n    --resolution 75..600dpi [300]\n";
    #[test]
    fn backend_option_names_and_resolution_are_preserved() {
        assert_eq!(
            source_name(HELP, Source::Duplex).expect("source"),
            "ADF Duplex"
        );
        assert_eq!(
            dpi_range(HELP).expect("DPI"),
            (75, 600, vec![75, 100, 150, 200, 300, 400, 600])
        );
        assert!(source_name("--source Flatbed [Flatbed]", Source::Duplex).is_err());
        assert_eq!(
            source_name("--mode Color|Gray [Color]", Source::Flatbed).expect("fixed source"),
            ""
        );
        assert!(source_name("--mode Color|Gray [Color]", Source::Duplex).is_err());
    }

    #[test]
    fn sane_resolution_steps_are_not_mistaken_for_lower_resolutions() {
        assert_eq!(
            dpi_range("--resolution 200..1200dpi (in steps of 100) [300]").expect("DPI"),
            (200, 600, vec![200, 300, 400, 600])
        );
        assert_eq!(
            dpi_range("--resolution 75..600dpi (in steps of 75) [300]").expect("DPI"),
            (75, 600, vec![75, 150, 300, 600])
        );
        assert_eq!(
            supported_dpi("--resolution 200|300|600|1200dpi [300]").expect("list"),
            vec![200, 300, 600]
        );
        assert!(supported_dpi("--resolution 601..1200dpi [1200]").is_err());
        assert!(supported_dpi("--resolution 75..600dpi [inactive]").is_err());
        assert!(supported_dpi("--resolution invalid [300]").is_err());
        assert!(supported_dpi("--resolution 600..75dpi [300]").is_err());
        assert!(supported_dpi("--mode Color [Color]").is_err());
    }

    #[test]
    fn sane_rejects_paper_larger_than_the_selected_source() {
        let options = "-x 0..216mm [216]\n-y 0..297mm [297]";
        assert_eq!(
            scan_area_arguments(options, Paper::A4).expect("A4"),
            vec!["-x", "210", "-y", "297"]
        );
        assert!(scan_area_arguments(options, Paper::Legal).is_err());
        assert!(scan_area_arguments("-x 0..200mm [200]\n-y 0..200mm [200]", Paper::A5).is_err());
        assert!(scan_area_arguments("-x 0..200mm [200]", Paper::A5).is_err());
        assert!(scan_area_arguments("-x 0..900px [900]\n-y 0..900px [900]", Paper::A5).is_err());
        assert!(
            scan_area_arguments(
                "-x 0..400mm (in steps of 10) [200]\n-y 0..400mm (in steps of 10) [200]",
                Paper::Letter
            )
            .is_err()
        );
    }
}
