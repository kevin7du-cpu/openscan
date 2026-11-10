//! View-only zoom. Image correction and exported pixels are independent of this state.
use egui::{
    ScrollArea, TextureHandle, Ui, Vec2,
    scroll_area::{DragScroll, ScrollSource},
};

#[derive(Default)]
pub(crate) struct Preview {
    source: Vec2,
    viewport: Vec2,
    /// None follows the viewport size; Some is a ratio of original image pixels.
    zoom: Option<f32>,
    reset_scroll: bool,
}

impl Preview {
    pub fn set_source(&mut self, size: [u32; 2]) {
        self.source = egui::vec2(size[0] as f32, size[1] as f32);
        self.reset_scroll = true;
    }

    fn scale(&self) -> f32 {
        self.zoom.unwrap_or_else(|| {
            (self.viewport.x / self.source.x.max(1.0))
                .min(self.viewport.y / self.source.y.max(1.0))
                .clamp(0.0001, 1.0)
        })
    }

    pub fn controls(&mut self, ui: &mut Ui, enabled: bool) {
        ui.add_enabled_ui(enabled, |ui| {
            ui.spacing_mut().slider_width = 110.0;
            // Right-to-left lays out Fit at the outer corner, like a document viewer.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Fit")
                    .accessible_name("Fit scanned page")
                    .on_hover_text("Fit the whole page in the preview")
                    .clicked()
                {
                    self.zoom = None;
                    self.reset_scroll = true;
                }
                let mut percent = self.scale() * 100.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut percent)
                            .range(1.0..=400.0)
                            .clamp_existing_to_range(false)
                            .suffix("%")
                            .fixed_decimals(0)
                            .speed(1.0),
                    )
                    .accessible_name("Scan preview zoom percent")
                    .on_hover_text(
                        "Double-click to enter a percentage; 100% shows original pixel size",
                    )
                    .changed()
                {
                    self.zoom = Some(percent / 100.0);
                }
                if ui
                    .button("+")
                    .accessible_name("Zoom in scanned page")
                    .clicked()
                {
                    self.zoom = Some((self.scale() * 1.25).clamp(0.01, 4.0));
                }
                let mut percent = self.scale() * 100.0;
                if ui
                    .add(
                        egui::Slider::new(&mut percent, 1.0..=400.0)
                            .logarithmic(true)
                            .show_value(false)
                            .clamping(egui::SliderClamping::Never),
                    )
                    .accessible_name("Scan preview zoom slider")
                    .changed()
                {
                    self.zoom = Some((percent / 100.0).clamp(0.01, 4.0));
                }
                if ui
                    .button("−")
                    .accessible_name("Zoom out scanned page")
                    .clicked()
                {
                    self.zoom = Some((self.scale() / 1.25).clamp(0.01, 4.0));
                }
                ui.label("Zoom");
            });
        });
    }

    pub fn show(&mut self, ui: &mut Ui, texture: &TextureHandle) -> egui::Response {
        self.show_selectable(ui, texture, false)
    }

    pub fn show_selectable(
        &mut self,
        ui: &mut Ui,
        texture: &TextureHandle,
        selecting: bool,
    ) -> egui::Response {
        // Keep the available size independent of image size, even at 400%.
        self.viewport = (ui.available_size() - Vec2::splat(2.0)).max(Vec2::splat(1.0));
        let size = self.source * self.scale();
        let mut scroll = ScrollArea::both()
            .id_salt("scan-preview-scroll")
            .auto_shrink([false, false])
            .max_width(ui.available_width())
            .max_height(ui.available_height())
            .scroll_source(ScrollSource {
                drag: if selecting {
                    DragScroll::Never
                } else {
                    DragScroll::Always
                },
                ..Default::default()
            });
        if std::mem::take(&mut self.reset_scroll) {
            scroll = scroll.scroll_offset(Vec2::ZERO);
        }
        scroll
            .show(ui, |ui| {
                let canvas = size.max(self.viewport);
                let (rect, _) = ui.allocate_exact_size(canvas, egui::Sense::hover());
                let rect = egui::Rect::from_center_size(rect.center(), size);
                ui.put(
                    rect,
                    egui::Image::new(texture)
                        .fit_to_exact_size(size)
                        .sense(if selecting {
                            egui::Sense::drag()
                        } else {
                            egui::Sense::click()
                        }),
                )
                .accessible_name("Scanned page preview")
            })
            .inner
    }
}
