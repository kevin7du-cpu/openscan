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
    fn options(&self, id: &str) -> Result<String> {
        let native = id
            .strip_prefix("sane:")
            .ok_or_else(|| error("Invalid SANE identifier."))?;
        let (status, text) = self.run(
            &[format!("--device-name={native}"), "--help".into()],
            &Cancellation::default(),
            Duration::from_secs(30),
        )?;
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
fn dpi_range(text: &str) -> (u32, u32, Vec<u32>) {
    let values = values(text, "--resolution ");
    let joined = values.join("|");
    let numbers: Vec<u32> = joined
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|v| v.parse().ok())
        .filter(|v| (1..=9600).contains(v))
        .collect();
    let min = numbers.iter().copied().min().unwrap_or(75).max(75);
    let max = numbers.iter().copied().max().unwrap_or(600).min(600);
    let list = if joined.contains("..") {
        [75, 100, 150, 200, 300, 400, 600]
            .into_iter()
            .filter(|v| *v >= min && *v <= max)
            .collect()
    } else {
        numbers
            .into_iter()
            .filter(|v| *v >= min && *v <= max)
            .collect()
    };
    (min, max, list)
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
        let text = self.options(id)?;
        let (min_dpi, max_dpi, dpi) = dpi_range(&text);
        let sources = [Source::Flatbed, Source::Feeder, Source::Duplex]
            .into_iter()
            .filter(|s| source_name(&text, *s).is_ok())
            .map(|source| SourceCaps {
                source,
                dpi: dpi.clone(),
                min_dpi,
                max_dpi,
            })
            .collect();
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
        let options = self.options(id)?;
        let source = source_name(&options, p.source)?;
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
        fs::create_dir_all(directory)?;
        if fs::read_dir(directory)?.next().is_some() {
            return Err(error(
                "Acquisition folder must be empty to preserve earlier pages.",
            ));
        }
        let (w, h) = p.paper.millimeters();
        let count = if p.source == Source::Flatbed {
            1
        } else {
            p.max_pages
        };
        let mut args = vec![
            format!("--device-name={native}"),
            format!("--mode={mode}"),
            format!("--resolution={}", p.dpi),
            "--format=png".into(),
            format!("--batch={}", directory.join("raw-%04d.png").display()),
            format!("--batch-count={count}"),
            "--batch-start=1".into(),
        ];
        if !source.is_empty() {
            args.push(format!("--source={source}"));
        }
        if options.lines().any(|l| l.trim().starts_with("-x ")) {
            args.extend(["-x".into(), w.to_string()]);
        }
        if options.lines().any(|l| l.trim().starts_with("-y ")) {
            args.extend(["-y".into(), h.to_string()]);
        }
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
            dpi_range(HELP),
            (75, 600, vec![75, 100, 150, 200, 300, 400, 600])
        );
        assert!(source_name("--source Flatbed [Flatbed]", Source::Duplex).is_err());
        assert_eq!(
            source_name("--mode Color|Gray [Color]", Source::Flatbed).expect("fixed source"),
            ""
        );
        assert!(source_name("--mode Color|Gray [Color]", Source::Duplex).is_err());
    }
}
