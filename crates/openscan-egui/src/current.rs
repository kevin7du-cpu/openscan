// SPDX-License-Identifier: MIT OR Apache-2.0
//! The window and its dialogs.
//!
//! Nothing here does slow work. Scanning, clean-up, OCR and saving are handed to a worker
//! thread (see `pipeline`), which reports back through a channel read once per frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{
    Align, Button, ComboBox, Context, Id, Layout, Modal, RichText, ScrollArea, TextureHandle, Ui,
};

use openscan_core::backend::{self, Device, FILE_IMPORT_BACKEND};
use openscan_core::cleanup::Page;
use openscan_core::ocr::{self, OcrEngine};
use openscan_core::pipeline::{self, Target};
use openscan_core::settings::{
    ColorMode, DialogLayout, Level, OptimizeOptions, OutputMode, PaperSize, Preset, PresetStore,
    RESOLUTIONS, ScanSettings, Sides, Source, TransferMechanism,
};

use crate::OpenScanEvent;
use scan_module::{CancellationToken, ScanProfile, session::ScanSession};

enum Event {
    Log(String),
    Saved(Vec<PathBuf>),
    Failed(String),
    Retained(Vec<Page>, ScanSession, bool),
    Accepted(ScanSession),
    Initialized(
        Vec<Device>,
        Vec<String>,
        PresetStore,
        Option<Arc<dyn OcrEngine>>,
        Vec<String>,
    ),
}
fn send(sender: &Sender<Event>, event: Event) {
    let _ = sender.send(event);
}

/// A local control style: embedded scanners never change their host's zoom or theme.
fn configure_controls(ui: &mut Ui) {
    let mut style = egui::Style {
        visuals: egui::Visuals::light(),
        ..Default::default()
    };
    for font in style.text_styles.values_mut() {
        font.size *= 1.1;
    }
    style.spacing.interact_size *= 1.1;
    style.spacing.button_padding *= 1.1;
    style.spacing.item_spacing *= 1.1;
    ui.set_style(style);
}

// Draw the existing dropdown chevron as geometry, independent of installed fonts.
fn scanner_menu(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    let response = ui.menu_button("From Scanner     ", contents).response;
    let center = egui::pos2(response.rect.right() - 10.0, response.rect.center().y);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-4.0, -2.0),
            center + egui::vec2(4.0, -2.0),
            center + egui::vec2(0.0, 3.0),
        ],
        ui.visuals().text_color(),
        egui::Stroke::NONE,
    ));
}

const DIALOG_WIDTH: f32 = 500.0;
const LABEL_WIDTH: f32 = 96.0;
const COMBO_WIDTH: f32 = 210.0;
const SIDE_BUTTON_WIDTH: f32 = 128.0;
const ROW_HEIGHT: f32 = 22.0;
/// Width of the second Scan dialog layout.
const COMPACT_WIDTH: f32 = 470.0;
/// How far settings that belong to the line above are pushed in.
const INDENT: f32 = 22.0;
/// Label width in the filter list, wide enough for the longest filter name.
const FILTER_LABEL_WIDTH: f32 = 140.0;
/// Choices for "Reduce resolution to"; 0 leaves the resolution alone.
const DOWNSAMPLE_CHOICES: &[u32] = &[0, 100, 150, 200, 300];

/// Scale text, buttons and their hit areas together, independently of scan DPI.
#[cfg(test)]
fn configure_interface(ctx: &Context) {
    ctx.set_visuals(egui::Visuals::light());
    ctx.set_zoom_factor(1.1);
}

/// Keep every dialog reachable at 200% scale, including on smaller displays.
fn show_modal<R>(
    ctx: &Context,
    id: Id,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    let available =
        (ctx.content_rect().size() - egui::vec2(32.0, 32.0)).max(egui::vec2(80.0, 80.0));
    Modal::new(id)
        .frame(egui::Frame::popup(&egui::Style {
            visuals: egui::Visuals::light(),
            ..Default::default()
        }))
        .show(ctx, |ui| {
            configure_controls(ui);
            // An Area remembers its previous size. Give the scroll viewport the
            // current display budget so it can grow after the initial sizing pass.
            ui.set_max_size(available);
            ScrollArea::both()
                .id_salt(id.with("dialog-scroll"))
                .max_width(available.x)
                .max_height(available.y)
                .show(ui, |ui| {
                    // Horizontal scrolling otherwise gives children unlimited width.
                    ui.set_width(DIALOG_WIDTH);
                    contents(ui)
                })
                .inner
        })
}

/// Scroll long forms while leaving their title and action buttons visible.
fn scroll_scan_form<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    let height = (ui.ctx().content_rect().height() - 110.0).max(80.0);
    ScrollArea::both()
        .id_salt("scan-form")
        .max_width(width)
        .max_height(height)
        .show(ui, |ui| {
            ui.set_width(width);
            contents(ui)
        })
        .inner
}

/// Which of the small option windows are open.
#[derive(Default)]
struct OptionWindows {
    /// Holds the choice being edited until OK or Cancel.
    scanner_options: Option<TransferMechanism>,
    /// The tabbed options window, and which tab it shows.
    tabs: Option<OptionsTab>,
    multiple_files: bool,
    /// Holds the name being typed for "Save as Preset".
    save_preset: Option<String>,
    /// Layout 2: the copy of the optimize settings being edited until OK or Cancel.
    optimize: Option<OptimizeOptions>,
    /// Layout 2: scanner settings that do not fit in the dialog itself.
    scanner_details: bool,
    help: bool,
}

/// Tabs of the options window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionsTab {
    ImageCorrections,
    Compression,
    Ocr,
}

/// A preset being created or edited.
struct PresetEditor {
    /// Position in the list when editing; `None` when creating.
    index: Option<usize>,
    name: String,
    settings: ScanSettings,
}

/// Buttons in the settings form that need the caller to do something.
#[derive(Default)]
struct FormRequests {
    scanner_options: bool,
    optimize_options: bool,
    ocr_options: bool,
    multiple_files_options: bool,
    browse_append: bool,
    scanner_details: bool,
    optimize_filters: bool,
}

/// The accepted OpenScan interface, reusable inside any egui host.
pub struct App {
    storage: PathBuf,
    models: PathBuf,
    session: Option<ScanSession>,
    host_events: Vec<OpenScanEvent>,
    host_accept: bool,
    scan_notes: bool,
    initialized: bool,
    standalone: bool,
    injected: Option<Arc<backend::native::NativeBackend>>,
    cancel_token: CancellationToken,
    /// Compatibility export profile. Corrected session images are never cleaned again.
    pub profile: ScanProfile,
    startup_window_frames: u8,
    devices: Vec<Device>,
    presets: PresetStore,
    presets_path: PathBuf,
    /// Settings shown in the Scan dialog.
    settings: ScanSettings,
    ocr_engine: Option<Arc<dyn OcrEngine>>,
    ocr_languages: Vec<String>,

    scan_open: bool,
    presets_open: bool,
    selected_preset: Option<usize>,
    preset_editor: Option<PresetEditor>,
    options: OptionWindows,

    pages: Vec<Page>,
    previews: Vec<TextureHandle>,
    selected_page: usize,

    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,
    /// Description of the job in progress, if any.
    busy: Option<&'static str>,
    ask_more_pages: bool,
    log: Vec<String>,
    status: String,
    error: Option<String>,
}

impl App {
    /// Storage is caller-owned; existing sessions and preset files are preserved.
    pub fn new(storage: PathBuf, models: PathBuf) -> Self {
        let devices = Vec::new();
        let presets_path = storage.join("presets-v3.json");
        let presets = PresetStore::default();
        let settings = ScanSettings::default();
        let (events_tx, events_rx) = channel();
        let ocr_engine = None;
        let ocr_languages = Vec::new();
        let mut app = Self {
            storage,
            models,
            session: None,
            host_events: Vec::new(),
            host_accept: true,
            scan_notes: false,
            initialized: false,
            standalone: false,
            injected: None,
            cancel_token: CancellationToken::default(),
            profile: ScanProfile::default(),
            startup_window_frames: 2,
            devices,
            presets,
            presets_path,
            settings,
            ocr_engine,
            ocr_languages,
            // The Scan dialog is what the program is for: show it straight away.
            scan_open: true,
            presets_open: false,
            selected_preset: None,
            preset_editor: None,
            options: OptionWindows::default(),
            pages: Vec::new(),
            previews: Vec::new(),
            selected_page: 0,
            events_tx,
            events_rx,
            busy: Some("Opening scanner"),
            ask_more_pages: false,
            log: Vec::new(),
            status: "Ready".to_owned(),
            error: None,
        };
        // For development: open a dialog at start-up, e.g. SCAN_MODULE_SHOW=scan.
        match std::env::var("SCAN_MODULE_SHOW").as_deref() {
            Ok("scan") => app.scan_open = true,
            Ok("presets") => {
                app.scan_open = false;
                app.presets_open = true;
            }
            Ok("none") => app.scan_open = false,
            Ok("scanner-options") => {
                app.scan_open = true;
                app.options.scanner_options = Some(app.settings.transfer);
            }
            Ok("corrections") => {
                app.scan_open = true;
                app.options.tabs = Some(OptionsTab::ImageCorrections);
            }
            Ok("ocr") => {
                app.scan_open = true;
                app.options.tabs = Some(OptionsTab::Ocr);
            }
            Ok("scan2") => {
                app.presets.layout = DialogLayout::Compact;
                app.scan_open = true;
            }
            Ok("optimize2") => {
                app.presets.layout = DialogLayout::Compact;
                app.scan_open = true;
                app.options.optimize = Some(app.settings.optimize_opts.clone());
            }
            Ok("scanner2") => {
                app.presets.layout = DialogLayout::Compact;
                app.scan_open = true;
                app.options.scanner_details = true;
            }
            _ => {}
        }
        app
    }

