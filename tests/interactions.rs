//! Behavior regressions exercised through egui's actual input and accessibility tree.

use egui::{Color32, Id, Key};
use egui_kittest::{Harness, kittest::Queryable};
use egui_office_widgets::{
    Choice, ChoiceCommit, ColorSwatchPalette, EditableChoice, RibbonGroup, SplitButton, Swatch,
};

#[derive(Default)]
struct SplitState {
    primary: usize,
    alternate: usize,
    disabled: bool,
    disable_menu: bool,
}

fn split_ui(ui: &mut egui::Ui, state: &mut SplitState) {
    ui.add_enabled_ui(!state.disabled, |ui| {
        let result = SplitButton::new(Id::unique("scan"), "Scan")
            .menu_enabled(!state.disable_menu)
            .show(ui, |ui| {
                if ui.button("Duplex").clicked() {
                    state.alternate += 1;
                    ui.close();
                }
            });
        if result.primary.clicked() {
            state.primary += 1;
        }
    });
}

#[test]
fn split_primary_does_not_open_menu() {
    let mut harness = Harness::new_ui_state(split_ui, SplitState::default());
    harness.get_by_label("Scan").click();
    harness.run();
    assert_eq!(harness.state().primary, 1);
    assert!(harness.query_by_label("Duplex").is_none());
}

#[test]
fn split_menu_does_not_run_primary() {
    let mut harness = Harness::new_ui_state(split_ui, SplitState::default());
    harness.get_by_label("Scan options").click();
    harness.run();
    assert_eq!(harness.state().primary, 0);
    harness.get_by_label("Duplex").click();
    harness.run();
    assert_eq!(harness.state().alternate, 1);
    assert_eq!(harness.state().primary, 0);
}

#[test]
fn split_menu_can_be_opened_and_dismissed_by_keyboard() {
    let mut harness = Harness::new_ui_state(split_ui, SplitState::default());
    harness.get_by_label("Scan options").focus();
    harness.run();
    harness.key_press(Key::Space);
    harness.run();
    assert!(harness.query_by_label("Duplex").is_some());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Duplex").is_none());
    assert_eq!(harness.state().primary, 0);
}

#[test]
fn disabled_parent_blocks_both_actions() {
    let mut harness = Harness::new_ui_state(
        split_ui,
        SplitState {
            disabled: true,
            ..Default::default()
        },
    );
    harness.get_by_label("Scan").click();
    harness.get_by_label("Scan options").click();
    harness.run();
    assert_eq!(harness.state().primary, 0);
    assert!(harness.query_by_label("Duplex").is_none());
}

#[test]
fn disabling_open_menu_closes_it() {
    let mut harness = Harness::new_ui_state(split_ui, SplitState::default());
    harness.get_by_label("Scan options").click();
    harness.run();
    harness.state_mut().disable_menu = true;
    harness.run();
    assert!(harness.query_by_label("Duplex").is_none());
}

#[test]
fn equally_labeled_split_buttons_keep_separate_menus() {
    let mut harness = Harness::new_ui(|ui| {
        SplitButton::new(Id::unique("left"), "Run")
            .menu_label("Left options")
            .show(ui, |ui| {
                let _ = ui.button("Left action");
            });
        SplitButton::new(Id::unique("right"), "Run")
            .menu_label("Right options")
            .show(ui, |ui| {
                let _ = ui.button("Right action");
            });
    });
    harness.get_by_label("Left options").click();
    harness.run();
    assert!(harness.query_by_label("Left action").is_some());
    assert!(harness.query_by_label("Right action").is_none());
}

const CHOICES: [Choice<'static>; 4] = [
    Choice {
        id: "locked",
        label: "Unavailable 1",
        value: "1",
        enabled: false,
    },
    Choice {
        id: "ten",
        label: "Ten",
        value: "10",
        enabled: true,
    },
    Choice {
        id: "eleven",
        label: "Eleven",
        value: "11",
        enabled: true,
    },
    Choice {
        id: "unicode",
        label: "Résumé",
        value: "Résumé",
        enabled: true,
    },
];

#[derive(Default)]
struct ChoiceState {
    draft: String,
    commits: Vec<ChoiceCommit>,
    disabled: bool,
    empty: bool,
}

