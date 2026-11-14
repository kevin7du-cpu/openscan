//! Compact correction settings; storage stays on the scanner worker.
use super::{LegacyOpenScanUi, enum_choice, worker::Command};
use scan_module::CleanupOptions;

impl LegacyOpenScanUi {
    pub(super) fn correction_window(&mut self, ctx: &egui::Context) {
        let Some(options) = &mut self.correction_editor else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Image correction")
            .id(egui::Id::unique(("image-correction", &self.instance)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(370.0)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| {
                    controls(ui, options);
                    ui.separator();
                    ui.label("Remember these choices for future scans and imports. Use Apply corrections to update current pages.");
                    ui.horizontal_wrapped(|ui| {
                        save = ui.button("Save settings").clicked();
                        cancel = ui.button("Cancel").clicked();
                        if ui.button("Restore defaults").clicked() { *options = CleanupOptions::default(); }
                    });
                });
                if let Some(error) = &self.error { ui.colored_label(egui::Color32::DARK_RED, error); }
            });
        if !self.busy {
            if save {
                let options = options.clone();
                self.submit(
                    Command::SaveCorrections(options),
                    "Saving image correction settings",
                );
            } else if !open || cancel {
                self.correction_editor = None;
            }
        }
    }
}

pub(super) fn controls(ui: &mut egui::Ui, options: &mut CleanupOptions) {
    ui.label("Black & White scans use Balanced cleanup.")
        .on_hover_text("Reduces paper texture and show-through before removing speckles. Use Grayscale or Color to retain subtle shading.");
    ui.checkbox(&mut options.deskew, "Straighten pages (deskew)");
    ui.checkbox(&mut options.auto_crop, "Crop to content");
    ui.checkbox(&mut options.remove_background, "Whiten uneven background");
    ui.checkbox(&mut options.denoise, "Remove dust / speckles");
    ui.checkbox(&mut options.sharpen, "Sharpen text");
    ui.checkbox(
        &mut options.skip_blank_pages,
        "Exclude blank pages (reversible)",
    );
    ui.horizontal(|ui| {
        ui.label("Blank ink %");
        ui.add(
            egui::DragValue::new(&mut options.blank_ink_percent)
                .range(0.0..=5.0)
                .speed(0.01)
                .fixed_decimals(2),
        )
        .accessible_name("Blank ink percent");
    });
    ui.horizontal(|ui| {
        ui.label("Brightness");
        ui.add(egui::DragValue::new(&mut options.brightness).range(-100..=100))
            .accessible_name("Brightness");
    });
    ui.horizontal(|ui| {
        ui.label("Contrast");
        ui.add(egui::DragValue::new(&mut options.contrast).range(-100..=100))
            .accessible_name("Contrast");
    });
    enum_choice(
        ui,
        "Rotate clockwise",
        &mut options.rotate_degrees,
        &[0, 90, 180, 270],
    );
}

pub(super) fn settings_button(ui: &mut egui::Ui) -> egui::Response {
    // Paint a vector gear so its appearance does not depend on platform fonts.
    let response = ui
        .button("     Image correction")
        .accessible_name("Image correction")
        .on_hover_text("Adjust and save image correction settings for future scans.");
    let scale = ui.text_style_height(&egui::TextStyle::Button) / 14.0;
    let center = egui::pos2(
        response.rect.left() + 11.0 * scale,
        response.rect.center().y,
    );
    let stroke = ui.style().interact(&response).fg_stroke;
    let points = (0..32)
        .map(|i| {
            let radius = if i % 4 < 2 { 7.0 } else { 5.0 };
            center + egui::Vec2::angled(i as f32 * std::f32::consts::TAU / 32.0) * radius * scale
        })
        .collect();
    ui.painter().add(egui::Shape::closed_line(points, stroke));
    ui.painter().circle_stroke(center, 2.3 * scale, stroke);
    response
}