    pub fn with_backend(
        storage: PathBuf,
        models: PathBuf,
        backend: Box<dyn scan_module::ScannerBackend>,
    ) -> Self {
        let mut app = Self::new(storage, models);
        app.injected = Some(Arc::new(backend::native::NativeBackend(backend)));
        app
    }
    /// Standalone hosts call this once. Embedded hosts retain their own window size.
    pub fn standalone(mut self) -> Self {
        self.standalone = true;
        self.host_accept = false;
        self
    }
    /// Done remains the visible label; hosts receive Accepted instead of a save dialog.
    pub fn set_accept_label(&mut self, _label: impl Into<String>) {
        self.host_accept = true;
    }
    pub fn enable_filing_formats(&mut self) {
        self.host_accept = true;
    }
    pub fn enable_scan_notes(&mut self) {
        self.scan_notes = true;
    }
    pub fn filing_format(&self) -> Option<scan_module::ScanFileFormat> {
        Some(scan_module::ScanFileFormat::Pdf)
    }
    pub fn take_events(&mut self) -> Vec<OpenScanEvent> {
        std::mem::take(&mut self.host_events)
    }
    pub fn session(&self) -> Option<&ScanSession> {
        self.session.as_ref()
    }
    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }
    pub fn settings(&self) -> &ScanSettings {
        &self.settings
    }
    pub fn settings_mut(&mut self) -> &mut ScanSettings {
        &mut self.settings
    }
    pub fn import_images(&mut self, paths: Vec<PathBuf>) {
        if self.busy.is_some() {
            return;
        }
        let device = Device {
            backend: FILE_IMPORT_BACKEND,
            id: "import".into(),
            name: "Image files (import)".into(),
        };
        self.start_scan_job(&Context::default(), true, device, paths);
    }
    pub fn open_session(&mut self, directory: PathBuf) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("Opening saved pages");
        self.scan_open = false;
        let events = self.events_tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let session = ScanSession::load(&directory)?;
                let pages = openscan_core::session::pages(&session)?;
                Ok((pages, session))
            })();
            send(
                &events,
                match result {
                    Ok((pages, session)) => Event::Retained(pages, session, false),
                    Err(e) => Event::Failed(format!("{e:#}")),
                },
            );
        });
    }
    fn refresh(&mut self, ctx: &Context, load_presets: bool) {
        self.busy = Some("Finding scanners");
        let events = self.events_tx.clone();
        let ctx = ctx.clone();
        let path = self.presets_path.clone();
        let models = self.models.clone();
        let existing = self.presets.clone();
        let injected = self.injected.clone();
        std::thread::spawn(move || {
            use backend::ScannerBackend;
            let (mut devices, mut problems) = if let Some(backend) = injected {
                match backend.list_devices() {
                    Ok(d) => (d, Vec::new()),
                    Err(e) => (Vec::new(), vec![e.to_string()]),
                }
            } else {
                backend::discover()
            };
            if !devices.iter().any(|d| d.backend == FILE_IMPORT_BACKEND) {
                devices.push(Device {
                    backend: FILE_IMPORT_BACKEND,
                    id: "import".into(),
                    name: "Image files (import)".into(),
                });
            }
            let presets = if load_presets {
                // Read the accepted standalone presets only as a migration fallback.
                // Each host subsequently writes its own v3 file, leaving the source intact.
                let source = if path.exists() {
                    path
                } else {
                    PresetStore::default_path()
                };
                match PresetStore::load(&source) {
                    Ok(p) => p,
                    Err(e) => {
                        problems.push(format!("Could not read presets: {e:#}"));
                        existing
                    }
                }
            } else {
                existing
            };
            let engine = ocr::find(Some(&models));
            let languages = engine.as_ref().map(|e| e.languages()).unwrap_or_default();
            send(
                &events,
                Event::Initialized(devices, problems, presets, engine, languages),
            );
            ctx.request_repaint();
        });
    }
    fn update_profile(&mut self) {
        self.profile.dpi = self.settings.resolution;
        self.profile.searchable_pdf = self.settings.ocr;
        self.profile.jpeg_quality = self.settings.optimize_opts.jpeg_quality.clamp(20, 100);
        self.profile.color = match self.settings.color_mode {
            ColorMode::BlackWhite => scan_module::ScanColor::BlackWhite,
            ColorMode::Grayscale => scan_module::ScanColor::Grayscale,
            _ => scan_module::ScanColor::Color,
        };
    }

    fn device_for(&self, settings: &ScanSettings) -> Option<Device> {
        self.devices
            .iter()
            .find(|device| device.key() == settings.device)
            .cloned()
    }

    /// The holding workspace opens at 65% of the monitor's width and height.
    /// Apply once, after the native backend provides monitor information, so
    /// later manual resizing remains under the user's control.
    fn size_initial_window(&mut self, ctx: &Context) {
        if self.startup_window_frames == 0 {
            return;
        }
        // Native monitor dimensions must first catch up with the initial 110%
        // egui zoom; otherwise resizing would apply that scale a second time.
        if self.startup_window_frames == 2 {
            self.startup_window_frames = 1;
            ctx.request_repaint();
            return;
        }
        if let Some(monitor) = ctx.input(|input| input.viewport().monitor_size)
            && monitor.is_finite()
            && monitor.x > 0.0
            && monitor.y > 0.0
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(monitor * 0.65));
            self.startup_window_frames = 0;
        }
    }

    fn save_presets(&mut self) {
        if let Err(error) = self.presets.save(&self.presets_path) {
            self.error = Some(format!("Could not save presets: {error:#}"));
        }
    }

    /// Starts a scan with the current settings. `keep_pages` adds to the pages already held.
    fn start_scan(&mut self, ctx: &Context, keep_pages: bool) {
        if self.busy.is_some() {
            return;
        }
        let Some(device) = self.device_for(&self.settings) else {
            self.error = Some("Choose a scanner first.".to_owned());
            return;
        };
        let input_files = if device.backend == FILE_IMPORT_BACKEND {
            let chosen = rfd::FileDialog::new()
                .set_title("Choose page images")
                .add_filter("Images", &["png", "jpg", "jpeg", "tif", "tiff", "bmp"])
                .pick_files();
            match chosen {
                Some(files) => files,
                None => return,
            }
        } else {
            Vec::new()
        };
        self.start_scan_job(ctx, keep_pages, device, input_files);
    }

    /// Shared worker entry point for acquisition and image import.
    fn start_scan_job(
        &mut self,
        ctx: &Context,
        keep_pages: bool,
        device: Device,
        input_files: Vec<PathBuf>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.scan_open = false;
        self.busy = Some("Scanning");
        self.status = "Scanning...".to_owned();

        self.error = None;
        self.cancel_token = CancellationToken::default();
        let previous = if keep_pages {
            self.session.clone()
        } else {
            None
        };
        let storage = self.storage.clone();
        let injected = self.injected.clone();
        let settings = self.settings.clone();
        let events = self.events_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let log = |message: String| {
                send(&events, Event::Log(message));
                ctx.request_repaint();
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<_> {
                    let pages =
                        if let Some(injected) = injected.filter(|_| device.backend == "Native") {
                            pipeline::scan_with_backend(
                                injected.as_ref(),
                                &device,
                                &settings,
                                &input_files,
                                &log,
                            )?
                        } else {
                            pipeline::scan_and_clean(&device, &settings, &input_files, &log)?
                        };
                    let session =
                        openscan_core::session::append(&storage, previous.as_ref(), &pages)?;
                    openscan_core::session::write_settings(&session, &settings)?;
                    Ok((pages, session))
                }));
            let event = match result {
                Ok(Ok((pages, session))) => Event::Retained(pages, session, keep_pages),
                Ok(Err(error)) => Event::Failed(format!("{error:#}")),
                Err(_) => Event::Failed(
                    "Scanner processing stopped unexpectedly; saved pages are retained.".into(),
                ),
            };
            send(&events, event);
            ctx.request_repaint();
        });
    }

    /// Asks where to save if needed, then writes the pages held.
    fn start_save(&mut self, ctx: &Context) {
        if self.busy.is_some() || self.pages.is_empty() {
            return;
        }
        if self.host_accept && self.settings.output == OutputMode::NewPdf {
            self.update_profile();
            if let Some(session) = &self.session {
                let session = session.clone();
                let settings = self.settings.clone();
                let events = self.events_tx.clone();
                let ctx = ctx.clone();
                self.busy = Some("Preparing document");
                std::thread::spawn(move || {
                    let event = match openscan_core::session::write_settings(&session, &settings) {
                        Ok(()) => Event::Accepted(session),
                        Err(e) => Event::Failed(format!("{e:#}")),
                    };
                    send(&events, event);
                    ctx.request_repaint();
                });
            }
            return;
        }
        let target = match self.settings.output {
            OutputMode::NewPdf => {
                let chosen = rfd::FileDialog::new()
                    .set_title("Save scanned document")
                    .add_filter("PDF", &["pdf"])
                    .set_file_name("Scan.pdf")
                    .save_file();
                match chosen {
                    Some(path) => Target::NewPdf(path.with_extension("pdf")),
                    None => {
                        self.status = "Not saved. The pages are still here; use Done.".to_owned();
                        return;
                    }
                }
            }
            OutputMode::Append => match self.settings.append_path.clone() {
                Some(path) => Target::Append(path),
                None => {
                    self.error = Some("Choose the PDF to append to with Browse.".to_owned());
                    return;
                }
            },
            OutputMode::MultipleFiles => {
                let folder = self.settings.multi.folder.clone().or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Folder for the scanned files")
                        .pick_folder()
                });
                match folder {
                    Some(folder) => Target::MultipleFiles {
                        folder,
                        prefix: self.settings.multi.prefix.clone(),
                        pages_per_file: self.settings.multi.pages_per_file.max(1) as usize,
                    },
                    None => {
                        self.status = "Not saved. The pages are still here; use Done.".to_owned();
                        return;
                    }
                }
            }
        };
        self.start_save_job(ctx, target);
    }

    fn start_save_job(&mut self, ctx: &Context, target: Target) {
        if self.busy.is_some() || self.pages.is_empty() {
            return;
        }
        self.busy = Some("Saving");
        self.status = "Saving...".to_owned();

        let pages = self.pages.clone();
        let settings = self.settings.clone();
        let engine = self.ocr_engine.clone();
        let events = self.events_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let log = |message: String| {
                send(&events, Event::Log(message));
                ctx.request_repaint();
            };
            let engine = engine.as_deref().map(|engine| engine as &dyn OcrEngine);
            let event = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pipeline::save(&pages, &settings, &target, engine, &log)
            })) {
                Ok(Ok(files)) => Event::Saved(files),
                Ok(Err(error)) => Event::Failed(format!("{error:#}")),
                Err(_) => Event::Failed(
                    "PDF processing stopped unexpectedly; saved pages are retained.".into(),
                ),
            };
            send(&events, event);
            ctx.request_repaint();
        });
    }

    /// Handles everything the worker threads have reported since the last frame.
    fn read_events(&mut self, ctx: &Context) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                Event::Log(message) => self.log.push(message),
                Event::Initialized(devices, problems, presets, engine, languages) => {
                    self.devices = devices;
                    self.log.extend(problems);
                    self.presets = presets;
                    self.ocr_engine = engine;
                    self.ocr_languages = languages;
                    if self.device_for(&self.settings).is_none()
                        && let Some(device) = self.devices.first()
                    {
                        self.settings.device = device.key();
                    }
                    self.busy = None;
                }
                Event::Retained(pages, session, keep) => {
                    if !keep {
                        self.pages.clear();
                        self.previews.clear();
                        self.selected_page = 0;
                    }
                    self.session = Some(session);
                    self.receive_pages(ctx, pages);
                }
                Event::Accepted(session) => {
                    self.busy = None;
                    self.host_events.push(OpenScanEvent::Accepted(session));
                }
                Event::Saved(files) => {
                    self.busy = None;
                    let names: Vec<String> = files
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect();
                    self.host_events
                        .extend(files.into_iter().map(OpenScanEvent::Exported));
                    self.status = format!("Saved {}", names.join(", "));
                    self.log.push(self.status.clone());
                }
                Event::Failed(message) => {
                    self.busy = None;
                    self.status = "Failed".to_owned();
                    self.log.push(format!("Error: {message}"));
                    self.error = Some(message);
                }
            }
        }
    }

    fn receive_pages(&mut self, ctx: &Context, pages: Vec<Page>) {
        self.busy = None;
        for page in pages {
            self.previews
                .push(preview_texture(ctx, &page, self.previews.len()));
            self.pages.push(page);
        }
        self.status = format!("{} page(s) ready", self.pages.len());
        if self.settings.prompt_more_pages {
            self.ask_more_pages = true;
        }
    }

    fn main_window(&mut self, ui: &mut Ui) {
        let ctx = &ui.ctx().clone();
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let idle = self.busy.is_none();
                ui.add_enabled_ui(idle, |ui| {
                    scanner_menu(ui, |ui| {
                        if ui.button("Scan...").clicked() {
                            self.scan_open = true;
                            ui.close();
                        }
                        if !self.presets.presets.is_empty() {
                            ui.separator();
                        }
                        let mut chosen = None;
                        for (index, preset) in self.presets.presets.iter().enumerate() {
                            if ui.button(&preset.name).clicked() {
                                chosen = Some(index);
                                ui.close();
                            }
                        }
                        if let Some(index) = chosen {
                            self.settings = self.presets.presets[index].settings.clone();
                            self.start_scan(ctx, false);
                        }
                        ui.separator();
                        if ui.button("Open saved pages...").clicked() {
                            if let Some(directory) = rfd::FileDialog::new().set_title("Choose an OpenScan session folder").set_directory(self.storage.join("Sessions")).pick_folder() {
                                self.open_session(directory);
                            }
                            ui.close();
                        }
                        if ui.button("Manage Presets...").clicked() {
                            self.presets_open = true;
                            ui.close();
                        }
                        ui.separator();
                        let before = self.presets.layout;
                        for &layout in DialogLayout::ALL {
                            let text = format!("Scan dialog: {}", layout.label());
                            if ui
                                .radio_value(&mut self.presets.layout, layout, text)
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                        if self.presets.layout != before {
                            self.save_presets();
                        }
                    });
                    if ui
                        .add_enabled(
                            !self.pages.is_empty() && self.device_for(&self.settings).is_some(),
                            Button::new("Scan more Pages"),
                        )
                        .on_hover_text("Scan another batch using the current settings and add it to these pages")
                        .clicked()
                    {
                        self.start_scan(ctx, true);
                    }
                    if ui
                        .add_enabled(!self.pages.is_empty(), Button::new("Done"))
                        .on_hover_text("Save all pages in the current document")
                        .clicked()
                    {
                        self.start_save(ctx);
                    }
                    if self.scan_notes && ui.add_enabled(!self.pages.is_empty(), Button::new("Scan Note")).clicked()
                        && let Some(session) = &self.session { self.host_events.push(OpenScanEvent::ScanNote(session.clone())); }
                    if ui.button("Refresh Scanners").clicked() {
                        self.refresh(ctx, false);
                    }
                });
                ui.separator();
                ui.label(&self.status);
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(90.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Activity").strong());
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(line);
                        }
                    });
            });

        egui::Panel::left("pages")
            .default_size(150.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Pages").strong());
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, texture) in self.previews.iter().enumerate() {
                            let image =
                                egui::Image::new(texture).max_size(egui::vec2(120.0, 160.0));
                            let button =
                                egui::Button::image(image).selected(index == self.selected_page);
                            if ui.add(button).clicked() {
                                self.selected_page = index;
                            }
                            ui.label(format!("Page {}", index + 1));
                            ui.add_space(6.0);
                        }
                    });
            });

        egui::CentralPanel::default().show(ui, |ui| match self.previews.get(self.selected_page) {
            Some(texture) => {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(egui::Image::new(texture).max_size(ui.available_size()));
                    });
            }
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("No pages yet. Choose From Scanner, then Scan.");
                });
            }
        });
    }

    fn scan_dialog(&mut self, ctx: &Context) {
        if !self.scan_open || self.preset_editor.is_some() {
            return;
        }
        if self.presets.layout == DialogLayout::Compact {
            self.compact_scan_dialog(ctx);
            return;
        }
        let mut requests = FormRequests::default();
        let mut close = false;
        let mut scan = false;
        show_modal(ctx, Id::unique("scan_dialog"), |ui| {
            ui.set_width(DIALOG_WIDTH);
            close |= title_bar(ui, "Scan");
            scroll_scan_form(ui, |ui| {
                settings_form(
                    ui,
                    &mut self.settings,
                    &self.devices,
                    self.ocr_engine.is_some(),
                    &mut requests,
                );
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(Button::new("Manage Presets...").min_size(egui::vec2(120.0, 0.0)))
                    .clicked()
                {
                    self.presets_open = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let size = egui::vec2(100.0, 0.0);
                    close |= ui.add(Button::new("Cancel").min_size(size)).clicked();
                    scan = ui.add(Button::new("Scan").min_size(size)).clicked();
                    if ui
                        .add(Button::new("Save as Preset").min_size(size))
                        .clicked()
                    {
                        self.options.save_preset = Some(String::new());
                    }
                });
            });
        });
        // Escape closes this dialog only when no window is open above it.
        let top_most = !self.presets_open && !self.any_option_window();
        if close || (top_most && escape_pressed(ctx)) {
            self.scan_open = false;
        }
        self.apply_form_requests(requests, false);
        if scan {
            self.start_scan(ctx, false);
        }
    }

    /// The second Scan dialog layout: Input, Output and Document Settings.
    fn compact_scan_dialog(&mut self, ctx: &Context) {
        let mut requests = FormRequests::default();
        let mut close = false;
        let mut scan = false;
        let mut defaults = false;
        show_modal(ctx, Id::unique("scan_dialog_compact"), |ui| {
            ui.set_width(COMPACT_WIDTH);
            close |= title_bar(ui, "Scan");
            scroll_scan_form(ui, |ui| {
                compact_form(
                    ui,
                    &mut self.settings,
                    &self.devices,
                    self.ocr_engine.is_some(),
                    &mut requests,
                );
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let size = egui::vec2(90.0, 0.0);
                self.options.help |= ui.add(Button::new("Help").min_size(size)).clicked();
                defaults = ui.add(Button::new("Defaults").min_size(size)).clicked();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    scan = ui.add(Button::new("Scan").min_size(size)).clicked();
                    close |= ui.add(Button::new("Cancel").min_size(size)).clicked();
                });
            });
        });
        if defaults {
            // Keep the chosen scanner; everything else returns to how it started.
            let device = std::mem::take(&mut self.settings.device);
            self.settings = ScanSettings {
                device,
                ..Default::default()
            };
        }
        let top_most = !self.presets_open && !self.any_option_window();
        if close || (top_most && escape_pressed(ctx)) {
            self.scan_open = false;
        }
        self.apply_form_requests(requests, false);
        if scan {
            self.start_scan(ctx, false);
        }
    }

    fn any_option_window(&self) -> bool {
        let options = &self.options;
        options.scanner_options.is_some()
            || options.tabs.is_some()
            || options.multiple_files
            || options.save_preset.is_some()
            || options.optimize.is_some()
            || options.scanner_details
            || options.help
    }

    /// Opens whatever the settings form asked for. `for_editor` says which settings it was showing.
    fn apply_form_requests(&mut self, requests: FormRequests, for_editor: bool) {
        let settings = match (&mut self.preset_editor, for_editor) {
            (Some(editor), true) => &mut editor.settings,
            _ => &mut self.settings,
        };
        if requests.scanner_options {
            self.options.scanner_options = Some(settings.transfer);
        }
        if requests.optimize_options {
            self.options.tabs = Some(OptionsTab::Compression);
        }
        if requests.ocr_options {
            self.options.tabs = Some(OptionsTab::Ocr);
        }
        self.options.multiple_files |= requests.multiple_files_options;
        self.options.scanner_details |= requests.scanner_details;
        if requests.optimize_filters {
            self.options.optimize = Some(settings.optimize_opts.clone());
        }
        if requests.browse_append {
            let chosen = rfd::FileDialog::new()
                .set_title("PDF to append to")
                .add_filter("PDF", &["pdf"])
                .pick_file();
            if let Some(path) = chosen {
                settings.append_path = Some(path);
            }
        }
    }

    fn presets_dialog(&mut self, ctx: &Context) {
        if !self.presets_open || self.preset_editor.is_some() {
            return;
        }
        let mut close = false;
        show_modal(ctx, Id::unique("presets_dialog"), |ui| {
            ui.set_width(450.0);
            close |= title_bar(ui, "Manage Scan Presets");
            ui.label("Scan Presets");
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().extreme_bg_color)
                    .show(ui, |ui| {
                        ui.set_width(310.0);
                        ui.set_height(200.0);
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (index, preset) in self.presets.presets.iter().enumerate() {
                                    let selected = self.selected_preset == Some(index);
                                    if ui.selectable_label(selected, &preset.name).clicked() {
                                        self.selected_preset = Some(index);
                                    }
                                }
                            });
                    });
                ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
                    ui.set_width(100.0);
                    let size = egui::vec2(100.0, 0.0);
                    let selected = self
                        .selected_preset
                        .filter(|&index| index < self.presets.presets.len());
                    let has_selection = selected.is_some();
                    if ui
                        .add_enabled(has_selection, Button::new("Edit").min_size(size))
                        .clicked()
                        && let Some(index) = selected
                    {
                        let preset = &self.presets.presets[index];
                        self.preset_editor = Some(PresetEditor {
                            index: Some(index),
                            name: preset.name.clone(),
                            settings: preset.settings.clone(),
                        });
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(has_selection, Button::new("Delete").min_size(size))
                        .clicked()
                        && let Some(index) = selected
                    {
                        self.presets.presets.remove(index);
                        self.selected_preset = None;
                        self.save_presets();
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(has_selection, Button::new("Move Up").min_size(size))
                        .clicked()
                        && let Some(index) = selected
                    {
                        self.selected_preset = Some(self.presets.move_up(index));
                        self.save_presets();
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(has_selection, Button::new("Move Down").min_size(size))
                        .clicked()
                        && let Some(index) = selected
                    {
                        self.selected_preset = Some(self.presets.move_down(index));
                        self.save_presets();
                    }
                    ui.add_space(28.0);
                    if ui.add(Button::new("Create").min_size(size)).clicked() {
                        let mut settings = ScanSettings::default();
                        settings.device = self.settings.device.clone();
                        self.preset_editor = Some(PresetEditor {
                            index: None,
                            name: String::new(),
                            settings,
                        });
                    }
                    ui.add_space(4.0);
                    close |= ui.add(Button::new("Close").min_size(size)).clicked();
                });
            });
        });
        if close || escape_pressed(ctx) {
            self.presets_open = false;
        }
    }

    fn preset_editor_dialog(&mut self, ctx: &Context) {
        let Some(editor) = &mut self.preset_editor else {
            return;
        };
        let mut requests = FormRequests::default();
        let mut close = false;
        let mut save = false;
        let title = if editor.index.is_some() {
            "Edit Scan Preset"
        } else {
            "Create Scan Preset"
        };
        show_modal(ctx, Id::unique("preset_editor"), |ui| {
            ui.set_width(DIALOG_WIDTH);
            close |= title_bar(ui, title);
            scroll_scan_form(ui, |ui| {
                ui.horizontal(|ui| {
                    label_cell(ui, "Preset name:");
                    ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(COMBO_WIDTH));
                });
                ui.add_space(4.0);
                settings_form(
                    ui,
                    &mut editor.settings,
                    &self.devices,
                    self.ocr_engine.is_some(),
                    &mut requests,
                );
            });
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let size = egui::vec2(100.0, 0.0);
                close |= ui.add(Button::new("Cancel").min_size(size)).clicked();
                let named = !editor.name.trim().is_empty();
                save = ui
                    .add_enabled(named, Button::new("Save").min_size(size))
                    .clicked();
            });
        });
        if save {
            if let Some(editor) = self.preset_editor.take() {
                let preset = Preset {
                    name: editor.name.trim().to_owned(),
                    settings: editor.settings,
                };
                match editor.index {
                    Some(index) if index < self.presets.presets.len() => {
                        self.presets.presets[index] = preset
                    }
                    _ => self.presets.upsert(preset),
                }
                self.save_presets();
            }
            return;
        }
        if close || (!self.any_option_window() && escape_pressed(ctx)) {
            self.preset_editor = None;
            return;
        }
        self.apply_form_requests(requests, true);
    }

    fn option_dialogs(&mut self, ctx: &Context) {
        // These windows edit whichever settings the form underneath them is showing.
        let settings = match &mut self.preset_editor {
            Some(editor) => &mut editor.settings,
            None => &mut self.settings,
        };

        if let Some(choice) = &mut self.options.scanner_options {
            let mut ok = false;
            let mut close = false;
            show_modal(ctx, Id::unique("scanner_options"), |ui| {
                ui.set_width(320.0);
                close |= title_bar(ui, "Scanner Options");
                ui.horizontal(|ui| {
                    ui.label("Transfer Mechanism:");
                    ComboBox::from_id_salt("transfer")
                        .width(180.0)
                        .selected_text(choice.label())
                        .show_ui(ui, |ui| {
                            for &option in TransferMechanism::ALL {
                                ui.selectable_value(choice, option, option.label());
                            }
                        })
                        .response
                        .accessible_name("Transfer mechanism");
                });
                ui.add_space(8.0);
                (ok, close) = ok_cancel(ui, close);
            });
            if ok {
                settings.transfer = *choice;
            }
            if ok || close || escape_pressed(ctx) {
                self.options.scanner_options = None;
            }
        }

        if let Some(tab) = &mut self.options.tabs {
            let mut close = false;
            let engine_name = self.ocr_engine.as_ref().map(|engine| engine.name());
            show_modal(ctx, Id::unique("scan_options"), |ui| {
                ui.set_width(420.0);
                ui.set_min_height(250.0);
                close |= title_bar(ui, "Scan Options");
                ui.horizontal(|ui| {
                    ui.selectable_value(tab, OptionsTab::ImageCorrections, "Image Corrections");
                    ui.selectable_value(tab, OptionsTab::Compression, "Compression");
                    ui.selectable_value(tab, OptionsTab::Ocr, "OCR");
                });
                ui.separator();
                ui.add_space(4.0);
                match *tab {
                    OptionsTab::ImageCorrections => {
                        let options = &mut settings.optimize_opts;
                        ui.label("Each correction can be switched on or off:");
                        ui.add_space(4.0);
                        ui.checkbox(&mut options.deskew, "Deskew").on_hover_text(
                            "Straighten pages that went through the scanner at an angle",
                        );
                        ui.checkbox(&mut options.despeckle, "Despeckle")
                            .on_hover_text(
                                "Remove stray dots that are too small to be part of a letter",
                            );
                        level_checkbox(
                            ui,
                            &mut options.background_removal,
                            "Remove wrinkle and crease shadows",
                        )
                        .on_hover_text(
                            "Even out shading from wrinkles, creases and uneven lighting",
                        );
                        ui.checkbox(&mut options.descreen, "Descreen")
                            .on_hover_text("Smooth away the dot pattern of printed photographs");
                        level_checkbox(ui, &mut options.text_sharpening, "Sharpen text")
                            .on_hover_text("Make the edges of letters crisper");
                        ui.checkbox(&mut options.crop_borders, "Crop")
                            .on_hover_text("Trim the black scanner background from the page edges");
                        ui.checkbox(&mut options.remove_blank_pages, "Remove blank pages")
                            .on_hover_text("Leave out pages with nothing on them");
                    }
                    OptionsTab::Compression => {
                        ui.checkbox(&mut settings.optimize, "Reduce size and optimize");
                        ui.add_space(4.0);
                        let options = &mut settings.optimize_opts;
                        ui.add_enabled_ui(settings.optimize, |ui| {
                            ui.add(
                                egui::Slider::new(&mut options.jpeg_quality, 10..=100)
                                    .text("Image quality"),
                            );
                            ui.horizontal(|ui| {
                                let mut reduce = options.downsample_dpi > 0;
                                if ui.checkbox(&mut reduce, "Reduce resolution to").changed() {
                                    options.downsample_dpi = if reduce { 150 } else { 0 };
                                }
                                if reduce {
                                    ui.add(
                                        egui::DragValue::new(&mut options.downsample_dpi)
                                            .range(50..=600)
                                            .suffix(" dpi"),
                                    );
                                } else {
                                    // Show a placeholder so the stored "off" value of 0 is not altered.
                                    let mut placeholder = 150;
                                    ui.add_enabled(
                                        false,
                                        egui::DragValue::new(&mut placeholder).suffix(" dpi"),
                                    );
                                }
                            });
                        });
                    }
                    OptionsTab::Ocr => {
                        let available = engine_name.is_some();
                        ui.add_enabled(
                            available,
                            egui::Checkbox::new(&mut settings.ocr, "Make Searchable (run OCR)"),
                        );
                        ui.label(format!(
                            "Engine: {}",
                            engine_name
                                .as_deref()
                                .unwrap_or("Tesseract (not installed)")
                        ));
                        ui.add_space(4.0);
                        let options = &mut settings.ocr_opts;
                        ui.add_enabled_ui(available && settings.ocr, |ui| {
                            ui.horizontal(|ui| {
                                label_cell(ui, "Language:");
                                ComboBox::from_id_salt("ocr_language")
                                    .width(160.0)
                                    .selected_text(&options.language)
                                    .show_ui(ui, |ui| {
                                        for language in &self.ocr_languages {
                                            ui.selectable_value(
                                                &mut options.language,
                                                language.clone(),
                                                language,
                                            );
                                        }
                                    })
                                    .response
                                    .accessible_name("OCR language");
                            });
                            ui.horizontal(|ui| {
                                label_cell(ui, "Page layout:");
                                let layouts = [
                                    (3u8, "Automatic"),
                                    (4, "Single column"),
                                    (6, "Single block of text"),
                                    (11, "Sparse text"),
                                ];
                                let current = layouts
                                    .iter()
                                    .find(|(mode, _)| *mode == options.page_seg_mode)
                                    .map_or("Automatic", |(_, name)| name);
                                ComboBox::from_id_salt("ocr_layout")
                                    .width(160.0)
                                    .selected_text(current)
                                    .show_ui(ui, |ui| {
                                        for (mode, name) in layouts {
                                            ui.selectable_value(
                                                &mut options.page_seg_mode,
                                                mode,
                                                name,
                                            );
                                        }
                                    })
                                    .response
                                    .accessible_name("OCR page layout");
                            });
                        });
                    }
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui
                        .add(Button::new("Close").min_size(egui::vec2(90.0, 0.0)))
                        .clicked();
                });
            });
            if close || escape_pressed(ctx) {
                self.options.tabs = None;
            }
        }

        if self.options.multiple_files {
            let mut close = false;
            show_modal(ctx, Id::unique("multiple_files_options"), |ui| {
                ui.set_width(420.0);
                close |= title_bar(ui, "Save Multiple Files");
                let options = &mut settings.multi;
                ui.horizontal(|ui| {
                    label_cell(ui, "Folder:");
                    let shown = options
                        .folder
                        .as_ref()
                        .map_or("(ask when saving)".to_owned(), |path| {
                            path.display().to_string()
                        });
                    ui.add(egui::Label::new(shown).truncate());
                });
                ui.horizontal(|ui| {
                    label_cell(ui, "");
                    if ui.button("Browse...").clicked()
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        options.folder = Some(folder);
                    }
                    if ui.button("Clear").clicked() {
                        options.folder = None;
                    }
                });
                ui.horizontal(|ui| {
                    label_cell(ui, "File name:");
                    ui.add(egui::TextEdit::singleline(&mut options.prefix).desired_width(140.0));
                    ui.label("_001.pdf");
                });
                ui.horizontal(|ui| {
                    label_cell(ui, "Pages per file:");
                    ui.add(egui::DragValue::new(&mut options.pages_per_file).range(1..=999));
                });
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui
                        .add(Button::new("Close").min_size(egui::vec2(90.0, 0.0)))
                        .clicked();
                });
            });
            if close || escape_pressed(ctx) {
                self.options.multiple_files = false;
            }
        }

        if let Some(draft) = &mut self.options.optimize {
            let mut ok = false;
            let mut close = false;
            let mut defaults = false;
            show_modal(ctx, Id::unique("optimize_scanned_pdf"), |ui| {
                ui.set_width(400.0);
                close |= title_bar(ui, "Optimize Scanned PDF");
                optimize_form(ui, draft);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let size = egui::vec2(90.0, 0.0);
                    defaults = ui.add(Button::new("Defaults").min_size(size)).clicked();
                    (ok, close) = ok_cancel(ui, close);
                });
            });
            if defaults {
                *draft = OptimizeOptions::default();
            }
            if ok {
                settings.optimize_opts = draft.clone();
            }
            if ok || close || escape_pressed(ctx) {
                self.options.optimize = None;
            }
        }

        if self.options.scanner_details {
            let mut close = false;
            show_modal(ctx, Id::unique("scanner_details"), |ui| {
                ui.set_width(360.0);
                close |= title_bar(ui, "Scanner Options");
                scanner_details_form(ui, settings);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui
                        .add(Button::new("Close").min_size(egui::vec2(90.0, 0.0)))
                        .clicked();
                });
            });
            if close || escape_pressed(ctx) {
                self.options.scanner_details = false;
            }
        }

        if self.options.help {
            let mut close = false;
            show_modal(ctx, Id::unique("scan_help"), |ui| {
                ui.set_width(420.0);
                close |= title_bar(ui, "Scan Help");
                for (heading, text) in HELP {
                    ui.label(RichText::new(*heading).strong());
                    ui.label(*text);
                    ui.add_space(6.0);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui
                        .add(Button::new("Close").min_size(egui::vec2(90.0, 0.0)))
                        .clicked();
                });
            });
            if close || escape_pressed(ctx) {
                self.options.help = false;
            }
        }

        if let Some(name) = &mut self.options.save_preset {
            let mut ok = false;
            let mut close = false;
            show_modal(ctx, Id::unique("save_preset"), |ui| {
                ui.set_width(320.0);
                close |= title_bar(ui, "Save as Preset");
                ui.horizontal(|ui| {
                    ui.label("Preset name:");
                    ui.add(egui::TextEdit::singleline(name).desired_width(200.0))
                        .request_focus();
                });
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let size = egui::vec2(90.0, 0.0);
                    close |= ui.add(Button::new("Cancel").min_size(size)).clicked();
                    let named = !name.trim().is_empty();
                    ok = ui
                        .add_enabled(named, Button::new("OK").min_size(size))
                        .clicked();
                });
            });
            if ok {
                let preset = Preset {
                    name: name.trim().to_owned(),
                    settings: settings.clone(),
                };
                self.presets.upsert(preset);
                if let Err(error) = self.presets.save(&self.presets_path) {
                    self.error = Some(format!("Could not save presets: {error:#}"));
                }
            }
            if ok || close || escape_pressed(ctx) {
                self.options.save_preset = None;
            }
        }
    }

    fn status_dialogs(&mut self, ctx: &Context) {
        if let Some(job) = self.busy {
            show_modal(ctx, Id::unique("busy"), |ui| {
                ui.set_width(360.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("{job}..."));
                });
                if let Some(last) = self.log.last() {
                    ui.label(RichText::new(last).weak());
                }
            });
        }

        if self.ask_more_pages && self.busy.is_none() {
            let mut more = false;
            let mut finish = false;
            show_modal(ctx, Id::unique("more_pages"), |ui| {
                ui.set_width(340.0);
                title_bar(ui, "Scan");
                ui.label(format!("{} page(s) scanned so far.", self.pages.len()));
                ui.label("Put the next pages in the scanner to continue, or finish.");
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    finish = ui
                        .add(Button::new("Done").min_size(egui::vec2(120.0, 0.0)))
                        .clicked();
                    more = ui
                        .add(Button::new("Scan more Pages").min_size(egui::vec2(120.0, 0.0)))
                        .clicked();
                });
            });
            if more {
                self.ask_more_pages = false;
                self.start_scan(ctx, true);
            } else if finish {
                self.ask_more_pages = false;
                self.start_save(ctx);
            }
        }

        if let Some(message) = self.error.clone() {
            let mut close = false;
            show_modal(ctx, Id::unique("error"), |ui| {
                ui.set_width(420.0);
                close |= title_bar(ui, "Scan Module");
                ui.label(&message);
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close |= ui
                        .add(Button::new("OK").min_size(egui::vec2(90.0, 0.0)))
                        .clicked();
                });
            });
            if close || escape_pressed(ctx) {
                self.error = None;
            }
        }
    }
}

