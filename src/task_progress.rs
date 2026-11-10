//! Accessible task progress: measured fractions or an indeterminate moving bar.
use egui::{Context, Id, Response, Role, Sense, Ui, Widget, WidgetInfo};

/// A measured fraction, or `None` for work whose total is unknown.
pub struct TaskProgress(pub Option<f32>);

impl Widget for TaskProgress {
    fn ui(self, ui: &mut Ui) -> Response {
        if let Some(fraction) = self.0.filter(|v| v.is_finite()) {
            return ui.add(egui::ProgressBar::new(fraction).show_percentage());
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(96.0), 18.0),
            Sense::hover(),
        );
        response.widget_info(|| {
            WidgetInfo::labeled(
                Role::ProgressIndicator,
                true,
                "Working; progress total unknown",
            )
        });
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, 5.0, ui.visuals().extreme_bg_color);
            let phase = ui.input(|i| (i.time * 1.5).sin() as f32 * 0.5 + 0.5);
            let width = rect.width() * 0.25;
            let segment = egui::Rect::from_min_size(
                rect.min + egui::vec2((rect.width() - width) * phase, 0.0),
                egui::vec2(width, rect.height()),
            );
            ui.painter()
                .rect_filled(segment, 5.0, ui.visuals().selection.bg_fill);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
        response
    }
}

/// The caller shows this only while its background worker is active.
/// Returns true when cancellation is requested; never claims an unknown percent.
pub fn task_progress_popup(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    status: &str,
    fraction: Option<f32>,
) -> bool {
    let mut cancel = false;
    egui::Window::new("Please wait")
        .id(Id::unique(id))
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(status);
            ui.add(TaskProgress(fraction));
            cancel = ui
                .button("Cancel operation")
                .on_hover_text("Request cancellation after the current processing step.")
                .clicked();
        });
    cancel
}
