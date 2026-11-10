//! Embeddable OpenScan panel. Call `show(ui)` from any egui/eframe application.
//! Original control arrangement: OpenScan contributors (MIT); native Rust port.
#![forbid(unsafe_code)]
mod clean_copy;
mod corrections;
mod preview;
mod worker;
use egui::{Color32, RichText};
use scan_module::{
    Device, DeviceCapabilities, Paper, ScanColor, ScanProfile, ScannerBackend, session::ScanSession,
};
use std::{path::PathBuf, time::Duration};
use worker::{Command, Event, Worker};

/// Host notifications; scanner files remain owned by the caller's storage directory.
pub enum OpenScanEvent {
    Accepted(ScanSession),
    Exported(PathBuf),
    ScanNote(ScanSession),
}
pub struct OpenScanUi {
    worker: Worker,
    pub profile: ScanProfile,
    session: Option<ScanSession>,
    devices: Vec<Device>,
    capabilities: Option<DeviceCapabilities>,
    presets: Vec<ScanProfile>,
    selected: usize,
    texture: Option<egui::TextureHandle>,
    texture_path: Option<PathBuf>,
    preview: preview::Preview,
    clean_review: Option<clean_copy::Review>,
    correction_editor: Option<scan_module::CleanupOptions>,
    undo_label: Option<String>,
    redo_label: Option<String>,
    busy: bool,
    ready: bool,
    status: String,
    error: Option<String>,
    preview_pending: bool,
    discover_pending: bool,
    capabilities_pending: bool,
    manage: bool,
    save_preset: bool,
    preset_name: String,
    preset_selected: Option<String>,
    replace: bool,
    rename: bool,
    events: Vec<OpenScanEvent>,
    accept_label: Option<String>,
    scan_notes: bool,
    filing_format: Option<scan_module::ScanFileFormat>,
    instance: String,
    native_dialog: bool,
    editor_backup: Option<ScanProfile>,
    editor_existing: Option<String>,
    name_profile: Option<ScanProfile>,
}
impl OpenScanUi {
    pub fn new(storage: PathBuf, models: PathBuf) -> Self {
        Self::with_backend(storage, models, scan_module::native_backend())
    }
    /// Supply a mock, eSCL, TWAIN or other acquisition implementation without changing this UI.
    pub fn with_backend(
        storage: PathBuf,
        models: PathBuf,
        backend: Box<dyn ScannerBackend>,
    ) -> Self {
        let native_dialog = backend.capabilities().driver_dialog;
        Self {
            worker: Worker::start(storage, models, backend),
            profile: ScanProfile::default(),
            session: None,
            devices: Vec::new(),
            capabilities: None,
            presets: Vec::new(),
            selected: 0,
            texture: None,
            texture_path: None,
            preview: preview::Preview::default(),
            clean_review: None,
            correction_editor: None,
            undo_label: None,
            redo_label: None,
            busy: true,
            ready: false,
            status: "Opening scan session".into(),
            error: None,
            preview_pending: false,
            discover_pending: true,
            capabilities_pending: false,
            manage: false,
            save_preset: false,
            preset_name: String::new(),
            preset_selected: None,
            replace: false,
            rename: false,
            events: Vec::new(),
            accept_label: None,
            scan_notes: false,
            filing_format: None,
            instance: uuid::Uuid::new_v4().to_string(),
            native_dialog,
            editor_backup: None,
            editor_existing: None,
            name_profile: None,
        }
    }
    pub fn set_accept_label(&mut self, label: impl Into<String>) {
        self.accept_label = Some(label.into());
    }
    /// Show Scan Note when the host provides a text-note editor.
    pub fn enable_scan_notes(&mut self) {
        self.scan_notes = true;
    }
    /// Opt in to a format-aware host filing action. The host reads `filing_format`
    /// when consuming Accepted and creates the requested PDF/JPEG in its catalog.
    pub fn enable_filing_formats(&mut self) {
        self.accept_label = Some("File as".into());
        self.filing_format = Some(scan_module::ScanFileFormat::Pdf);
    }
    pub fn filing_format(&self) -> Option<scan_module::ScanFileFormat> {
        self.filing_format
    }
    pub fn take_events(&mut self) -> Vec<OpenScanEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn session(&self) -> Option<&ScanSession> {
        self.session.as_ref()
    }
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn cancel(&self) {
        self.worker.cancel();
    }
    pub fn import_images(&mut self, paths: Vec<PathBuf>) {
        self.submit(
            Command::Import(paths, self.profile.clone()),
            "Importing and correcting images",
        );
    }
    pub fn open_session(&mut self, directory: PathBuf) {
        self.submit(Command::Open(directory), "Opening saved session");
    }
    fn submit(&mut self, command: Command, status: &str) {
        if self.busy {
            return;
        }
        match self.worker.submit(command) {
            Ok(()) => {
                self.busy = true;
                self.status = status.into();
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                Event::Session(session) => {
                    self.ready = true;
                    self.selected = self.selected.min(session.pages.len().saturating_sub(1));
                    let path = session
                        .pages
                        .get(self.selected)
                        .map(|p| session.directory.join(&p.image_path));
                    if path != self.texture_path {
                        self.preview_pending = path.is_some();
                        if path.is_none() {
                            self.texture = None;
                            self.texture_path = None;
                        }
                    }
                    self.session = Some(session);
                }
                Event::Devices(devices) => {
                    // The previous selection may have been produced by corrupt
                    // SANE discovery output; discard it if it is no longer valid.
                    if !self
                        .profile
                        .device_id
                        .as_ref()
                        .is_some_and(|id| devices.iter().any(|d| &d.id == id))
                    {
                        self.profile.device_id = devices.first().map(|d| d.id.clone());
                    }
                    self.capabilities = None;
                    self.devices = devices;
                    self.capabilities_pending = self.profile.device_id.is_some();
                    if self.devices.is_empty() {
                        self.status="No scanner found. Connect a supported scanner and refresh, or import images.".into();
                    }
                }
                Event::Capabilities(capabilities) => {
                    if self.profile.device_id.as_deref() == Some(&capabilities.device_id) {
                        if !capabilities
                            .sources
                            .iter()
                            .any(|c| c.source == self.profile.source)
                            && let Some(first) = capabilities.sources.first()
                        {
                            self.profile.source = first.source;
                        }
                        if capabilities.sources.is_empty() {
                            self.profile.native_interface = self.native_dialog;
                        }
                        self.capabilities = Some(capabilities);
                        self.fit_resolution();
                        self.status =
                            "Scanner ready. Choose settings, then scan or import pages.".into();
                    }
                }
                Event::Presets(presets) => self.presets = presets,
                Event::Corrections { options, saved } => {
                    // Hosts may set their own profile before the initial poll.
                    if saved || self.profile.cleanup == Default::default() {
                        self.profile.cleanup = options;
                    }
                    if saved {
                        self.correction_editor = None;
                    }
                }
                Event::CleanPreview(data) => {
                    self.clean_review = Some(clean_copy::Review::new(ctx, *data));
                    self.status = "Clean copy ready for review; original scan preserved.".into();
                }
                Event::CleanApplied(index) => {
                    self.clean_review = None;
                    self.selected = index;
                    self.preview_pending = true;
                }
                Event::Focus(index) => self.selected = index,
                Event::History(undo, redo) => {
                    self.undo_label = undo;
                    self.redo_label = redo;
                }
                Event::Preview(index, image, original_size) => {
                    if index == self.selected {
                        self.preview.set_source(original_size);
                        self.texture = Some(ctx.load_texture(
                            format!("openscan-preview-{}", self.instance),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                image.as_raw(),
                            ),
                            egui::TextureOptions::LINEAR,
                        ));
                        self.texture_path = self.session.as_ref().and_then(|s| {
                            s.pages.get(index).map(|p| s.directory.join(&p.image_path))
                        });
                        self.preview_pending = false;
                    }
                }
                Event::Progress(status) | Event::Message(status) => self.status = status,
                Event::Error(error) => {
                    self.error = Some(error);
                    self.status =
                        "Operation stopped; completed pages remain in the saved session.".into();
                }
                Event::Exported(path) => self.events.push(OpenScanEvent::Exported(path)),
                Event::Done => self.busy = false,
            }
        }
        if !self.busy && self.ready {
            let previous_error = self.error.clone();
            if self.discover_pending {
                self.discover_pending = false;
                self.submit(Command::Devices, "Finding scanners");
            } else if self.capabilities_pending {
                self.capabilities_pending = false;
                if let Some(id) = self.profile.device_id.clone() {
                    self.submit(Command::Capabilities(id), "Reading scanner capabilities");
                }
            } else if self.preview_pending {
                self.preview_pending = false;
                let max_side = ctx.input(|input| input.max_texture_side).clamp(1, 4096) as u32;
                self.submit(
                    Command::Preview(self.selected, max_side),
                    "Loading page preview",
                );
            }
            if self.error.is_none() {
                self.error = previous_error;
            }
        }
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
    }
    fn fit_resolution(&mut self) {
        if !self.native_dialog {
            self.profile.native_interface = false;
        }
        if let Some(source) = self
            .capabilities
            .as_ref()
            .and_then(|c| c.sources.iter().find(|s| s.source == self.profile.source))
            && !source.resolutions.is_empty()
            && !source.resolutions.contains(&self.profile.dpi)
        {
            self.profile.dpi = *source
                .resolutions
                .iter()
                .min_by_key(|dpi| dpi.abs_diff(self.profile.dpi))
                .unwrap_or(&300);
        }
    }
    fn import_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Page images",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_files()
        {
            self.import_images(paths);
        }
    }
    fn scan(&mut self) {
        self.submit(Command::Scan(self.profile.clone()), "Starting scanner");
    }
    fn save_pdf(&mut self, append: bool) {
        let source = if append {
            let Some(source) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_title("Append pages to a new PDF copy")
                .pick_file()
            else {
                return;
            };
            Some(source)
        } else {
            None
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("Scanned document.pdf")
            .save_file()
        {
            self.submit(
                Command::Export {
                    path,
                    append: source,
                    profile: self.profile.clone(),
                },
                "Exporting PDF",
            );
        }
    }
    fn can_scan(&self) -> bool {
        self.profile
            .device_id
            .as_ref()
            .is_some_and(|id| self.devices.iter().any(|d| &d.id == id))
            && (self.profile.native_interface
                || self
                    .capabilities
                    .as_ref()
                    .is_some_and(|c| c.sources.iter().any(|s| s.source == self.profile.source)))
    }
    fn scan_actions(&mut self, ui: &mut egui::Ui) {
        let included = self
            .session
            .as_ref()
            .is_some_and(|s| s.pages.iter().any(|p| !p.excluded));
        ui.add_enabled_ui(self.ready&&!self.busy,|ui|{
            if ui.add_enabled(self.can_scan(),egui::Button::new("Scan")).clicked(){self.scan();}
            if let Some(label)=&self.accept_label
                && ui.add_enabled(included,egui::Button::new(label))
                    .on_hover_text("Save in the selected PaperDesk folder using the chosen format. PDF combines included pages; JPG creates one image per page. Originals are preserved.").clicked()
                && let Some(session)=&self.session {self.events.push(OpenScanEvent::Accepted(session.clone()));}
            if let Some(format)=&mut self.filing_format {
                use scan_module::ScanFileFormat;
                egui::ComboBox::from_id_salt("scan-filing-format").selected_text(match format {ScanFileFormat::Pdf=>"PDF",ScanFileFormat::Jpeg=>"Image (JPG)"}).width(100.0).show_ui(ui,|ui| {
                    ui.selectable_value(format,ScanFileFormat::Pdf,"PDF");
                    ui.selectable_value(format,ScanFileFormat::Jpeg,"Image (JPG)");
                }).response.accessible_name("File as format");
            }
            if ui.button("Import images").clicked(){self.import_dialog();}
            if ui.add_enabled(included,egui::Button::new("Save PDF"))
                .on_hover_text("Export included pages to a separate PDF file. Enable Searchable PDF for OCR text.").clicked(){self.save_pdf(false);}
            if ui.add_enabled(included,egui::Button::new("Append to PDF copy")).on_hover_text("Creates a new file; document-level forms, bookmarks and signatures are not preserved.").clicked(){self.save_pdf(true);}
        });
        if ui
            .add_enabled(self.busy, egui::Button::new("Cancel"))
            .clicked()
        {
            self.worker.cancel();
            self.status = "Cancelling; completed pages will be retained.".into();
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("scan-settings-scroll").auto_shrink([false,false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible).show(ui,|ui|{
            ui.add_enabled_ui(!self.busy&&self.ready,|ui|{
                heading(ui,"Scanner");
                let previous=self.profile.device_id.clone();
                let selected=self.devices.iter().find(|d|Some(&d.id)==self.profile.device_id.as_ref()).map(|d|d.name.as_str()).unwrap_or("Select a scanner");
                egui::ComboBox::from_id_salt("scanner-device").width(300.0).selected_text(selected).show_ui(ui,|ui|{for d in &self.devices {ui.selectable_value(&mut self.profile.device_id,Some(d.id.clone()),&d.name);}}).response.accessible_name("Scanner");
                if previous!=self.profile.device_id {self.capabilities=None;self.capabilities_pending=true;}
                if ui.button("Refresh scanners").clicked(){self.submit(Command::Devices,"Finding scanners");}
                if let Some(caps)=&self.capabilities {ui.small(&caps.note);} else {ui.small("Choose a scanner, or import images to try corrections.");}
                ui.add_enabled(self.native_dialog,egui::Checkbox::new(&mut self.profile.native_interface,"Use scanner's interface"));
                heading(ui,"Acquisition");
                ui.add_enabled_ui(!self.profile.native_interface,|ui|{
                    let previous=self.profile.source;
                    let sources=self.capabilities.as_ref().map(|c|c.sources.iter().map(|s|s.source).collect::<Vec<_>>()).unwrap_or_default();
                    enum_choice(ui,"Source / sides",&mut self.profile.source,&sources);
                    if previous!=self.profile.source{self.fit_resolution();}
                    enum_choice(ui,"Color",&mut self.profile.color,&[ScanColor::Color,ScanColor::Grayscale,ScanColor::BlackWhite]);
                    enum_choice(ui,"Paper size",&mut self.profile.paper,&[Paper::Letter,Paper::A4,Paper::Legal,Paper::A5]);
                    let resolutions=self.capabilities.as_ref().and_then(|c|c.sources.iter().find(|s|s.source==self.profile.source)).map(|s|s.resolutions.clone()).unwrap_or_default();
                    ui.horizontal(|ui|{ui.label("Resolution (DPI)");egui::ComboBox::from_id_salt("scan-dpi").selected_text(self.profile.dpi.to_string()).show_ui(ui,|ui|{for dpi in &resolutions{ui.selectable_value(&mut self.profile.dpi,*dpi,dpi.to_string());}}).response.accessible_name("Resolution DPI");});
                });
                ui.horizontal(|ui|{ui.label("Maximum pages");ui.add(egui::DragValue::new(&mut self.profile.max_pages).range(1..=250)).accessible_name("Maximum pages");});
                ui.checkbox(&mut self.profile.flip_duplex_backs,"Rotate duplex backs 180°");
                ui.add_space(12.0);
                if self.editor_backup.is_some() {
                    // A named preset has its own corrections; editing it must not
                    // replace the separately saved defaults.
                    egui::CollapsingHeader::new("Image correction").show(ui,|ui| corrections::controls(ui,&mut self.profile.cleanup));
                } else if corrections::settings_button(ui).clicked() {
                    self.correction_editor=Some(self.profile.cleanup.clone());
                }
                heading(ui,"Output");
                ui.checkbox(&mut self.profile.searchable_pdf,"Searchable PDF (local OCR)");
                ui.small("PDF: lossless compression. JPG: maximum quality, one file per page.");
                ui.small("Imported images use the selected DPI when resolution metadata is unavailable. Review faint pages before excluding them.");
            });
        });
    }
    /// Render the original OpenScan arrangement inside the caller's available egui area.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        ui.push_id(self.instance.clone(),|ui|{
            if self.editor_backup.is_some() || self.clean_review.is_some() || self.correction_editor.is_some(){ui.disable();}
            egui::Panel::top(format!("scan-heading-{}",self.instance)).show(ui,|ui|{
                ui.add_enabled_ui(self.ready&&!self.busy,|ui|{ui.horizontal(|ui|{
                    ui.menu_button("From Scanner",|ui|{if ui.add_enabled(self.can_scan(),egui::Button::new("Scan")).clicked(){self.scan();ui.close();}
                    if ui.button("Manage presets").clicked(){self.manage=true;ui.close();}});
                    ui.menu_button("Session",|ui|{
                        if ui.button("Import images…").clicked(){self.import_dialog();ui.close();}
                        if ui.button("Open saved session…").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Scan session",&["json"]).pick_file() && let Some(parent)=path.parent(){self.open_session(parent.into());}ui.close();}
                        if ui.button("New session").clicked(){self.submit(Command::New,"Creating new session");ui.close();}
                        let included=self.session.as_ref().is_some_and(|s|s.pages.iter().any(|p|!p.excluded));
                        if ui.add_enabled(included,egui::Button::new("Save images…")).clicked() && let Some(parent)=rfd::FileDialog::new().pick_folder(){self.submit(Command::Images(parent.join(format!("scan-images-{}",uuid::Uuid::new_v4()))),"Saving PNG pages");ui.close();}
                        if let Some(session)=&self.session {ui.label("Saved session folder");ui.label(session.directory.display().to_string());if ui.button("Copy session folder").clicked(){ui.ctx().copy_text(session.directory.display().to_string());}}
                    });
                });});
                ui.heading("Scan documents");
                ui.horizontal_wrapped(|ui|{ui.add_enabled_ui(self.ready&&!self.busy,|ui|{
                    ui.label("Preset");let mut chosen=None;
                    egui::ComboBox::from_id_salt("scan-preset").width(230.0).selected_text(&self.profile.name).show_ui(ui,|ui|{for profile in &self.presets{if ui.selectable_label(self.preset_selected.as_deref()==Some(&profile.name),&profile.name).clicked(){chosen=Some(profile.clone());}}}).response.accessible_name("Scan preset");
                    if let Some(profile)=chosen {self.preset_selected=Some(profile.name.clone());self.profile=profile;self.capabilities=None;self.capabilities_pending=self.profile.device_id.is_some();}
                    if ui.button("Save as preset").clicked(){self.preset_name=self.profile.name.clone();self.name_profile=None;self.replace=false;self.rename=false;self.save_preset=true;}
                    if ui.button("Manage presets").clicked(){self.manage=true;}
                });ui.separator();self.scan_actions(ui);});
            });
            egui::Panel::bottom(format!("scan-actions-{}",self.instance)).show(ui,|ui|{
                ui.separator();ui.horizontal_wrapped(|ui|{if self.busy{ui.spinner();}ui.label(&self.status);});
                if let Some(error)=self.error.clone(){ui.horizontal_wrapped(|ui|{ui.colored_label(Color32::from_rgb(180,55,45),error);if ui.button("Dismiss scanner error").clicked(){self.error=None;}});}
                self.edit_actions(ui);
            });
            egui::Panel::left(format!("scan-settings-{}",self.instance)).default_size(350.0).size_range(280.0..=410.0).resizable(true).show(ui,|ui|self.settings(ui));
            egui::CentralPanel::default().show(ui,|ui|{
                let count=self.session.as_ref().map_or(0,|s|s.pages.len());
                egui::Panel::top(format!("scan-tools-{}",self.instance)).show(ui,|ui|{ui.horizontal_wrapped(|ui|{
                    self.history_actions(ui);
                    ui.add_enabled_ui(!self.busy&&count>0,|ui|{
                    if ui.button("Rotate 90°").clicked(){self.submit(Command::Rotate(self.selected),"Rotating page");}
                    if ui.add_enabled(self.selected>0,egui::Button::new("Move up")).clicked(){let next=self.selected-1;self.submit(Command::Move(self.selected,next),"Reordering pages");self.selected=next;}
                    if ui.add_enabled(self.selected+1<count,egui::Button::new("Move down")).clicked(){let next=self.selected+1;self.submit(Command::Move(self.selected,next),"Reordering pages");self.selected=next;}
                    if ui.button("Del").accessible_name("Delete selected page").on_hover_text("Delete the selected page from this scan. Undo restores it.").clicked(){self.delete_page(self.selected);}
                    if ui.button("OCR Clean").on_hover_text("Whiten paper, reduce wrinkle shadows and remove isolated specks while preserving printed text. Review before Apply; Undo restores the scan.").clicked(){let max_side=ctx.input(|i|i.max_texture_side).clamp(1,4096) as u32;self.submit(Command::PrepareClean(self.selected,max_side),"Preparing OCR clean copy");}
                    if self.scan_notes && ui.button("Scan Note").on_hover_text("Open text from all included scan pages in PaperDesk's notepad. Reuse OCR when available; recognize missing text automatically. Formatting is optional.").clicked()
                        && let Some(session) = &self.session { self.events.push(OpenScanEvent::ScanNote(session.clone())); }
                });});});
                egui::Panel::bottom(format!("scan-details-{}",self.instance)).show(ui,|ui|{
                    if let Some(page)=self.session.as_ref().and_then(|s|s.pages.get(self.selected)){ui.label(format!("Page {} of {} · {} DPI · deskew {:.2}° · ink {:.2}%{}",self.selected+1,count,page.dpi,page.deskew_angle,page.ink_percent,if page.excluded{" · EXCLUDED"}else{""}));}
                    else {ui.label("Page preview — scans stay on this computer.");}
                    self.preview.controls(ui,self.texture.is_some());
                });
                egui::Panel::left(format!("scan-pages-{}",self.instance)).default_size(150.0).size_range(100.0..=220.0).resizable(true).show(ui,|ui|{
                    egui::ScrollArea::vertical().auto_shrink([false,false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible).show(ui,|ui|{
                        let mut chosen=None;
                        let mut deleted=None;
                        ui.add_enabled_ui(!self.busy,|ui|{
                            if let Some(session)=&self.session {
                                for (index,page) in session.pages.iter().enumerate() {
                                    let response=ui.selectable_label(self.selected==index,format!("Page {}{}",index+1,if page.excluded{" — excluded"}else{""}));
                                    if response.clicked(){chosen=Some(index);}
                                    response.context_menu(|ui| {
                                        if ui.button("Delete page").on_hover_text("Remove this page from the scan. Undo restores it.").clicked(){deleted=Some(index);ui.close();}
                                    });
                                }
                            }
                        });
                        if let Some(index)=chosen {self.selected=index;self.preview_pending=true;}
                        if let Some(index)=deleted {self.delete_page(index);}
                    });
                });
                egui::CentralPanel::default().frame(egui::Frame::new().fill(if ui.visuals().dark_mode{Color32::from_rgb(40,45,51)}else{Color32::WHITE}).inner_margin(12.0)).show(ui,|ui|{
                    if let Some(texture)=&self.texture {
                        let response=self.preview.show(ui,texture);
                        response.context_menu(|ui| {
                            if ui.add_enabled(!self.busy,egui::Button::new("Delete page")).on_hover_text("Delete the displayed page. Undo restores it.").clicked(){self.delete_page(self.selected);ui.close();}
                        });
                    }
                    else {ui.centered_and_justified(|ui|{ui.label("Scan or import a page to preview it here.");});}
                });
            });
        });
        self.preset_windows(&ctx);
        self.correction_window(&ctx);
        if let Some(review) = &mut self.clean_review {
            let (command, close) = review.show(&ctx, self.busy, self.error.as_deref());
            if close {
                self.clean_review = None;
            }
            if let Some(command) = command {
                self.submit(command, "Updating scan cleanup");
            }
        }
        if self.busy
            && egui_office_widgets::task_progress_popup(
                &ctx,
                egui::Id::unique(("scan-progress", &self.instance)),
                &self.status,
                None,
            )
        {
            self.worker.cancel();
            self.status = "Cancelling after the current processing step".into();
        }
    }
    fn edit_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let has_pages = self.session.as_ref().is_some_and(|s| !s.pages.is_empty());
            if ui.add_enabled(!self.busy && has_pages, egui::Button::new("Apply corrections"))
                .on_hover_text("Apply the current correction settings to all scan pages, starting from their preserved originals. Undo restores the previous images.").clicked() {
                self.submit(Command::Correct(self.profile.clone()), "Applying corrections from originals");
            }
        });
    }
    fn history_actions(&mut self, ui: &mut egui::Ui) {
        let undo = self.undo_label.as_ref().map_or_else(
            || "Undo — no changes to undo".into(),
            |label| format!("Undo: {label}"),
        );
        if history_arrow(
            ui,
            !self.busy && self.undo_label.is_some(),
            false,
            "Undo",
            &undo,
        )
        .clicked()
        {
            self.submit(Command::Undo, "Undoing scan edit");
        }
        let redo = self.redo_label.as_ref().map_or_else(
            || "Redo — no changes to redo".into(),
            |label| format!("Redo: {label}"),
        );
        if history_arrow(
            ui,
            !self.busy && self.redo_label.is_some(),
            true,
            "Redo",
            &redo,
        )
        .clicked()
        {
            self.submit(Command::Redo, "Redoing scan edit");
        }
    }
    fn delete_page(&mut self, index: usize) {
        self.selected = index;
        self.submit(Command::Delete(index), "Deleting page from scan session");
    }
    fn preset_windows(&mut self, ctx: &egui::Context) {
        if self.manage {
            let mut open = true;
            egui::Window::new("Manage presets")
                .id(egui::Id::unique(format!(
                    "preset-manager-{}",
                    self.instance
                )))
                .open(&mut open)
                .default_width(600.0)
                .show(ctx, |ui| {
                    ui.add_enabled_ui(!self.busy && self.editor_backup.is_none(), |ui| {
                        ui.heading("Scanner presets");
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .min_scrolled_height(160.0)
                            .show(ui, |ui| {
                                for preset in &self.presets {
                                    ui.selectable_value(
                                        &mut self.preset_selected,
                                        Some(preset.name.clone()),
                                        &preset.name,
                                    );
                                }
                            });
                        if let Some(p) = self
                            .presets
                            .iter()
                            .find(|p| Some(&p.name) == self.preset_selected.as_ref())
                        {
                            ui.separator();
                            ui.label(format!(
                                "{:?} · {:?} · {:?} · {} DPI",
                                p.source, p.color, p.paper, p.dpi
                            ));
                            ui.label(format!(
                                "Deskew: {}   Crop: {}   Skip blank: {}",
                                p.cleanup.deskew, p.cleanup.auto_crop, p.cleanup.skip_blank_pages
                            ));
                            ui.label(format!(
                                "Searchable PDF: {}   PDF compression: lossless",
                                p.searchable_pdf
                            ));
                        }
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("New").clicked() {
                                self.begin_preset_edit(ScanProfile::default(), None);
                            }
                            let selected = self
                                .presets
                                .iter()
                                .find(|p| Some(&p.name) == self.preset_selected.as_ref())
                                .cloned();
                            if ui
                                .add_enabled(selected.is_some(), egui::Button::new("Edit"))
                                .clicked()
                                && let Some(p) = selected.clone()
                            {
                                self.begin_preset_edit(p.clone(), Some(p.name));
                            }
                            if ui
                                .add_enabled(selected.is_some(), egui::Button::new("Duplicate"))
                                .clicked()
                                && let Some(p) = selected.clone()
                            {
                                self.preset_name = format!("{} copy", p.name);
                                self.name_profile = Some(p);
                                self.replace = false;
                                self.rename = false;
                                self.save_preset = true;
                            }
                            if ui
                                .add_enabled(selected.is_some(), egui::Button::new("Rename"))
                                .clicked()
                                && let Some(p) = selected.clone()
                            {
                                self.preset_name = p.name;
                                self.rename = true;
                                self.replace = false;
                                self.save_preset = true;
                            }
                            if ui
                                .add_enabled(selected.is_some(), egui::Button::new("Delete"))
                                .clicked()
                                && let Some(p) = selected
                            {
                                self.submit(Command::DeletePreset(p.name), "Archiving preset");
                                self.preset_selected = None;
                            }
                            if ui.button("Close").clicked() {
                                self.manage = false;
                            }
                        });
                        ui.small("Deleted presets are retained in Presets/deleted.");
                    });
                });
            if !open {
                self.manage = false;
            }
        }
        if self.editor_backup.is_some() {
            let mut open = true;
            let mut close = false;
            egui::Window::new(if self.editor_existing.is_some() {
                "Edit preset"
            } else {
                "New preset"
            })
            .id(egui::Id::unique(format!("preset-editor-{}", self.instance)))
            .open(&mut open)
            .default_size(egui::vec2(400.0, 650.0))
            .show(ctx, |ui| {
                egui::Panel::bottom(format!("preset-editor-footer-{}", self.instance)).show(
                    ui,
                    |ui| {
                        ui.add_enabled_ui(!self.busy, |ui| {
                            ui.text_edit_singleline(&mut self.profile.name)
                                .accessible_name("Preset name");
                            ui.horizontal(|ui| {
                                if ui.button("Save preset").clicked() {
                                    let replace = self.editor_existing.as_deref()
                                        == Some(self.profile.name.as_str());
                                    self.submit(
                                        Command::SavePreset(self.profile.clone(), replace),
                                        "Saving preset",
                                    );
                                    close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    close = true;
                                }
                            });
                        });
                    },
                );
                self.settings(ui);
            });
            if !open || close {
                if let Some(profile) = self.editor_backup.take() {
                    self.profile = profile;
                }
                self.editor_existing = None;
                self.capabilities = None;
                self.capabilities_pending = self.profile.device_id.is_some();
            }
        }
        if self.save_preset {
            let mut open = true;
            egui::Window::new(if self.rename {
                "Rename preset"
            } else {
                "Save scan preset"
            })
            .id(egui::Id::unique(format!("save-preset-{}", self.instance)))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.text_edit_singleline(&mut self.preset_name)
                        .accessible_name("Preset name");
                    if !self.rename {
                        ui.checkbox(
                            &mut self.replace,
                            "Replace an existing preset with this name",
                        );
                    }
                    if ui.button("Save preset").clicked() {
                        if self.rename {
                            if let Some(old) = self.preset_selected.clone() {
                                self.submit(
                                    Command::RenamePreset(old, self.preset_name.clone()),
                                    "Renaming preset",
                                );
                            }
                        } else {
                            let mut profile = self
                                .name_profile
                                .take()
                                .unwrap_or_else(|| self.profile.clone());
                            profile.name = self.preset_name.clone();
                            self.submit(
                                Command::SavePreset(profile, self.replace),
                                "Saving preset",
                            );
                        }
                        self.save_preset = false;
                    }
                });
            });
            if !open {
                self.save_preset = false;
            }
        }
    }
    fn begin_preset_edit(&mut self, profile: ScanProfile, existing: Option<String>) {
        self.editor_backup = Some(self.profile.clone());
        self.profile = profile;
        self.editor_existing = existing;
        self.capabilities = None;
        self.capabilities_pending = self.profile.device_id.is_some();
    }
}
// Draw arrows as vectors so every host/font set gets the same sharp icons.
fn history_arrow(
    ui: &mut egui::Ui,
    enabled: bool,
    right: bool,
    name: &str,
    tooltip: &str,
) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let label = ui
        .painter()
        .layout_no_wrap(name.into(), font, ui.visuals().text_color());
    let icon = 18.0;
    let padding = ui.spacing().button_padding;
    let width = icon + 4.0 + label.size().x + padding.x * 2.0;
    let height = (label.size().y + padding.y * 2.0)
        .max(ui.spacing().interact_size.y)
        .max(icon);
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::new("")
                .frame(false)
                .min_size(egui::vec2(width, height)),
        )
        .accessible_name(name)
        .on_hover_text(tooltip)
        .on_disabled_hover_text(tooltip);
    let visuals = ui.style().interact(&response);
    let color = if !response.enabled() {
        Color32::from_gray(if ui.visuals().dark_mode { 112 } else { 158 })
    } else if ui.visuals().dark_mode {
        Color32::from_rgb(115, 181, 245)
    } else {
        Color32::from_rgb(37, 112, 185)
    };
    let origin = egui::pos2(
        response.rect.left() + padding.x,
        response.rect.center().y - icon * 0.5,
    );
    let scale = icon / 36.0;
    let point = |x: f32, y: f32| origin + egui::vec2(if right { 36.0 - x } else { x }, y) * scale;
    // A curved shaft and a filled arrowhead stay sharp at every display scale.
    ui.painter()
        .add(egui::epaint::CubicBezierShape::from_points_stroke(
            [
                point(31.0, 27.0),
                point(31.0, 13.0),
                point(23.0, 12.0),
                point(12.0, 12.0),
            ],
            false,
            Color32::TRANSPARENT,
            egui::Stroke::new(5.0 * scale, color),
        ));
    ui.painter().add(egui::Shape::convex_polygon(
        vec![point(3.0, 12.0), point(15.0, 3.0), point(15.0, 21.0)],
        color,
        egui::Stroke::NONE,
    ));
    ui.painter().galley_with_override_text_color(
        egui::pos2(
            origin.x + icon + 4.0,
            response.rect.center().y - label.size().y * 0.5,
        ),
        label,
        if response.enabled() {
            visuals.fg_stroke.color
        } else {
            color
        },
    );
    response
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(text).strong());
    ui.add_space(4.0);
}
fn enum_choice<T: Copy + PartialEq + std::fmt::Debug>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    choices: &[T],
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(format!("{value:?}"))
            .show_ui(ui, |ui| {
                for option in choices {
                    ui.selectable_value(value, *option, format!("{option:?}"));
                }
            })
            .response
            .accessible_name(label);
    });
}

#[cfg(test)]
mod edit_tests;