impl App {
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.initialized {
            self.initialized = true;
            self.refresh(&ctx, true);
        }
        if self.standalone {
            self.size_initial_window(&ctx);
        }
        self.read_events(&ctx);
        if self.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        ui.scope(|ui| {
            configure_controls(ui);
            self.main_window(ui);
        });
        self.scan_dialog(&ctx);
        self.presets_dialog(&ctx);
        self.preset_editor_dialog(&ctx);
        self.option_dialogs(&ctx);
        self.status_dialogs(&ctx);
        self.update_profile();
    }
}

/// The settings shared by the Scan dialog and the preset editor.
fn settings_form(
    ui: &mut Ui,
    settings: &mut ScanSettings,
    devices: &[Device],
    ocr_available: bool,
    requests: &mut FormRequests,
) {
    ui.horizontal(|ui| {
        label_cell(ui, "Scanner:");
        let current = devices
            .iter()
            .find(|device| device.key() == settings.device);
        let shown = current.map_or("(no scanner selected)", |device| device.name.as_str());
        ComboBox::from_id_salt("scanner")
            .width(COMBO_WIDTH)
            .truncate()
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for device in devices {
                    ui.selectable_value(&mut settings.device, device.key(), &device.name);
                }
            })
            .response
            .accessible_name("Scanner");
        requests.scanner_options |= side_button(ui, "Advanced Options...", true);
    });

    group_box(ui, "Configure scanner", |ui| {
        ui.checkbox(
            &mut settings.use_native_ui,
            "Configure scanner using scanner's interface",
        );
        ui.checkbox(&mut settings.prompt_more_pages, "Prompt to scan more pages");
        // With the scanner's own window in charge, the choices below belong to it.
        let own = !settings.use_native_ui;
        combo_row(
            ui,
            "Sides:",
            "sides",
            &mut settings.sides,
            Sides::ALL,
            |v| v.label().to_owned(),
            true,
        );
        combo_row(
            ui,
            "Source:",
            "source",
            &mut settings.source,
            Source::ALL,
            |v| v.label().to_owned(),
            own,
        );
        combo_row(
            ui,
            "Color Mode:",
            "color",
            &mut settings.color_mode,
            ColorMode::ALL,
            |v| v.label().to_owned(),
            own,
        );
        combo_row(
            ui,
            "Paper Size:",
            "paper",
            &mut settings.paper_size,
            PaperSize::ALL,
            |v| v.label().to_owned(),
            own,
        );
        combo_row(
            ui,
            "Resolution:",
            "dpi",
            &mut settings.resolution,
            RESOLUTIONS,
            |v| format!("{v} dpi"),
            own,
        );
    });

    group_box(ui, "Output", |ui| {
        ui.radio_value(
            &mut settings.output,
            OutputMode::NewPdf,
            OutputMode::NewPdf.label(),
        );
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut settings.output,
                OutputMode::Append,
                OutputMode::Append.label(),
            );
            let appending = settings.output == OutputMode::Append;
            let shown = settings
                .append_path
                .as_ref()
                .and_then(|path| path.file_name())
                .map_or(String::new(), |name| name.to_string_lossy().into_owned());
            ui.add_enabled_ui(appending, |ui| {
                let mut text = shown;
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(200.0)
                        .interactive(false),
                );
            });
            requests.browse_append |= side_button(ui, "Browse...", appending);
        });
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut settings.output,
                OutputMode::MultipleFiles,
                OutputMode::MultipleFiles.label(),
            );
            let enabled = settings.output == OutputMode::MultipleFiles;
            requests.multiple_files_options |= side_button(ui, "Options...", enabled);
        });
    });

    group_box(ui, "Optimize Scanned PDF", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut settings.optimize, "Reduce size and optimize");
            requests.optimize_options |= side_button(ui, "Options...", true);
        });
        ui.horizontal(|ui| {
            ui.add_enabled(
                ocr_available,
                egui::Checkbox::new(&mut settings.ocr, "Make Searchable (run OCR)"),
            );
            requests.ocr_options |= side_button(ui, "Options...", true);
        });
        if !ocr_available {
            ui.label(RichText::new("Install Tesseract or OCR models to enable OCR.").weak());
        }
    });
}

