//! Interactive review of reusable office widgets; all actions are demonstrations.

use egui::{Color32, Id};
use egui_office_widgets::{
    Choice, ChoiceCommit, ColorSwatchPalette, EditableChoice, RibbonGroup, SplitButton, Swatch,
};

const SIZES: [Choice<'static>; 5] = [
    Choice {
        id: "small",
        label: "10 pt",
        value: "10",
        enabled: true,
    },
    Choice {
        id: "normal",
        label: "11 pt",
        value: "11",
        enabled: true,
    },
    Choice {
        id: "body",
        label: "12 pt",
        value: "12",
        enabled: true,
    },
    Choice {
        id: "large",
        label: "18 pt",
        value: "18",
        enabled: true,
    },
    Choice {
        id: "unavailable",
        label: "Reserved size",
        value: "20",
        enabled: false,
    },
];

const COLORS: [Swatch<'static>; 8] = [
    Swatch {
        id: "ocean",
        label: "Ocean",
        color: Color32::from_rgb(46, 124, 180),
        enabled: true,
    },
    Swatch {
        id: "forest",
        label: "Forest",
        color: Color32::from_rgb(54, 140, 96),
        enabled: true,
    },
    Swatch {
        id: "amber",
        label: "Amber",
        color: Color32::from_rgb(210, 150, 45),
        enabled: true,
    },
    Swatch {
        id: "rose",
        label: "Rose",
        color: Color32::from_rgb(190, 85, 105),
        enabled: true,
    },
    Swatch {
        id: "violet",
        label: "Violet",
        color: Color32::from_rgb(135, 99, 180),
        enabled: true,
    },
    Swatch {
        id: "slate",
        label: "Slate",
        color: Color32::from_rgb(120, 135, 150),
        enabled: true,
    },
    Swatch {
        id: "paper",
        label: "Paper",
        color: Color32::from_rgb(225, 222, 210),
        enabled: true,
    },
    Swatch {
        id: "locked",
        label: "Unavailable color",
        color: Color32::from_rgb(80, 80, 80),
        enabled: false,
    },
];

struct Gallery {
    disabled: bool,
    draft: String,
    committed: String,
    color: Option<String>,
    activity: Vec<String>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            disabled: false,
            draft: "1".into(),
            committed: "No value committed".into(),
            color: Some("ocean".into()),
            activity: vec!["Gallery ready. These actions do not scan or modify documents.".into()],
        }
    }
}

impl Gallery {
    fn record(&mut self, text: impl Into<String>) {
        self.activity.insert(0, text.into());
        self.activity.truncate(8);
    }
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Office widget gallery");
                ui.label(egui::RichText::new("egui 0.37").weak());
            });
            ui.label("Reusable controls for document and office applications");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let mut theme = ui.options(|options| options.theme_preference);
                ui.selectable_value(&mut theme, egui::ThemePreference::System, "System");
                ui.selectable_value(&mut theme, egui::ThemePreference::Dark, "Dark");
                ui.selectable_value(&mut theme, egui::ThemePreference::Light, "Light");
                ui.ctx().set_theme(theme);
                ui.separator();
                ui.checkbox(&mut self.disabled, "Disable demo controls");
            });
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_enabled_ui(!self.disabled, |ui| {
                    ui.heading("1. Split command button");
                    ui.label("The main button runs the default action. The arrow opens alternatives.");
                    let scan = SplitButton::new(Id::unique("scan"), "Scan")
                        .show(ui, |ui| {
                            let mut action = None;
                            if ui.button("Scan both sides").clicked() { action = Some("Duplex scan requested (demo)"); ui.close(); }
                            if ui.button("Choose scanner").clicked() { action = Some("Scanner selection requested (demo)"); ui.close(); }
                            action
                        });
                    if scan.primary.clicked() { self.record("Default scan requested (demo)"); }
                    if let Some(action) = scan.menu.and_then(|menu| menu.inner) { self.record(action); }
                    ui.add_space(16.0);

                    ui.heading("2. Editable choice");
                    ui.label("Type 1 and press Enter to commit 1. Use an arrow key to deliberately select a suggestion.");
                    let choice = EditableChoice::new(Id::unique("size"), "Font size", &mut self.draft, &SIZES).show(ui);
                    if let Some(commit) = choice.commit {
                        self.committed = match commit {
                            ChoiceCommit::Typed(value) => format!("Typed value: {value:?}"),
                            ChoiceCommit::Selected { id, value } => format!("Selected {id}: {value:?}"),
                        };
                        self.record(self.committed.clone());
                    }
                    ui.label(&self.committed);
                    ui.add_space(16.0);

                    ui.heading("3. Ribbon groups");
                    ui.label("Groups grow with content and return the actions produced by their controls.");
                    let mut action = None;
                    ui.scope_builder(egui::UiBuilder::new().layout(egui::Layout::left_to_right(egui::Align::Min)), |ui| {
                        RibbonGroup::new(Id::unique("capture"), "Capture").show(ui, |ui| {
                            if ui.button("Import PDF").clicked() { action = Some("Import requested (demo)"); }
                            if ui.button("Import images").clicked() { action = Some("Image import requested (demo)"); }
                        });
                        RibbonGroup::new(Id::unique("organize"), "Organize").show(ui, |ui| {
                            if ui.button("New folder").clicked() { action = Some("New folder requested (demo)"); }
                            if ui.button("Edit metadata").clicked() { action = Some("Metadata editor requested (demo)"); }
                            if ui.button("Review history").clicked() { action = Some("History requested (demo)"); }
                        });
                    });
                    if let Some(action) = action { self.record(action); }
                    ui.add_space(16.0);

                    ui.heading("4. Named color palette");
                    ui.label("Tab to a color; arrows move focus. Enter/Space selects. Home/End jump to the ends.");
                    let palette = ColorSwatchPalette::new(Id::unique("colors"), &COLORS, self.color.as_deref()).columns(4).show(ui);
                    if let Some(color) = palette.picked {
                        self.record(format!("Color selected: {color}"));
                        self.color = Some(color);
                    }
                    ui.label(format!("Selected: {}", self.color.as_deref().unwrap_or("none")));
                });
                ui.add_space(12.0);
                ui.separator();
                ui.heading("Activity");
                for event in &self.activity { ui.label(event); }
            });
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([920.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Office widget gallery",
        options,
        Box::new(|_| Ok(Box::new(Gallery::default()))),
    )
}
