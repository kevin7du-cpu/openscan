use egui::{Frame, Id, InnerResponse, Ui};

/// A titled group of existing widgets that grows to fit its contents.
///
/// Application commands, icons, ribbon tabs, and keyboard accelerators remain
/// outside this layout helper. No fixed content height clips tall controls.
#[must_use = "Call show to render the group"]
pub struct RibbonGroup<'a> {
    id: Id,
    title: &'a str,
}

impl<'a> RibbonGroup<'a> {
    /// Construct a group with a stable ID and a visible footer caption.
    pub fn new(id: Id, title: &'a str) -> Self {
        Self { id, title }
    }

    /// Render arbitrary content above the group's caption.
    pub fn show<R>(self, ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        ui.push_id(self.id, |ui| {
            Frame::group(ui.style()).show(ui, |ui| {
                ui.vertical(|ui| {
                    let result = contents(ui);
                    ui.shrink_width_to_current();
                    ui.separator();
                    ui.small(self.title);
                    result
                })
                .inner
            })
        })
        .inner
    }
}
