use egui_kittest::{Harness, kittest::Queryable};
use openscan_ui::{Action, ScanPanel};

#[test]
fn explicit_scan_and_cancel_actions_do_not_start_automatically() {
    let mut panel = ScanPanel::new(false, None);
    panel.profile.device_id = Some("sane:synthetic".into());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1040., 900.))
        .build_ui_state(
            |ui, state: &mut (ScanPanel, Vec<Action>)| state.1.extend(state.0.show(ui)),
            (panel, Vec::new()),
        );
    h.run();
    assert!(h.state().1.is_empty());
    h.get_by_label("Scan").click();
    h.run();
    assert!(matches!(h.state().1.last(), Some(Action::Scan)));
    h.state_mut().0.busy = true;
    h.step();
    h.get_by_label("Cancel operation").click();
    h.step();
    assert!(matches!(h.state().1.last(), Some(Action::Cancel)));
}

#[test]
fn preset_and_scan_panels_render_at_narrow_and_wide_sizes() {
    for presets in [false, true] {
        for width in [480., 1040.] {
            let panel = ScanPanel::new(presets, None);
            let mut h = Harness::builder()
                .with_size(egui::vec2(width, 820.))
                .build_ui_state(
                    |ui, panel| {
                        panel.show(ui);
                    },
                    panel,
                );
            h.run();
            h.get_by_label(if presets {
                "Manage scan presets"
            } else {
                "Scan documents"
            });
            h.get_by_label("Close");
        }
    }
}

#[test]
fn narrow_scan_footer_keeps_close_completely_inside_the_window() {
    for height in [450., 820., 900.] {
        let panel = ScanPanel::new(false, None);
        let mut h = Harness::builder()
            .with_size(egui::vec2(480., height))
            .build_ui_state(
                |ui, state: &mut (ScanPanel, Vec<Action>)| {
                    openscan_ui::dialog_style(ui);
                    state.1.extend(state.0.show(ui));
                },
                (panel, Vec::new()),
            );
        h.run();
        let close = h.get_by_label("Close").rect();
        assert!(
            close.bottom() <= height,
            "Close is clipped at height {height}: {close:?}"
        );
        h.get_by_label("Close").click();
        h.run();
        assert!(matches!(h.state().1.last(), Some(Action::Close)));
    }
}