fn choice_ui(ui: &mut egui::Ui, state: &mut ChoiceState) {
    ui.add_enabled_ui(!state.disabled, |ui| {
        let choices = if state.empty { &[][..] } else { &CHOICES[..] };
        let result =
            EditableChoice::new(Id::unique("choice"), "Value", &mut state.draft, choices).show(ui);
        if let Some(commit) = result.commit {
            state.commits.push(commit);
        }
    });
    let _ = ui.button("Outside");
}

#[test]
fn enter_commits_exact_text_even_when_suggestions_match() {
    let mut harness = Harness::new_ui_state(choice_ui, ChoiceState::default());
    harness.get_by_label("Value").focus();
    harness.run();
    harness.get_by_label("Value").type_text("1");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().commits,
        vec![ChoiceCommit::Typed("1".into())]
    );
    assert_eq!(harness.state().draft, "1");
}

#[test]
fn arrow_selection_skips_disabled_suggestions() {
    let mut harness = Harness::new_ui_state(
        choice_ui,
        ChoiceState {
            draft: "1".into(),
            ..Default::default()
        },
    );
    harness.get_by_label("Value").focus();
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().draft, "10");
    assert_eq!(
        harness.state().commits,
        vec![ChoiceCommit::Selected {
            id: "ten".into(),
            value: "10".into()
        }]
    );
}

#[test]
fn clicking_suggestion_commits_its_id_and_value() {
    let mut harness = Harness::new_ui_state(choice_ui, ChoiceState::default());
    harness.get_by_label("Value choices").click();
    harness.run();
    harness.get_by_label("Eleven").click();
    harness.run();
    assert_eq!(
        harness.state().commits,
        vec![ChoiceCommit::Selected {
            id: "eleven".into(),
            value: "11".into()
        }]
    );
}

#[test]
fn escape_and_blur_do_not_commit() {
    let mut harness = Harness::new_ui_state(
        choice_ui,
        ChoiceState {
            draft: "Résumé".into(),
            ..Default::default()
        },
    );
    harness.get_by_label("Value choices").click();
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    harness.get_by_label("Outside").click();
    harness.run();
    assert!(harness.state().commits.is_empty());
    assert_eq!(harness.state().draft, "Résumé");
}

#[test]
fn empty_choices_allow_exact_typed_commit() {
    let mut harness = Harness::new_ui_state(
        choice_ui,
        ChoiceState {
            draft: "custom".into(),
            empty: true,
            ..Default::default()
        },
    );
    harness.get_by_label("Value").focus();
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().commits,
        vec![ChoiceCommit::Typed("custom".into())]
    );
}

#[test]
fn disabled_choice_cannot_open_or_commit() {
    let mut harness = Harness::new_ui_state(
        choice_ui,
        ChoiceState {
            disabled: true,
            ..Default::default()
        },
    );
    harness.get_by_label("Value choices").click();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Ten").is_none());
    assert!(harness.state().commits.is_empty());
}

#[test]
fn ime_candidate_acceptance_does_not_submit_the_form() {
    let mut harness = Harness::new_ui_state(choice_ui, ChoiceState::default());
    harness.get_by_label("Value").focus();
    harness.run();
    harness.event(egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "e".into(),
        active_range_chars: None,
    }));
    harness.run();
    // A platform can deliver the commit and its confirming key in one frame.
    // Harness::event processes each queued event in a separate frame, so use
    // RawInput here to exercise the actual batched-input contract.
    harness.input_mut().events.extend([
        egui::Event::Ime(egui::ImeEvent::Commit("é".into())),
        egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    harness.run();
    assert!(harness.state().commits.is_empty());
    assert_eq!(harness.state().draft, "é");
}

#[test]
fn virtualized_suggestions_follow_keyboard_focus() {
    let entries: Vec<String> = (0..100).map(|index| format!("Choice {index:03}")).collect();
    let choices: Vec<Choice<'_>> = entries
        .iter()
        .map(|entry| Choice {
            id: entry,
            label: entry,
            value: entry,
            enabled: true,
        })
        .collect();
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut ChoiceState| {
            let result = EditableChoice::new(
                Id::unique("large"),
                "Large list",
                &mut state.draft,
                &choices,
            )
            .show(ui);
            if let Some(commit) = result.commit {
                state.commits.push(commit);
            }
        },
        ChoiceState::default(),
    );
    harness.get_by_label("Large list").focus();
    harness.run();
    for _ in 0..35 {
        harness.key_press(Key::ArrowDown);
        harness.run();
    }
    assert!(harness.query_by_label("Choice 034").is_some());
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().draft, "Choice 034");
}

