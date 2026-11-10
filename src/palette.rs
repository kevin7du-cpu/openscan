use egui::{Button, Color32, Grid, Id, Key, Modifiers, Response, Stroke, StrokeKind, Ui, vec2};

/// A named color with a stable identity independent of its position.
#[derive(Clone, Copy, Debug)]
pub struct Swatch<'a> {
    /// Unique stable ID within a palette.
    pub id: &'a str,
    /// Human-readable accessible name.
    pub label: &'a str,
    /// Color displayed by the control.
    pub color: Color32,
    /// Whether selection is allowed.
    pub enabled: bool,
}

/// Palette layout response and an explicitly selected stable swatch ID.
pub struct PaletteResponse {
    /// Response covering the palette.
    pub response: Response,
    /// Selected ID, or none if no activation happened this frame.
    pub picked: Option<String>,
}

/// A named color grid using standard, focusable egui buttons.
///
/// Tab visits enabled swatches. Arrow keys move focus without changing the
/// selection; Enter/Space selects. Home/End focus the first/last enabled swatch.
#[must_use = "Call show to render the palette"]
pub struct ColorSwatchPalette<'a> {
    id: Id,
    swatches: &'a [Swatch<'a>],
    selected: Option<&'a str>,
    columns: usize,
}

impl<'a> ColorSwatchPalette<'a> {
    /// Create a palette. The application owns selection state.
    pub fn new(id: Id, swatches: &'a [Swatch<'a>], selected: Option<&'a str>) -> Self {
        Self {
            id,
            swatches,
            selected,
            columns: 8,
        }
    }

    /// Set the number of columns. Zero is treated as one.
    pub fn columns(mut self, columns: usize) -> Self {
        self.columns = columns.clamp(1, self.swatches.len().max(1));
        self
    }

    /// Render and return a picked ID; this never changes application data itself.
    pub fn show(self, ui: &mut Ui) -> PaletteResponse {
        let mut picked = None;
        let mut responses = Vec::with_capacity(self.swatches.len());
        let grid = Grid::new(self.id).show(ui, |ui| {
            for (index, swatch) in self.swatches.iter().enumerate() {
                let selected = self.selected == Some(swatch.id);
                let response = ui
                    .push_id(swatch.id, |ui| {
                        ui.add_enabled(
                            swatch.enabled,
                            Button::selectable(selected, "").min_size(vec2(40.0, 30.0)),
                        )
                        .accessible_name(swatch.label)
                        .on_hover_text(format!(
                            "{} (#{:02X}{:02X}{:02X})",
                            swatch.label,
                            swatch.color.r(),
                            swatch.color.g(),
                            swatch.color.b()
                        ))
                    })
                    .inner;
                let rect = response.rect.shrink(5.0);
                let color = if response.enabled() {
                    swatch.color
                } else {
                    swatch.color.gamma_multiply(0.35)
                };
                ui.painter().rect_filled(rect, 2.0, color);
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    ui.visuals().widgets.noninteractive.bg_stroke,
                    StrokeKind::Inside,
                );
                if selected {
                    let light = u32::from(color.r()) * 299
                        + u32::from(color.g()) * 587
                        + u32::from(color.b()) * 114
                        > 128_000;
                    let ink = if light {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                    let center = rect.center();
                    ui.painter().line_segment(
                        [center + vec2(-5.0, 0.0), center + vec2(-1.0, 4.0)],
                        Stroke::new(2.0, ink),
                    );
                    ui.painter().line_segment(
                        [center + vec2(-1.0, 4.0), center + vec2(6.0, -4.0)],
                        Stroke::new(2.0, ink),
                    );
                }
                if response.clicked() {
                    picked = Some(swatch.id.to_owned());
                }
                responses.push(response);
                if (index + 1) % self.columns == 0 {
                    ui.end_row();
                }
            }
        });
        if let Some(current) = responses
            .iter()
            .position(|response| response.has_focus() && response.enabled())
        {
            for key in [
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
                Key::Home,
                Key::End,
            ] {
                if ui.input_mut(|input| input.consume_key(Modifiers::NONE, key)) {
                    if let Some(next) = neighbor(self.swatches, current, self.columns, key)
                        && let Some(response) = responses.get(next)
                    {
                        response.request_focus();
                    }
                    break;
                }
            }
        }
        PaletteResponse {
            response: grid.response,
            picked,
        }
    }
}

fn neighbor(swatches: &[Swatch<'_>], current: usize, columns: usize, key: Key) -> Option<usize> {
    if key == Key::Home {
        return swatches.iter().position(|swatch| swatch.enabled);
    }
    if key == Key::End {
        return swatches.iter().rposition(|swatch| swatch.enabled);
    }
    let step = match key {
        Key::ArrowLeft => -1,
        Key::ArrowRight => 1,
        Key::ArrowUp => -(columns as isize),
        Key::ArrowDown => columns as isize,
        _ => return None,
    };
    let mut next = current.checked_add_signed(step)?;
    while let Some(swatch) = swatches.get(next) {
        if matches!(key, Key::ArrowLeft | Key::ArrowRight) && next / columns != current / columns {
            return None;
        }
        if swatch.enabled {
            return Some(next);
        }
        next = next.checked_add_signed(step)?;
    }
    None
}