/// What the Help button explains.
const HELP: &[(&str, &str)] = &[
    (
        "Input",
        "Choose the scanner and whether to scan one or both sides. Options has the paper \
         source, colour, paper size and resolution.",
    ),
    (
        "Output",
        "Make one new PDF, split the pages into several files, or add the pages to the end \
         of a PDF you already have.",
    ),
    (
        "Document Settings",
        "Optimize makes the file smaller and cleans the pages; slide towards Small Size or \
         High Quality. Make Searchable reads the text so you can search and copy it.",
    ),
];

/// The settings shown by the second Scan dialog layout.
fn compact_form(
    ui: &mut Ui,
    settings: &mut ScanSettings,
    devices: &[Device],
    ocr_available: bool,
    requests: &mut FormRequests,
) {
    group_box(ui, "Input", |ui| {
        ui.horizontal(|ui| {
            label_cell(ui, "Scanner:");
            scanner_combo(ui, "scanner_compact", settings, devices);
            requests.scanner_details |= small_side_button(ui, "Options...", true);
        });
        combo_row(
            ui,
            "Sides:",
            "sides_compact",
            &mut settings.sides,
            Sides::ALL,
            |v| v.label().to_owned(),
            true,
        );
        ui.checkbox(
            &mut settings.prompt_more_pages,
            "Prompt for scanning more pages",
        );
    });

    group_box(ui, "Output", |ui| compact_output(ui, settings, requests));

    group_box(ui, "Document Settings", |ui| {
        ui.checkbox(&mut settings.optimize, "Optimize Scanned PDF");
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.add_enabled_ui(settings.optimize, |ui| {
                quality_slider(ui, &mut settings.optimize_opts.jpeg_quality);
            });
            requests.optimize_filters |= small_side_button(ui, "Options...", true);
        });
        ui.horizontal(|ui| {
            ui.add_enabled(
                ocr_available,
                egui::Checkbox::new(&mut settings.ocr, "Make Searchable (Run OCR)"),
            );
            requests.ocr_options |= small_side_button(ui, "Options...", true);
        });
        if !ocr_available {
            ui.label(RichText::new("Install Tesseract or OCR models to enable OCR.").weak());
        }
        let mut unavailable = false;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut unavailable, "Make PDF/A-1b compliant"),
        )
        .on_disabled_hover_text("Not available yet");
        ui.checkbox(&mut settings.add_metadata, "Add Metadata")
            .on_hover_text("Record a title, this program's name and the date in the PDF");
    });
}

