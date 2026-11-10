use egui::{Button, Id, InnerResponse, Popup, Response, Ui, WidgetText};

/// A default command beside a separately focusable alternatives menu.
///
/// Both halves use egui buttons. Tab, Enter/Space, focus indication, and menu
/// dismissal are handled by egui. The application handles the primary action
/// only when `output.primary.clicked()` is true.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// use egui_office_widgets::SplitButton;
/// let output = SplitButton::new(egui::Id::unique("scan"), "Scan")
///     .show(ui, |ui| ui.button("Scan settings"));
/// if output.primary.clicked() {
///     // Dispatch the default scan command.
/// }
/// # });
/// ```
#[must_use = "Call show to render the control"]
pub struct SplitButton {
    id: Id,
    label: WidgetText,
    menu_label: String,
    primary_enabled: bool,
    menu_enabled: bool,
}

/// Responses from the two independent actions and the optional open menu.
pub struct SplitButtonResponse<R> {
    /// Use this response to dispatch the default command.
    pub primary: Response,
    /// The separately focusable menu trigger.
    pub menu_button: Response,
    /// Content returned while the alternatives menu is open.
    pub menu: Option<InnerResponse<R>>,
}

impl SplitButton {
    /// Create a control with a stable caller-owned ID and visible command label.
    pub fn new(id: Id, label: impl Into<WidgetText>) -> Self {
        let label = label.into();
        Self {
            id,
            menu_label: format!("{} options", label.text()),
            label,
            primary_enabled: true,
            menu_enabled: true,
        }
    }

    /// Set the menu trigger's accessible name and tooltip.
    pub fn menu_label(mut self, label: impl Into<String>) -> Self {
        self.menu_label = label.into();
        self
    }

    /// Enable the default action independently of the menu.
    pub fn primary_enabled(mut self, enabled: bool) -> Self {
        self.primary_enabled = enabled;
        self
    }

    /// Enable the menu independently of the default action.
    pub fn menu_enabled(mut self, enabled: bool) -> Self {
        self.menu_enabled = enabled;
        self
    }

    /// Render the control. A disabled parent disables both halves.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> SplitButtonResponse<R> {
        ui.push_id(self.id, |ui| {
            let (primary, menu_button) = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    let primary = ui.add_enabled(self.primary_enabled, Button::new(self.label));
                    let menu_button = menu_trigger(ui, self.menu_enabled, &self.menu_label);
                    (primary, menu_button)
                })
                .inner;

            let popup_id = Popup::default_response_id(&menu_button);
            let menu = if menu_button.enabled() {
                Popup::menu(&menu_button)
                    .accessible_name(self.menu_label)
                    .show(contents)
            } else {
                Popup::close_id(ui.ctx(), popup_id);
                None
            };

            SplitButtonResponse {
                primary,
                menu_button,
                menu,
            }
        })
        .inner
    }
}

// Paint the decoration so no optional font or Unicode glyph is required.
pub(crate) fn menu_trigger(ui: &mut Ui, enabled: bool, name: &str) -> Response {
    let side = ui.spacing().interact_size.y;
    let response = ui
        .add_enabled(enabled, Button::new("").min_size(egui::vec2(side, side)))
        .accessible_name(name)
        .on_hover_text(name);
    let center = response.rect.center();
    let color = if response.enabled() {
        ui.style().interact(&response).fg_stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-4.0, -2.0),
            center + egui::vec2(4.0, -2.0),
            center + egui::vec2(0.0, 3.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
    response
}
