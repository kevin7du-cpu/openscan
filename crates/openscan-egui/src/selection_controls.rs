// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared selection colors, retaining egui's interaction and accessibility behavior.

use egui::{Color32, IntoAtoms, Response, Stroke, Ui};

const BLUE: Color32 = Color32::from_rgb(0, 103, 192);

fn configure(ui: &mut Ui, selected: bool, round: bool) {
    let label_color = ui.visuals().text_color();
    let visuals = &mut ui.style_mut().visuals;
    // The selection mark has its own color; labels retain their existing ink.
    visuals.override_text_color = Some(label_color);
    for state in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        state.bg_fill = if selected && !round {
            crate::soft_3d::CHECKED_FILL
        } else {
            crate::soft_3d::FIELD_FILL
        };
        state.weak_bg_fill = state.bg_fill;
        state.bg_stroke = Stroke::new(1.0, if selected { BLUE } else { crate::soft_3d::EDGE });
        state.fg_stroke.color = if selected && !round {
            Color32::WHITE
        } else {
            BLUE
        };
    }
    // Hover and keyboard focus change the outline, not the selected value.
    visuals.widgets.hovered.bg_stroke.color = BLUE;
    visuals.widgets.active.bg_stroke.color = BLUE;
}

pub(crate) fn checkbox<'a>(
    ui: &mut Ui,
    checked: &'a mut bool,
    label: impl IntoAtoms<'a>,
) -> Response {
    ui.scope(|ui| {
        configure(ui, *checked, false);
        ui.checkbox(checked, label)
    })
    .inner
}

pub(crate) fn checkbox_enabled<'a>(
    ui: &mut Ui,
    enabled: bool,
    checked: &'a mut bool,
    label: impl IntoAtoms<'a>,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| checkbox(ui, checked, label))
        .inner
}

pub(crate) fn radio_value<'a, T: PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    alternative: T,
    label: impl IntoAtoms<'a>,
) -> Response {
    ui.scope(|ui| {
        configure(ui, *current == alternative, true);
        ui.radio_value(current, alternative, label)
    })
    .inner
}