/// The Output group of the second layout.
fn compact_output(ui: &mut Ui, settings: &mut ScanSettings, requests: &mut FormRequests) {
    let appending = settings.output == OutputMode::Append;
    if ui.radio(!appending, "New PDF Document").clicked() && appending {
        settings.output = OutputMode::NewPdf;
    }
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        let mut multiple = settings.output == OutputMode::MultipleFiles;
        ui.add_enabled_ui(!appending, |ui| {
            if ui.checkbox(&mut multiple, "Multiple files").changed() {
                settings.output = if multiple {
                    OutputMode::MultipleFiles
                } else {
                    OutputMode::NewPdf
                };
            }
        });
        requests.multiple_files_options |= small_side_button(ui, "More Options...", multiple);
    });
    if ui.radio(appending, "Append to existing file").clicked() {
        settings.output = OutputMode::Append;
    }
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        ui.add_enabled_ui(appending, |ui| {
            ui.label("Select:");
            let mut text = settings
                .append_path
                .as_ref()
                .and_then(|path| path.file_name())
                .map_or(String::new(), |name| name.to_string_lossy().into_owned());
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(230.0)
                    .interactive(false),
            );
        });
        requests.browse_append |= small_side_button(ui, "Browse...", appending);
    });
}

/// The contents of "Optimize Scanned PDF": compression choices and the page filters.
fn optimize_form(ui: &mut Ui, options: &mut OptimizeOptions) {
    group_box(ui, "Optimization Options", |ui| {
        let mut unavailable = false;
        ui.add_enabled(
            false,
            egui::Checkbox::new(&mut unavailable, "Apply Adaptive Compression"),
        )
        .on_disabled_hover_text("Not available yet");
        // One method each so far; the lists are here for when more are added.
        filter_row(
            ui,
            "Color/Grayscale:",
            "color_codec",
            &mut (),
            &[()],
            |()| "JPEG".to_owned(),
        );
        filter_row(ui, "Monochrome:", "mono_codec", &mut (), &[()], |()| {
            "CCITT Group 4".to_owned()
        });
        filter_row(
            ui,
            "Reduce resolution to:",
            "downsample",
            &mut options.downsample_dpi,
            DOWNSAMPLE_CHOICES,
            |dpi| match dpi {
                0 => "Off".to_owned(),
                dpi => format!("{dpi} dpi"),
            },
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| quality_slider(ui, &mut options.jpeg_quality));
    });

    group_box(ui, "Filters", |ui| {
        let level = |v: Level| v.label().to_owned();
        switch_row(ui, "Deskew:", "deskew", &mut options.deskew);
        filter_row(
            ui,
            "Background Removal:",
            "background",
            &mut options.background_removal,
            Level::ALL,
            level,
        );
        switch_row(ui, "Descreen:", "descreen", &mut options.descreen);
        filter_row(
            ui,
            "Text Sharpening:",
            "sharpen",
            &mut options.text_sharpening,
            Level::ALL,
            level,
        );
        switch_row(ui, "Despeckle:", "despeckle", &mut options.despeckle);
        switch_row(ui, "Crop Borders:", "crop", &mut options.crop_borders);
        switch_row(
            ui,
            "Remove Blank Pages:",
            "blank",
            &mut options.remove_blank_pages,
        );
    });
}

