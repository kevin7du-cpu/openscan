//! Host-embedded egui scan and preset controls. The host owns styling and window lifecycle.
use egui::{TextureHandle, Ui};
use openscan_core::{Capabilities, Color, Device, Paper, Profile, Session, Source};

#[derive(Clone, Debug)]
pub enum Action {
    Refresh,
    SearchNetwork,
    SelectDevice(String),
    Scan,
    Import,
    SavePdf,
    SaveImages,
    AppendPdf,
    LoadPreset(usize),
    SavePreset(bool),
    DeletePreset,
    Rotate(i16),
    Move(i32),
    TogglePage,
    Reprocess,
    Cancel,
    Close,
}
pub struct ScanPanel {
    pub profile: Profile,
    pub devices: Vec<Device>,
    pub capabilities: Option<Capabilities>,
    pub session: Option<Session>,
    pub presets: Vec<Profile>,
    pub selected_preset: Option<usize>,
    pub selected_page: usize,
    pub busy: bool,
    pub status: String,
    pub error: Option<String>,
    pub presets_only: bool,
    pub preview: Option<TextureHandle>,
    pub ocr_available: bool,
    pub advanced: bool,
    pub network_address: String,
    delete_armed: bool,
}
impl Default for ScanPanel {
    fn default() -> Self {
        Self {
            profile: Profile::default(),
            devices: Vec::new(),
            capabilities: None,
            session: None,
            presets: Vec::new(),
            selected_preset: None,
            selected_page: 0,
            busy: false,
            status: "Discovering scanners…".into(),
            error: None,
            presets_only: false,
            preview: None,
            ocr_available: openscan_core::ocr::available(),
            advanced: false,
            network_address: String::new(),
            delete_armed: false,
        }
    }
}
impl ScanPanel {
    pub fn new(presets_only: bool, session: Option<Session>) -> Self {
        Self {
            presets_only,
            session,
            ..Self::default()
        }
    }
    /// This draws only controls. The host executes returned actions outside painting.
    pub fn show(&mut self, ui: &mut Ui) -> Vec<Action> {
        let mut actions = Vec::new();
        ui.heading(if self.presets_only {
            "Manage scan presets"
        } else {
            "Scan documents"
        });
        ui.add_space(8.);
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
            ui.add_space(6.);
        }
        ui.horizontal(|ui| {
            if self.busy {
                ui.spinner();
            }
            ui.label(&self.status);
        });
        ui.separator();
        let wide = ui.available_width() >= 750. && !self.presets_only;
        if wide {
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical()
                    .id_salt("scan-settings")
                    .max_height((cols[0].available_height() - 56.).max(200.))
                    .show(&mut cols[0], |ui| self.settings(ui, &mut actions));
                self.pages(&mut cols[1], &mut actions);
            });
        } else {
            // At the 480-point minimum width the six scan actions wrap onto two
            // 30-point rows. Reserve both rows so Close stays outside the scroller.
            let footer_space = if self.presets_only || self.busy {
                64.
            } else {
                96.
            };
            egui::ScrollArea::vertical()
                .id_salt("scan-settings-narrow")
                .max_height((ui.available_height() - footer_space).max(150.))
                .show(ui, |ui| {
                    self.settings(ui, &mut actions);
                    if !self.presets_only {
                        ui.separator();
                        self.pages(ui, &mut actions);
                    }
                });
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if self.busy {
                if ui.button("Cancel operation").clicked() {
                    actions.push(Action::Cancel);
                }
            } else {
                if !self.presets_only {
                    if ui
                        .add_enabled(self.profile.device_id.is_some(), egui::Button::new("Scan"))
                        .clicked()
                    {
                        actions.push(Action::Scan);
                    }
                    if ui.button("Import images…").clicked() {
                        actions.push(Action::Import);
                    }
                    let ready = self
                        .session
                        .as_ref()
                        .is_some_and(|s| s.pages.iter().any(|p| !p.excluded));
                    if ui
                        .add_enabled(ready, egui::Button::new("Save PDF…"))
                        .clicked()
                    {
                        actions.push(Action::SavePdf);
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Append to PDF…"))
                        .clicked()
                    {
                        actions.push(Action::AppendPdf);
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Save images…"))
                        .clicked()
                    {
                        actions.push(Action::SaveImages);
                    }
                }
                if ui.button("Close").clicked() {
                    actions.push(Action::Close);
                }
            }
        });
        actions
    }
    fn settings(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.add_enabled_ui(!self.busy, |ui| {
            let preset_label = ui.label("Preset").id;
            let selected = self
                .selected_preset
                .and_then(|i| self.presets.get(i))
                .map(|p| p.name.as_str())
                .unwrap_or("Custom settings");
            egui::ComboBox::from_id_salt("scan-preset")
                .width(ui.available_width().min(340.))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (i, preset) in self.presets.iter().enumerate() {
                        if ui
                            .selectable_label(self.selected_preset == Some(i), &preset.name)
                            .clicked()
                        {
                            self.delete_armed = false;
                            actions.push(Action::LoadPreset(i));
                        }
                    }
                }).response.labelled_by(preset_label);
            let preset_name_label = ui.label("Preset name").id;
            ui.add(
                egui::TextEdit::singleline(&mut self.profile.name)
                    .desired_width(f32::INFINITY)
                    .char_limit(80),
            ).labelled_by(preset_name_label);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Save as new preset").clicked() {
                    actions.push(Action::SavePreset(false));
                }
                if ui
                    .add_enabled(
                        self.selected_preset.is_some(),
                        egui::Button::new("Update preset"),
                    )
                    .clicked()
                {
                    actions.push(Action::SavePreset(true));
                }
                if ui
                    .add_enabled(
                        self.selected_preset.is_some(),
                        egui::Button::new(if self.delete_armed {
                            "Confirm delete"
                        } else {
                            "Delete preset"
                        }),
                    )
                    .clicked()
                {
                    if self.delete_armed {
                        actions.push(Action::DeletePreset);
                        self.delete_armed = false;
                    } else {
                        self.delete_armed = true;
                    }
                }
            });
            ui.add_space(12.);
            let scanner_label = ui.label("Scanner").id;
            let current = self
                .devices
                .iter()
                .find(|d| Some(&d.id) == self.profile.device_id.as_ref())
                .map(|d| d.name.as_str())
                .unwrap_or("Select a scanner");
            egui::ComboBox::from_id_salt("scanner-device")
                .width(ui.available_width().min(340.))
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for device in &self.devices {
                        if ui
                            .selectable_label(
                                self.profile.device_id.as_deref() == Some(&device.id),
                                format!("{} ({})", device.name, device.driver),
                            )
                            .clicked()
                        {
                            actions.push(Action::SelectDevice(device.id.clone()));
                        }
                    }
                }).response.labelled_by(scanner_label);
            if ui.button("Refresh scanners").clicked() {
                actions.push(Action::Refresh);
            }
            if ui.button("Search network scanners").on_hover_text("Search using mDNS. Your operating system may ask for network access. You can also connect by scanner URL below.").clicked() {
                actions.push(Action::SearchNetwork);
            }
            egui::CollapsingHeader::new("Add network scanner by address").show(ui, |ui| {
                let address_label = ui.label("Scanner URL").id;
                ui.add(
                    egui::TextEdit::singleline(&mut self.network_address)
                        .hint_text("http://scanner.local/eSCL/")
                        .desired_width(f32::INFINITY),
                ).labelled_by(address_label);
                if ui.button("Connect").clicked() {
                    actions.push(Action::SelectDevice(format!(
                        "escl:{}",
                        self.network_address.trim()
                    )));
                }
            });
            ui.add_space(8.);
            let sources = self
                .capabilities
                .as_ref()
                .map(|c| c.sources.iter().map(|s| s.source).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![Source::Flatbed, Source::Feeder, Source::Duplex]);
            let source_label_id = ui.label("Source and sides").id;
            egui::ComboBox::from_id_salt("scanner-source")
                .selected_text(source_label(self.profile.source))
                .show_ui(ui, |ui| {
                    for source in sources {
                        ui.selectable_value(&mut self.profile.source, source, source_label(source));
                    }
                }).response.labelled_by(source_label_id);
            let color_label_id = ui.label("Color").id;
            egui::ComboBox::from_id_salt("scanner-color")
                .selected_text(color_label(self.profile.color))
                .show_ui(ui, |ui| {
                    for color in [Color::Color, Color::Grayscale, Color::BlackWhite] {
                        ui.selectable_value(&mut self.profile.color, color, color_label(color));
                    }
                }).response.labelled_by(color_label_id);
            let paper_label = ui.label("Paper size").id;
            egui::ComboBox::from_id_salt("scanner-paper")
                .selected_text(format!("{:?}", self.profile.paper))
                .show_ui(ui, |ui| {
                    for paper in [Paper::Letter, Paper::A4, Paper::Legal, Paper::A5] {
                        ui.selectable_value(&mut self.profile.paper, paper, format!("{paper:?}"));
                    }
                }).response.labelled_by(paper_label);
            ui.horizontal(|ui| {
                let label = ui.label("Resolution").id;
                ui.add(
                    egui::DragValue::new(&mut self.profile.dpi)
                        .range(75..=600)
                        .suffix(" DPI"),
                ).labelled_by(label);
            });
            ui.horizontal(|ui| {
                let label = ui.label("Maximum pages").id;
                ui.add(egui::DragValue::new(&mut self.profile.max_pages).range(1..=250)).labelled_by(label);
            });
            ui.checkbox(
                &mut self.profile.flip_duplex_backs,
                "Rotate duplex backs 180°",
            );
            ui.add_space(10.);
            ui.strong("Image correction");
            ui.checkbox(
                &mut self.profile.cleanup.deskew,
                "Straighten tilted pages (deskew)",
            );
            ui.checkbox(&mut self.profile.cleanup.auto_crop, "Crop to page content");
            ui.checkbox(
                &mut self.profile.cleanup.remove_background,
                "Whiten page background",
            );
            ui.checkbox(&mut self.profile.cleanup.denoise, "Remove speckles");
            ui.checkbox(&mut self.profile.cleanup.sharpen, "Sharpen text");
            ui.checkbox(
                &mut self.profile.cleanup.skip_blank_pages,
                "Exclude blank pages",
            );
            ui.add(
                egui::Slider::new(&mut self.profile.cleanup.brightness, -100..=100)
                    .text("Brightness"),
            );
            ui.add(
                egui::Slider::new(&mut self.profile.cleanup.contrast, -100..=100).text("Contrast"),
            );
            ui.horizontal(|ui| {
                let label = ui.label("Blank ink threshold").id;
                ui.add(
                    egui::DragValue::new(&mut self.profile.cleanup.blank_ink_percent)
                        .range(0.0..=5.0)
                        .speed(0.01)
                        .suffix("%"),
                ).labelled_by(label);
            });
            ui.add_space(10.);
            ui.strong("PDF output");
            ui.add_enabled(
                self.ocr_available,
                egui::Checkbox::new(
                    &mut self.profile.searchable_pdf,
                    "Make searchable (OCR)",
                ),
            );
            if !self.ocr_available {
                ui.small("Install the free OCR models to enable searchable PDFs.");
            }
            ui.add(
                egui::Slider::new(&mut self.profile.jpeg_quality, 20..=100).text("Image quality"),
            );
        });
    }
    fn pages(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let count = self.session.as_ref().map_or(0, |s| s.pages.len());
        ui.strong(format!("Pages ({count})"));
        if count == 0 {
            ui.label("Scan or import pages to preview them here.");
            return;
        }
        self.selected_page = self.selected_page.min(count - 1);
        ui.horizontal_wrapped(|ui| {
            let page_label = ui.label("Page").id;
            ui.add(
                egui::DragValue::new(&mut self.selected_page)
                    .range(0..=count - 1)
                    .custom_formatter(|v, _| format!("{}", v as usize + 1))
                    .custom_parser(|s| s.parse::<usize>().ok().map(|v| v.saturating_sub(1) as f64)),
            )
            .labelled_by(page_label);
            ui.add_enabled_ui(!self.busy, |ui| {
                for (label, action) in [
                    ("Rotate left", Action::Rotate(-90)),
                    ("Rotate right", Action::Rotate(90)),
                    ("Move earlier", Action::Move(-1)),
                    ("Move later", Action::Move(1)),
                    ("Include / exclude", Action::TogglePage),
                    ("Apply corrections", Action::Reprocess),
                ] {
                    if ui.button(label).clicked() {
                        actions.push(action);
                    }
                }
            });
        });
        if let Some(page) = self
            .session
            .as_ref()
            .and_then(|s| s.pages.get(self.selected_page))
        {
            ui.label(format!(
                "{} • {} DPI • {:.2}% ink • deskew {:.2}°",
                if page.excluded {
                    "Excluded"
                } else {
                    "Included"
                },
                page.dpi,
                page.ink_percent,
                page.deskew_angle
            ));
        }
        if let Some(texture) = &self.preview {
            let size = texture.size_vec2();
            let available = egui::vec2(
                ui.available_width().max(30.),
                (ui.available_height() - 50.).max(80.),
            );
            let scale = (available.x / size.x).min(available.y / size.y).min(1.);
            ui.image((texture.id(), size * scale));
        }
        ui.small("Original scans are kept in the recovery session. Corrections can be reapplied.");
    }
}
fn source_label(source: Source) -> &'static str {
    match source {
        Source::Flatbed => "Flatbed",
        Source::Feeder => "Feeder — one side",
        Source::Duplex => "Feeder — both sides",
    }
}
fn color_label(color: Color) -> &'static str {
    match color {
        Color::Color => "Color",
        Color::Grayscale => "Grayscale",
        Color::BlackWhite => "Black and white",
    }
}

/// Matches PdfCraft's outlined 30-point dialog controls; the host supplies its palette.
pub fn dialog_style(ui: &mut Ui) {
    let visuals = ui.visuals().clone();
    let foreground = visuals.widgets.inactive.fg_stroke;
    craft_ui::theme::WidgetPalette {
        noninteractive: visuals.widgets.noninteractive.bg_fill,
        inactive: visuals.widgets.inactive.bg_fill,
        hovered: visuals.widgets.hovered.bg_fill,
        active: visuals.widgets.active.bg_fill,
        open: visuals.widgets.open.bg_fill,
    }
    .apply_to(
        &mut ui.visuals_mut().widgets,
        egui::CornerRadius::same(4),
        foreground,
    );
    ui.spacing_mut().interact_size.y = 30.;
}
