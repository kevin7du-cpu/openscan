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
    ColorMode, Level, OutputMode, PaperSize, Preset, PresetStore, RESOLUTIONS, ScanSettings, Sides,
    Source, TransferMechanism,
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

const SCANNER_FONT: &str = "OpenScan/Inter-Regular";

/// Add a private family without replacing the host's fonts or fallback chains.
fn scanner_font(ctx: &Context) -> egui::FontFamily {
    let family = egui::FontFamily::Name(SCANNER_FONT.into());
    let definitions = ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        (!definitions.families.contains_key(&family)).then(|| definitions.clone())
    });
    if let Some(mut definitions) = definitions {
        let mut stack = vec![SCANNER_FONT.to_owned()];
        if let Some(fallbacks) = definitions.families.get(&egui::FontFamily::Proportional) {
            stack.extend(fallbacks.iter().cloned());
        }
        definitions.font_data.insert(
            SCANNER_FONT.to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/Inter-Regular.ttf"
            ))),
        );
        definitions.families.insert(family.clone(), stack);
        ctx.set_fonts(definitions);
        ctx.request_repaint();
    }
    // A host may already have laid out text this pass. egui then installs the
    // new family at the next pass; keep the first pass safe in embedded panels.
    if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        family
    } else {
        egui::FontFamily::Proportional
    }
}

/// A local control style: embedded scanners never change their host's zoom or theme.
fn configure_controls(ui: &mut Ui) {
    let family = scanner_font(ui.ctx());
    let mut style = egui::Style {
        visuals: egui::Visuals::light(),
        ..Default::default()
    };
    let ink = egui::Color32::from_rgb(24, 28, 34);
    style.visuals.override_text_color = Some(ink);
    style.visuals.weak_text_color = Some(egui::Color32::from_rgb(65, 72, 82));
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.fg_stroke.color = ink;
    }
    for font in style.text_styles.values_mut() {
        font.family = family.clone();
        font.size *= 1.1;
    }
    style.spacing.interact_size *= 1.1;
    style.spacing.button_padding *= 1.1;
    style.spacing.item_spacing *= 1.1;
    ui.set_style(style);
}