/// Scanner settings the second layout keeps behind its Options button.
fn scanner_details_form(ui: &mut Ui, settings: &mut ScanSettings) {
    ui.checkbox(
        &mut settings.use_native_ui,
        "Configure scanner using scanner's interface",
    );
    // With the scanner's own window in charge, the choices below belong to it.
    let own = !settings.use_native_ui;
    combo_row(
        ui,
        "Source:",
        "source_compact",
        &mut settings.source,
        Source::ALL,
        |v| v.label().to_owned(),
        own,
    );
    combo_row(
        ui,
        "Color Mode:",
        "color_compact",
        &mut settings.color_mode,
        ColorMode::ALL,
        |v| v.label().to_owned(),
        own,
    );
    combo_row(
        ui,
        "Paper Size:",
        "paper_compact",
        &mut settings.paper_size,
        PaperSize::ALL,
        |v| v.label().to_owned(),
        own,
    );
    combo_row(
        ui,
        "Resolution:",
        "dpi_compact",
        &mut settings.resolution,
        RESOLUTIONS,
        |v| format!("{v} dpi"),
        own,
    );
    combo_row(
        ui,
        "Transfer:",
        "transfer_compact",
        &mut settings.transfer,
        TransferMechanism::ALL,
        |v| v.label().to_owned(),
        true,
    );
}