const SWATCHES: [Swatch<'static>; 3] = [
    Swatch {
        id: "red",
        label: "Red",
        color: Color32::RED,
        enabled: true,
    },
    Swatch {
        id: "locked",
        label: "Locked",
        color: Color32::GRAY,
        enabled: false,
    },
    Swatch {
        id: "blue",
        label: "Blue",
        color: Color32::BLUE,
        enabled: true,
    },
];

fn palette_ui(ui: &mut egui::Ui, picked: &mut Option<String>) {
    let result = ColorSwatchPalette::new(Id::unique("palette"), &SWATCHES, picked.as_deref())
        .columns(3)
        .show(ui);
    if let Some(id) = result.picked {
        *picked = Some(id);
    }
}

#[test]
fn palette_click_returns_stable_id() {
    let mut harness = Harness::new_ui_state(palette_ui, None);
    harness.get_by_label("Blue").click();
    harness.run();
    assert_eq!(harness.state().as_deref(), Some("blue"));
}

#[test]
fn palette_arrows_move_focus_without_selecting_and_skip_disabled() {
    let mut harness = Harness::new_ui_state(palette_ui, None);
    harness.get_by_label("Red").focus();
    harness.run();
    harness.key_press(Key::ArrowRight);
    harness.run();
    assert_eq!(harness.state(), &None);
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().as_deref(), Some("blue"));
}

#[test]
fn palette_disabled_swatch_is_not_selectable() {
    let mut harness = Harness::new_ui_state(palette_ui, None);
    harness.get_by_label("Locked").click();
    harness.run();
    assert_eq!(harness.state(), &None);
}

#[test]
fn palette_extreme_column_counts_and_empty_data_are_safe() {
    let mut harness = Harness::new_ui(|ui| {
        ColorSwatchPalette::new(Id::unique("empty"), &[], None)
            .columns(0)
            .show(ui);
        ColorSwatchPalette::new(Id::unique("huge"), &SWATCHES, None)
            .columns(usize::MAX)
            .show(ui);
    });
    harness.get_by_label("Red").focus();
    harness.run();
    harness.key_press(Key::ArrowUp);
    harness.run();
}

#[test]
fn ribbon_returns_child_action_without_clipping_tall_contents() {
    let mut harness = Harness::new_ui_state(
        |ui, count: &mut usize| {
            let result = RibbonGroup::new(Id::unique("group"), "Tall group").show(ui, |ui| {
                for index in 0..6 {
                    ui.label(format!("Row {index}"));
                }
                ui.button("Last action").clicked()
            });
            if result.inner {
                *count += 1;
            }
        },
        0,
    );
    harness.get_by_label("Last action").click();
    harness.run();
    assert_eq!(*harness.state(), 1);
}

#[test]
fn ribbon_groups_leave_room_for_neighbors_and_following_content() {
    let mut harness = Harness::new_ui_state(
        |ui, sizes: &mut Option<(egui::Rect, egui::Rect)>| {
            ui.scope_builder(
                egui::UiBuilder::new().layout(egui::Layout::left_to_right(egui::Align::Min)),
                |ui| {
                    let first = RibbonGroup::new(Id::unique("first"), "First")
                        .show(ui, |ui| ui.label("One"));
                    let second = RibbonGroup::new(Id::unique("second"), "Second")
                        .show(ui, |ui| ui.label("Two"));
                    *sizes = Some((first.response.rect, second.response.rect));
                },
            );
            ui.label("Following content");
        },
        None,
    );
    harness.run();
    let (first, second) = harness.state().unwrap();
    assert!(
        first.width() < 160.0,
        "A compact group must not consume the row"
    );
    assert!(
        first.height() < 100.0,
        "A group must not consume remaining height"
    );
    assert!(second.left() >= first.right());
    assert!((second.top() - first.top()).abs() < 1.0);
}
