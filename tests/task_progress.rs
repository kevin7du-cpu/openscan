//! Progress accessibility, measured values and wait-popup cancellation.
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

#[test]
fn wait_popup_supports_unknown_measured_progress_and_cancellation() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(640., 480.))
        .build_ui_state(
            |ui, state: &mut (Option<f32>, bool)| {
                state.1 = egui_office_widgets::task_progress_popup(
                    ui.ctx(),
                    "test-task",
                    "Extracting boxed text",
                    state.0,
                );
            },
            (None, false),
        );
    h.step();
    h.get_by_label("Please wait");
    h.get_by_label("Working; progress total unknown");
    assert_eq!(
        h.get_by_label("Working; progress total unknown")
            .accesskit_node()
            .numeric_value(),
        None
    );
    h.get_by_label("Cancel operation").click();
    h.step();
    assert!(h.state().1);
    h.state_mut().0 = Some(0.5);
    h.step();
    assert_eq!(
        h.get_by_role(egui::accesskit::Role::ProgressIndicator)
            .accesskit_node()
            .numeric_value(),
        Some(50.0)
    );
    assert!(
        h.query_by_label("Working; progress total unknown")
            .is_none()
    );
}