/// The drop-down list of scanners.
fn scanner_combo(ui: &mut Ui, id: &str, settings: &mut ScanSettings, devices: &[Device]) {
    let current = devices
        .iter()
        .find(|device| device.key() == settings.device);
    let shown = current.map_or("(no scanner selected)", |device| device.name.as_str());
    ComboBox::from_id_salt(id)
        .width(COMBO_WIDTH)
        .truncate()
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for device in devices {
                ui.selectable_value(&mut settings.device, device.key(), &device.name);
            }
        })
        .response
        .accessible_name("Scanner");
}

/// "Small Size" to "High Quality": sets how hard page images are compressed.
fn quality_slider(ui: &mut Ui, quality: &mut u8) {
    ui.label("Small Size");
    ui.spacing_mut().slider_width = 150.0;
    ui.add(egui::Slider::new(quality, 10..=100).show_value(false))
        .accessible_name("Image quality")
        .on_hover_text(format!("Image quality {quality}"));
    ui.label("High Quality");
}

/// A checkbox for a filter with strengths: ticking it chooses Medium.
fn level_checkbox(ui: &mut Ui, level: &mut Level, text: &str) -> egui::Response {
    let mut on = *level != Level::Off;
    let response = ui.checkbox(&mut on, text);
    if response.changed() {
        *level = if on { Level::Medium } else { Level::Off };
    }
    response
}

/// A labelled drop-down list in the filter list.
fn filter_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut T,
    choices: &[T],
    text: impl Fn(T) -> String,
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(FILTER_LABEL_WIDTH, ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(FILTER_LABEL_WIDTH);
                ui.label(label);
            },
        );
        ComboBox::from_id_salt(id)
            .width(150.0)
            .selected_text(text(*value))
            .show_ui(ui, |ui| {
                for &choice in choices {
                    ui.selectable_value(value, choice, text(choice));
                }
            })
            .response
            .accessible_name(label);
    });
}

/// An On/Off drop-down list in the filter list.
fn switch_row(ui: &mut Ui, label: &str, id: &str, value: &mut bool) {
    filter_row(ui, label, id, value, &[true, false], |on| {
        if on { "On" } else { "Off" }.to_owned()
    });
}

/// A narrower `side_button`, for the second layout.
fn small_side_button(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_enabled(enabled, Button::new(text).min_size(egui::vec2(104.0, 0.0)))
            .clicked()
    })
    .inner
}

/// Whether Escape was pressed this frame. Dialogs close on Escape, their X or Cancel, but not
/// on a click beside them: the click that starts the program can land there and would close
/// the Scan dialog before it is seen.
fn escape_pressed(ctx: &Context) -> bool {
    ctx.input(|input| input.key_pressed(egui::Key::Escape))
}

/// A dialog's heading row. Returns whether its close button was clicked.
fn title_bar(ui: &mut Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui
                .add(Button::new("X").frame(false))
                .on_hover_text("Close")
                .clicked();
        });
    });
    ui.separator();
    ui.add_space(2.0);
    close
}

/// OK and Cancel at the right edge. Returns `(ok, cancel)`, keeping an earlier cancel.
fn ok_cancel(ui: &mut Ui, cancelled: bool) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = cancelled;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let size = egui::vec2(90.0, 0.0);
        cancel |= ui.add(Button::new("Cancel").min_size(size)).clicked();
        ok = ui.add(Button::new("OK").min_size(size)).clicked();
    });
    (ok, cancel)
}

/// A captioned frame around related settings.
fn group_box(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.label(title);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui);
    });
}

/// A left-aligned label of fixed width, so the controls after it line up.
fn label_cell(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_WIDTH, ROW_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(LABEL_WIDTH);
            ui.label(text);
        },
    );
}

/// A button pushed to the right edge of the current row. Returns whether it was clicked.
fn side_button(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_enabled(
            enabled,
            Button::new(text).min_size(egui::vec2(SIDE_BUTTON_WIDTH, 0.0)),
        )
        .clicked()
    })
    .inner
}

/// A labelled drop-down list. When disabled it shows nothing, as its value is not in use.
fn combo_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut T,
    choices: &[T],
    text: impl Fn(T) -> String,
    enabled: bool,
) {
    ui.horizontal(|ui| {
        label_cell(ui, label);
        ui.add_enabled_ui(enabled, |ui| {
            let shown = if enabled { text(*value) } else { String::new() };
            ComboBox::from_id_salt(id)
                .width(COMBO_WIDTH)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for &choice in choices {
                        ui.selectable_value(value, choice, text(choice));
                    }
                })
                .response
                .accessible_name(label);
        });
    });
}