const DIALOG_WIDTH: f32 = 500.0;
const LABEL_WIDTH: f32 = 96.0;
const COMBO_WIDTH: f32 = 210.0;
const SIDE_BUTTON_WIDTH: f32 = 128.0;
const ROW_HEIGHT: f32 = 22.0;
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
    /// Open the scan dialog without starting acquisition or resetting held pages/settings.
    /// Hosts call this when their Scan Page command is clicked, including after Cancel.
    pub fn open_scan_dialog(&mut self) {
        if !self.is_busy() {
            self.scan_open = true;
        }
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
                    if ui.button("Scan").on_hover_text("Open the scan dialog").clicked() {
                        self.open_scan_dialog();
                    }
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
                            let response = ui.add(button);
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    ui.is_enabled(),
                                    index == self.selected_page,
                                    format!("Select page {}", index + 1),
                                )
                            });
                            if response.clicked() {
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
                    ui.label("No pages yet. Click Scan to open the scan dialog.");
                });
            }
        });
    }

    fn scan_dialog(&mut self, ctx: &Context) {
        if !self.scan_open || self.preset_editor.is_some() {
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

    fn any_option_window(&self) -> bool {
        let options = &self.options;
        options.scanner_options.is_some()
            || options.tabs.is_some()
            || options.multiple_files
            || options.save_preset.is_some()
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
                    outlined_dropdown(
                        ui,
                        ComboBox::from_id_salt("transfer")
                            .width(180.0)
                            .selected_text(choice.label()),
                        |ui| {
                            for &option in TransferMechanism::ALL {
                                ui.selectable_value(choice, option, option.label());
                            }
                        },
                    )
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
                        crate::selection_controls::checkbox(ui, &mut options.deskew, "Deskew")
                            .on_hover_text(
                                "Straighten pages that went through the scanner at an angle",
                            );
                        crate::selection_controls::checkbox(
                            ui,
                            &mut options.despeckle,
                            "Despeckle",
                        )
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
                        crate::selection_controls::checkbox(ui, &mut options.descreen, "Descreen")
                            .on_hover_text("Smooth away the dot pattern of printed photographs");
                        level_checkbox(ui, &mut options.text_sharpening, "Sharpen text")
                            .on_hover_text("Make the edges of letters crisper");
                        crate::selection_controls::checkbox(ui, &mut options.crop_borders, "Crop")
                            .on_hover_text("Trim the black scanner background from the page edges");
                        crate::selection_controls::checkbox(
                            ui,
                            &mut options.remove_blank_pages,
                            "Remove blank pages",
                        )
                        .on_hover_text("Leave out pages with nothing on them");
                    }
                    OptionsTab::Compression => {
                        crate::selection_controls::checkbox(
                            ui,
                            &mut settings.optimize,
                            "Reduce size and optimize",
                        );
                        ui.add_space(4.0);
                        let options = &mut settings.optimize_opts;
                        ui.add_enabled_ui(settings.optimize, |ui| {
                            ui.add(
                                egui::Slider::new(&mut options.jpeg_quality, 10..=100)
                                    .text("Image quality"),
                            );
                            ui.horizontal(|ui| {
                                let mut reduce = options.downsample_dpi > 0;
                                if crate::selection_controls::checkbox(
                                    ui,
                                    &mut reduce,
                                    "Reduce resolution to",
                                )
                                .changed()
                                {
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
                        crate::selection_controls::checkbox_enabled(
                            ui,
                            available,
                            &mut settings.ocr,
                            "Make Searchable (run OCR)",
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
                                outlined_dropdown(
                                    ui,
                                    ComboBox::from_id_salt("ocr_language")
                                        .width(160.0)
                                        .selected_text(&options.language),
                                    |ui| {
                                        for language in &self.ocr_languages {
                                            ui.selectable_value(
                                                &mut options.language,
                                                language.clone(),
                                                language,
                                            );
                                        }
                                    },
                                )
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
                                outlined_dropdown(
                                    ui,
                                    ComboBox::from_id_salt("ocr_layout")
                                        .width(160.0)
                                        .selected_text(current),
                                    |ui| {
                                        for (mode, name) in layouts {
                                            ui.selectable_value(
                                                &mut options.page_seg_mode,
                                                mode,
                                                name,
                                            );
                                        }
                                    },
                                )
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
        // Match the inset of the grouped dropdown rows below.
        ui.add_space(egui::Frame::group(ui.style()).total_margin().left);
        let current = devices
            .iter()
            .find(|device| device.key() == settings.device);
        let shown = current.map_or("(no scanner selected)", |device| device.name.as_str());
        // ComboBox::width sets a minimum. Bound the child UI as well so a
        // long device name cannot grow beneath the Advanced Options button.
        ui.allocate_ui_with_layout(
            egui::vec2(COMBO_WIDTH, ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                outlined_dropdown(
                    ui,
                    ComboBox::from_id_salt("scanner")
                        .width(COMBO_WIDTH)
                        .truncate()
                        .selected_text(shown),
                    |ui| {
                        for device in devices {
                            ui.selectable_value(&mut settings.device, device.key(), &device.name);
                        }
                    },
                )
                .response
                .accessible_name("Scanner")
                .on_hover_text(shown);
            },
        );
        requests.scanner_options |= side_button(ui, "Advanced Options...", true);
    });

    group_box(ui, "Configure scanner", |ui| {
        crate::selection_controls::checkbox(
            ui,
            &mut settings.use_native_ui,
            "Configure scanner using scanner's interface",
        );
        crate::selection_controls::checkbox(
            ui,
            &mut settings.prompt_more_pages,
            "Prompt to scan more pages",
        );
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
        crate::selection_controls::radio_value(
            ui,
            &mut settings.output,
            OutputMode::NewPdf,
            OutputMode::NewPdf.label(),
        );
        ui.horizontal(|ui| {
            crate::selection_controls::radio_value(
                ui,
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
            crate::selection_controls::radio_value(
                ui,
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
            crate::selection_controls::checkbox(
                ui,
                &mut settings.optimize,
                "Reduce size and optimize",
            );
            requests.optimize_options |= side_button(ui, "Options...", true);
        });
        ui.horizontal(|ui| {
            crate::selection_controls::checkbox_enabled(
                ui,
                ocr_available,
                &mut settings.ocr,
                "Make Searchable (run OCR)",
            );
            requests.ocr_options |= side_button(ui, "Options...", true);
        });
        if !ocr_available {
            ui.label(RichText::new("Install Tesseract or OCR models to enable OCR.").weak());
        }
    });
}

/// A checkbox for a filter with strengths: ticking it chooses Medium.
fn level_checkbox(ui: &mut Ui, level: &mut Level, text: &str) -> egui::Response {
    let mut on = *level != Level::Off;
    let response = crate::selection_controls::checkbox(ui, &mut on, text);
    if response.changed() {
        *level = if on { Level::Medium } else { Level::Off };
    }
    response
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

/// White dropdown fields with a visible border; other controls keep their style.
fn outlined_dropdown<R>(
    ui: &mut Ui,
    combo: ComboBox<'_>,
    menu_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    ui.scope(|ui| {
        let widgets = &mut ui.style_mut().visuals.widgets;
        for state in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.bg_fill = egui::Color32::WHITE;
            state.weak_bg_fill = egui::Color32::WHITE;
            state.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(150));
        }
        // Show interaction through the outline, keeping the field white.
        let focus_outline = egui::Stroke::new(1.0, egui::Color32::from_rgb(65, 100, 145));
        widgets.hovered.bg_stroke = focus_outline;
        widgets.active.bg_stroke = focus_outline;
        widgets.open.bg_stroke = focus_outline;
        combo.show_ui(ui, menu_contents)
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
            outlined_dropdown(
                ui,
                ComboBox::from_id_salt(id)
                    .width(COMBO_WIDTH)
                    .selected_text(shown),
                |ui| {
                    for &choice in choices {
                        ui.selectable_value(value, choice, text(choice));
                    }
                },
            )
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
    use egui_kittest::{Harness, kittest::Queryable};
    use openscan_core::settings::DialogLayout;

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
    fn scan_dialog_fits_small_and_full_hd_screens_with_either_legacy_preference() {
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
                        ui.scope(|ui| {
                            configure_controls(ui);
                            app.main_window(ui);
                        });
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
                // Old preset files may still store Compact. Only the single
                // scan dialog is shown, without changing their scan settings.
                visible_text_bounds(&output, "Manage Presets...");
                let id = "scan_dialog";
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
    fn long_scanner_names_stay_clear_of_advanced_options() {
        for name in [
            "HP8C8647 (HP Smart Tank 7600 series)".to_owned(),
            format!("Office scanner {}", "long device name ".repeat(14)),
        ] {
            let device = Device {
                backend: "sane",
                id: "net:office:scanner".to_owned(),
                name,
            };
            let mut settings = ScanSettings {
                device: device.key(),
                ..Default::default()
            };
            let devices = vec![device];
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1100.0, 780.0))
                .build_ui(move |ui| {
                    show_modal(ui.ctx(), Id::unique("layout-regression"), |ui| {
                        title_bar(ui, "Scan");
                        let mut requests = FormRequests::default();
                        settings_form(ui, &mut settings, &devices, true, &mut requests);
                    });
                });
            harness.run();
            let scanner = harness
                .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Scanner")
                .rect();
            let advanced = harness.get_by_label("Advanced Options...").rect();
            assert!(
                scanner.right() + 4.0 <= advanced.left(),
                "Scanner {scanner:?} overlaps Advanced Options {advanced:?}"
            );
            assert!((scanner.center().y - advanced.center().y).abs() < 2.0);
            assert!(scanner.width() <= COMBO_WIDTH + 1.0);
            for label in [
                "Sides:",
                "Source:",
                "Color Mode:",
                "Paper Size:",
                "Resolution:",
            ] {
                let below = harness
                    .get_by_role_and_label(egui::accesskit::Role::ComboBox, label)
                    .rect();
                assert!(
                    (scanner.left() - below.left()).abs() < 1.0
                        && (scanner.right() - below.right()).abs() < 1.0,
                    "Scanner {scanner:?} is not aligned with {label} {below:?}"
                );
            }
        }
    }

    #[test]
    fn scan_page_and_button_reopen_the_single_dialog_after_cancel() {
        let mut app = test_app();
        app.standalone = false;
        app.presets.layout = DialogLayout::Compact;
        app.settings.resolution = 600;
        let settings = app.settings.clone();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    egui::Panel::top("host-toolbar").show(ui, |ui| {
                        if ui.button("Scan Page").clicked() {
                            app.open_scan_dialog();
                        }
                    });
                    app.show(ui);
                },
                app,
            );
        harness.run();
        // Every host command opens the dialog, including a second visit after Cancel.
        for _ in 0..2 {
            harness.get_by_label("Scan Page").click();
            harness.run();
            harness.get_by_label("Configure scanner");
            harness.get_by_label("Manage Presets...");
            assert!(harness.state().scan_open);
            harness.get_by_label("Cancel").click();
            harness.run();
            harness.run_steps(3);
            assert!(!harness.state().scan_open);
            assert_eq!(harness.state().settings, settings);
        }
        for removed in [
            "From Scanner     ",
            "Scan...",
            "Open saved pages...",
            "Manage Presets...",
            "Scan dialog: Layout 1",
            "Scan dialog: Layout 2",
        ] {
            assert!(harness.query_by_label(removed).is_none(), "{removed}");
        }
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Scan")
            .click();
        harness.run();
        assert!(harness.state().scan_open);
        assert_eq!(harness.state().settings, settings);
        assert!(harness.state().pages.is_empty());
        assert!(!harness.state().is_busy());
        harness.get_by_label("Manage Presets...").click();
        harness.run();
        harness.get_by_label("Manage Scan Presets");
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
