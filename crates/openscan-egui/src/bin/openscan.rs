//! A complete eframe/wgpu host with caller-selected, persistent storage.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#[path = "../../examples/support/mod.rs"]
mod support;

struct App {
    scanner: openscan_egui::OpenScanUi,
    closing: bool,
}
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if ui.input(|input| input.viewport().close_requested()) && self.scanner.is_busy() {
            self.closing = true;
            self.scanner.cancel();
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        ui.add_enabled_ui(!self.closing, |ui| self.scanner.show(ui));
        if self.closing && !self.scanner.is_busy() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--convert") {
        if let Err(e) = convert(&args[1..]) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let paths = match support::Paths::parse(std::env::args_os().skip(1)) {
        Ok(Some(paths)) => paths,
        Ok(None) => {
            println!(
                "OpenScan Rust\nUsage: standalone [--storage DIRECTORY] [--models DIRECTORY]\nStorage defaults to this user's application-data directory. Models default to STORAGE/models or OPENSCAN_MODELS."
            );
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error}\nUse --help for usage.");
            std::process::exit(2);
        }
    };
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 780.0])
            .with_min_inner_size([400.0, 300.0])
            .with_maximized(false),
        ..Default::default()
    };
    eframe::run_native(
        "Scan — OpenScan (Rust)",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::ThemePreference::Light);
            Ok(Box::new(App {
                scanner: openscan_egui::OpenScanUi::new(paths.storage, paths.models).standalone(),
                closing: false,
            }))
        }),
    )
}

use openscan_core::{backend, ocr, pipeline, settings};
/// Converts image files to one cleaned PDF using the default settings.
///
/// Options, given before the output name: `--mode=bw|grey|color`, `--quality=N`,
/// `--reduce-to=DPI` (0 keeps the scan resolution), `--scanned-at=DPI`, `--background=LEVEL`.
fn convert(args: &[String]) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    use settings::{ColorMode, Level};
    use std::path::PathBuf;

    let models = std::env::var_os("OPENSCAN_MODELS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("models")))
        });
    let engine = ocr::find(models.as_deref());
    let mut settings = settings::ScanSettings {
        ocr: engine.is_some(),
        ..Default::default()
    };
    let mut rest = args;
    while let Some((option, value)) = rest
        .first()
        .and_then(|arg| arg.strip_prefix("--"))
        .and_then(|arg| arg.split_once('='))
    {
        let number = || value.parse::<u32>().with_context(|| format!("--{option}"));
        match option {
            "mode" => {
                settings.color_mode = match value {
                    "bw" => ColorMode::BlackWhite,
                    "grey" | "gray" => ColorMode::Grayscale,
                    "color" | "colour" => ColorMode::Color,
                    _ => bail!("--mode must be bw, grey or color"),
                }
            }
            "quality" => settings.optimize_opts.jpeg_quality = number()?.clamp(1, 100) as u8,
            "reduce-to" => settings.optimize_opts.downsample_dpi = number()?,
            "scanned-at" => settings.resolution = number()?,
            "background" => {
                settings.optimize_opts.background_removal = match value {
                    "off" => Level::Off,
                    "low" => Level::Low,
                    "medium" => Level::Medium,
                    "high" => Level::High,
                    _ => bail!("--background must be off, low, medium or high"),
                }
            }
            _ => bail!("unknown option --{option}"),
        }
        rest = &rest[1..];
    }

    let (output, inputs) = rest
        .split_first()
        .context("usage: scan-module --convert [OPTIONS] OUTPUT.pdf IMAGE...")?;
    let inputs: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let device = backend::discover()
        .0
        .into_iter()
        .find(|device| device.backend == backend::FILE_IMPORT_BACKEND)
        .context("the file import back end is missing")?;

    let log = |message: String| eprintln!("{message}");
    let pages = pipeline::scan_and_clean(&device, &settings, &inputs, &log)?;
    let engine = engine.as_deref();
    let target = pipeline::Target::NewPdf(PathBuf::from(output));
    for path in pipeline::save(&pages, &settings, &target, engine, &log)? {
        println!("{}", path.display());
    }
    Ok(())
}