/// Uploads a reduced copy of the page for the page list and preview.
fn preview_texture(ctx: &Context, page: &Page, index: usize) -> TextureHandle {
    let small = &page.preview;
    let size = [small.width() as usize, small.height() as usize];
    let image = egui::ColorImage::from_rgb(size, small.as_raw());
    ctx.load_texture(format!("page-{index}"), image, egui::TextureOptions::LINEAR)
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn test_app() -> App {
        let device = Device {
            backend: FILE_IMPORT_BACKEND,
            id: "import".to_owned(),
            name: "Image files (import)".to_owned(),
        };
        let settings = ScanSettings {
            device: device.key(),
            ocr: false,
            prompt_more_pages: false,
            ..Default::default()
        };
        let (events_tx, events_rx) = channel();
        App {
            models: PathBuf::new(),
            storage: std::env::temp_dir().join(format!("openscan-ui-{}", uuid::Uuid::new_v4())),
            session: None,
            host_events: Vec::new(),
            host_accept: false,
            scan_notes: false,
            initialized: true,
            standalone: true,
            injected: None,
            cancel_token: CancellationToken::default(),
            profile: ScanProfile::default(),
            startup_window_frames: 2,
            devices: vec![device],
            presets: PresetStore::default(),
            presets_path: PathBuf::new(),
            settings,
            ocr_engine: None,
            ocr_languages: Vec::new(),
            scan_open: false,
            presets_open: false,
            selected_preset: None,
            preset_editor: None,
            options: OptionWindows::default(),
            pages: Vec::new(),
            previews: Vec::new(),
            selected_page: 0,
            events_tx,
            events_rx,
            busy: None,
            ask_more_pages: false,
            log: Vec::new(),
            status: "Ready".to_owned(),
            error: None,
        }
    }

    fn input_for_pixels(ctx: &Context, size: egui::Vec2) -> egui::RawInput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                size / ctx.zoom_factor(),
            )),
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(1.0);
        input
    }

    fn wait_for_worker(app: &mut App, ctx: &Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while app.busy.is_some() {
            app.read_events(ctx);
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn scan_more_preserves_pages_on_failure_then_done_saves_both_batches() {
        let directory = std::env::temp_dir().join(format!(
            "scan-module-ui-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let first = directory.join("first.png");
        let second = directory.join("second.png");
        let output = directory.join("both.pdf");
        for (path, start) in [(&first, 30), (&second, 90)] {
            let image = image::GrayImage::from_fn(180, 220, |x, y| {
                image::Luma([
                    if (start..start + 30).contains(&x) && (30..190).contains(&y) {
                        0
                    } else {
                        255
                    },
                ])
            });
            image.save(path).unwrap();
        }
        let original_first = std::fs::read(&first).unwrap();
        let original_second = std::fs::read(&second).unwrap();
        let ctx = Context::default();
        let mut app = test_app();
        let device = app.devices[0].clone();

        app.start_scan_job(&ctx, false, device.clone(), vec![first.clone()]);
        wait_for_worker(&mut app, &ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.pages.len(), 1);
        assert_eq!(app.previews.len(), 1);
        assert_eq!(app.status, "1 page(s) ready");
        let held = app.pages[0].image.clone();

        app.start_scan_job(
            &ctx,
            true,
            device.clone(),
            vec![directory.join("missing.png")],
        );
        wait_for_worker(&mut app, &ctx);
        assert!(app.error.take().is_some());
        assert_eq!(app.pages.len(), 1);
        assert_eq!(app.pages[0].image, held);

        app.start_scan_job(&ctx, true, device.clone(), vec![second.clone()]);
        // A second click during acquisition must not clear or start another document.
        app.start_scan_job(&ctx, false, device, vec![first.clone()]);
        wait_for_worker(&mut app, &ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.pages.len(), 2);
        assert_eq!(app.previews.len(), 2);
        assert_eq!(app.pages[0].image, held);
        assert_ne!(app.pages[1].image, held);
        assert_eq!(app.status, "2 page(s) ready");
        assert!(
            !output.exists(),
            "scanning must wait for Done before saving"
        );

        app.start_save_job(&ctx, Target::NewPdf(output.clone()));
        wait_for_worker(&mut app, &ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(lopdf::Document::load(&output).unwrap().get_pages().len(), 2);
        assert_eq!(app.pages.len(), 2);
        assert_eq!(app.pages[0].image, held);
        assert_eq!(std::fs::read(&first).unwrap(), original_first);
        assert_eq!(std::fs::read(&second).unwrap(), original_second);
        // The same Done button hands a durable session to embedded hosts.
        app.host_accept = true;
        app.start_save(&ctx);
        wait_for_worker(&mut app, &ctx);
        let accepted = app
            .take_events()
            .into_iter()
            .find_map(|e| match e {
                OpenScanEvent::Accepted(session) => Some(session),
                _ => None,
            })
            .expect("Done must notify the host");
        assert_eq!(accepted.pages.len(), 2);
        assert!(
            openscan_core::session::settings(&accepted)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            std::fs::read(accepted.resolve(&accepted.pages[0].original_path).unwrap()).unwrap(),
            original_first
        );
        std::fs::remove_dir_all(&app.storage).unwrap();
        for path in [first, second, output] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn interface_defaults_to_110_percent_text_and_buttons() {
        let ctx = Context::default();
        let zoom = ctx.zoom_factor();
        let mut sizes = Vec::new();
        let mut output = ctx.run_ui(input_for_pixels(&ctx, egui::vec2(1280.0, 720.0)), |ui| {
            let base = egui::Style::default();
            for scaled in [false, true] {
                ui.scope(|ui| {
                    ui.set_style(base.clone());
                    if scaled {
                        configure_controls(ui);
                    }
                    sizes.push((
                        egui::TextStyle::Button.resolve(ui.style()).size,
                        ui.spacing().interact_size,
                    ));
                });
            }
        });
        output.textures_delta.clear();
        assert_eq!(sizes[1].0, sizes[0].0 * 1.1);
        assert_eq!(sizes[1].1, sizes[0].1 * 1.1);
        assert_eq!(ctx.zoom_factor(), zoom);
    }

    #[test]
    fn holding_window_uses_65_percent_of_monitor_only_after_initial_zoom_settles() {
        let ctx = Context::default();
        let mut app = test_app();
        let mut input = input_for_pixels(&ctx, egui::vec2(1100.0, 780.0));
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .monitor_size = Some(egui::vec2(1920.0, 1080.0));
        let mut output = ctx.run_pass(input.clone(), |ctx| app.size_initial_window(ctx));
        output.textures_delta.clear();
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_)))
        );
        let mut output = ctx.run_pass(input.clone(), |ctx| app.size_initial_window(ctx));
        output.textures_delta.clear();
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::InnerSize(egui::vec2(1248.0, 702.0))));
        assert!(commands.contains(&egui::ViewportCommand::Maximized(false)));
        let mut output = ctx.run_pass(input, |ctx| app.size_initial_window(ctx));
        output.textures_delta.clear();
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_)))
        );
    }

    fn visible_text_bounds(output: &egui::FullOutput, label: &str) -> egui::Rect {
        fn find(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                egui::Shape::Vec(shapes) => {
                    shapes.iter().rev().find_map(|shape| find(shape, label))
                }
                _ => None,
            }
        }
        let (bounds, clip) = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| find(&shape.shape, label).map(|bounds| (bounds, shape.clip_rect)))
            .unwrap_or_else(|| panic!("missing label: {label}"));
        assert!(
            clip.expand(1.0).contains_rect(bounds),
            "{label} is clipped: {bounds:?} beyond {clip:?}"
        );
        bounds
    }

    #[test]
    fn enlarged_scan_dialogs_fit_small_and_full_hd_screens() {
        for (pixels, zoom) in [
            (egui::vec2(960.0, 460.0), 1.1),
            (egui::vec2(1280.0, 720.0), 2.0),
            (egui::vec2(1920.0, 1080.0), 2.0),
        ] {
            for layout in [DialogLayout::Classic, DialogLayout::Compact] {
                let ctx = Context::default();
                configure_interface(&ctx);
                // Larger user-selected zoom still needs reachable footer actions.
                ctx.set_zoom_factor(zoom);
                let mut app = test_app();
                app.scan_open = true;
                app.presets.layout = layout;
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output.textures_delta.clear();
                    output = ctx.run_ui(input_for_pixels(&ctx, pixels), |ui| {
                        app.main_window(ui);
                        app.scan_dialog(ui.ctx());
                    });
                }
                output.textures_delta.clear();
                let more = visible_text_bounds(&output, "Scan more Pages");
                let done = visible_text_bounds(&output, "Done");
                assert!(more.right() < done.left());
                assert!((more.center().y - done.center().y).abs() < 1.0);
                visible_text_bounds(&output, "Scan");
                visible_text_bounds(&output, "Cancel");
                let id = match layout {
                    DialogLayout::Classic => "scan_dialog",
                    DialogLayout::Compact => "scan_dialog_compact",
                };
                let bounds = ctx
                    .memory(|memory| memory.area_rect(Id::unique(id)))
                    .unwrap();
                assert!(
                    ctx.content_rect().expand(1.0).contains_rect(bounds),
                    "{layout:?} at {pixels:?}: {bounds:?} beyond {:?}",
                    ctx.content_rect()
                );
            }
        }
    }

    #[test]
    fn long_scanner_names_do_not_expand_either_scan_dialog() {
        for compact in [false, true] {
            let ctx = Context::default();
            let device = Device {
                backend: "sane",
                id: "net:office:scanner".to_owned(),
                name: format!("Office scanner {}", "long device name ".repeat(14)),
            };
            let mut settings = ScanSettings {
                device: device.key(),
                ..Default::default()
            };
            let devices = vec![device];
            let width = if compact { COMPACT_WIDTH } else { DIALOG_WIDTH };
            let mut bounds = egui::Rect::NOTHING;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 780.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_pass(input, |ctx| {
                    bounds = Modal::new(Id::unique("layout-regression"))
                        .show(ctx, |ui| {
                            ui.set_width(width);
                            title_bar(ui, "Scan");
                            let mut requests = FormRequests::default();
                            if compact {
                                compact_form(ui, &mut settings, &devices, true, &mut requests);
                            } else {
                                settings_form(ui, &mut settings, &devices, true, &mut requests);
                            }
                        })
                        .response
                        .rect;
                });
                output.textures_delta.clear();
            }
            assert!(bounds.is_finite());
            assert!(
                bounds.width() <= width + 40.0,
                "compact={compact}: dialog grew to {} points",
                bounds.width()
            );
        }
    }
    #[test]
    #[ignore = "Optional wgpu render; set OPENSCAN_CAPTURE to a new output directory"]
    fn capture_accepted_interface() {
        let output = std::env::var_os("OPENSCAN_CAPTURE")
            .map(PathBuf::from)
            .expect("OPENSCAN_CAPTURE");
        std::fs::create_dir_all(&output).unwrap();
        for scan in [false, true] {
            let mut app = test_app();
            app.scan_open = scan;
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1200.0, 800.0))
                .build_ui(move |ui| app.show(ui));
            harness.run_steps(4);
            harness
                .render()
                .unwrap()
                .save(output.join(if scan { "scan.png" } else { "holding.png" }))
                .unwrap();
        }
    }
}
