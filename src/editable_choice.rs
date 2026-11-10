use egui::{
    Button, Id, Key, Modifiers, Popup, PopupCloseBehavior, Response, ScrollArea, TextEdit, Ui,
};

use crate::split_button::menu_trigger;

/// One suggestion. IDs must be unique within this control and stable across sorting.
#[derive(Clone, Copy, Debug)]
pub struct Choice<'a> {
    /// Stable application identifier; independent of display text.
    pub id: &'a str,
    /// Text shown in the suggestion list.
    pub label: &'a str,
    /// Value placed in the text field when selected.
    pub value: &'a str,
    /// Whether the item can be selected.
    pub enabled: bool,
}

/// An explicit commit. Typing alone or dismissing the list does not commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceCommit {
    /// Enter without a deliberately highlighted suggestion preserves exact text.
    Typed(String),
    /// An explicitly selected suggestion, with a stable identity and value.
    Selected {
        /// Stable suggestion identity.
        id: String,
        /// Selected value.
        value: String,
    },
}

/// Interaction result from an editable dropdown.
pub struct EditableChoiceResponse {
    /// Text field response, including changes to the draft.
    pub response: Response,
    /// Explicit commit to validate and apply in the application's model.
    pub commit: Option<ChoiceCommit>,
}

#[derive(Clone, Default)]
struct Navigation {
    open: bool,
    active: Option<String>,
    composing: bool,
}

/// A text field with suggestions and explicit typed-versus-selected commits.
///
/// Enter commits exactly what was typed unless ArrowUp/ArrowDown selected a
/// suggestion. Escape dismisses suggestions without changing the draft. Blur
/// never commits. The caller owns the draft and validates commit values.
#[must_use = "Call show to render the control"]
pub struct EditableChoice<'a> {
    id: Id,
    label: &'a str,
    draft: &'a mut String,
    choices: &'a [Choice<'a>],
    width: f32,
}

impl<'a> EditableChoice<'a> {
    /// Create a labeled editable dropdown with stable identity.
    pub fn new(id: Id, label: &'a str, draft: &'a mut String, choices: &'a [Choice<'a>]) -> Self {
        Self {
            id,
            label,
            draft,
            choices,
            width: 200.0,
        }
    }

    /// Set the preferred text field width in logical points.
    pub fn width(mut self, width: f32) -> Self {
        if width.is_finite() {
            self.width = width.max(40.0);
        }
        self
    }

    /// Render the input and optional filtered suggestions.
    pub fn show(self, ui: &mut Ui) -> EditableChoiceResponse {
        ui.push_id(self.id, |ui| self.show_inner(ui)).inner
    }

    fn show_inner(self, ui: &mut Ui) -> EditableChoiceResponse {
        let text_id = ui.make_persistent_id("text");
        let state_id = ui.make_persistent_id("navigation");
        let mut nav = ui.data_mut(|data| data.get_temp::<Navigation>(state_id).unwrap_or_default());
        let focused = ui.is_enabled() && ui.memory(|memory| memory.has_focus(text_id));
        let mut ime_this_frame = false;
        if focused {
            ui.input(|input| {
                for event in &input.events {
                    if let egui::Event::Ime(event) = event {
                        ime_this_frame = true;
                        match event {
                            egui::ImeEvent::Preedit { text, .. } => {
                                nav.composing = !text.is_empty()
                            }
                            egui::ImeEvent::Commit(_) => nav.composing = false,
                            _ => {}
                        }
                    }
                }
            });
        } else {
            nav.composing = false;
        }
        // Composition belongs to TextEdit/the platform. In particular, Enter
        // accepting an IME candidate must not submit the application's value.
        let capture_keys = focused && !nav.composing && !ime_this_frame;
        let pressed = |ui: &mut Ui, key| {
            capture_keys && ui.input_mut(|input| input.consume_key(Modifiers::NONE, key))
        };
        let down = pressed(ui, Key::ArrowDown);
        let up = pressed(ui, Key::ArrowUp);
        let enter = pressed(ui, Key::Enter);
        let escape = pressed(ui, Key::Escape);

        let label = ui.label(self.label);
        let (mut field, arrow) = ui
            .horizontal(|ui| {
                let field = ui
                    .add(
                        TextEdit::singleline(self.draft)
                            .id(text_id)
                            .desired_width(self.width),
                    )
                    .labelled_by(label.id);
                let arrow = menu_trigger(ui, true, &format!("{} choices", self.label));
                (field, arrow)
            })
            .inner;
        if field.changed() {
            nav.open = true;
            nav.active = None;
        }
        if arrow.clicked() {
            nav.open = !nav.open;
            nav.active = None;
            field.request_focus();
        }
        if !ui.is_enabled() || escape || nav.composing || ime_this_frame {
            nav.open = false;
            nav.active = None;
        }

        let query = self.draft.to_lowercase();
        let matches: Vec<_> = self
            .choices
            .iter()
            .filter(|choice| {
                choice.label.to_lowercase().contains(&query)
                    || choice.value.to_lowercase().contains(&query)
            })
            .collect();
        let enabled: Vec<_> = matches
            .iter()
            .copied()
            .filter(|choice| choice.enabled)
            .collect();
        if !enabled
            .iter()
            .any(|choice| Some(choice.id) == nav.active.as_deref())
        {
            nav.active = None;
        }
        if down || up {
            nav.open = true;
            nav.active = navigate(&enabled, nav.active.as_deref(), down);
        }

        let mut commit = None;
        if enter {
            let active = if nav.open {
                enabled
                    .iter()
                    .find(|choice| Some(choice.id) == nav.active.as_deref())
                    .copied()
            } else {
                None
            };
            commit = Some(match active {
                Some(choice) => select(self.draft, choice),
                None => ChoiceCommit::Typed(self.draft.clone()),
            });
            nav.open = false;
            nav.active = None;
        }

        let anchor = field.clone().union(arrow);
        let mut picked = None;
        Popup::from_response(&anchor)
            .id(state_id.with("popup"))
            .open_bool(&mut nav.open)
            .width(anchor.rect.width())
            .accessible_name(format!("{} suggestions", self.label))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if matches.is_empty() {
                    ui.label("No matching choices");
                }
                let row_height = ui.spacing().interact_size.y;
                let mut scroll = ScrollArea::vertical().max_height(220.0);
                if field.changed() {
                    scroll = scroll.vertical_scroll_offset(0.0);
                } else if (down || up)
                    && let Some(index) = matches
                        .iter()
                        .position(|choice| Some(choice.id) == nav.active.as_deref())
                {
                    let offset = (index.saturating_sub(3) as f32)
                        * (row_height + ui.spacing().item_spacing.y);
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                scroll.show_rows(ui, row_height, matches.len(), |ui, range| {
                    for index in range {
                        if let Some(choice) = matches.get(index) {
                            let response = ui
                                .push_id(choice.id, |ui| {
                                    ui.add_enabled(
                                        choice.enabled,
                                        Button::selectable(
                                            Some(choice.id) == nav.active.as_deref(),
                                            choice.label,
                                        ),
                                    )
                                })
                                .inner;
                            if response.clicked() {
                                picked = Some(*choice);
                            }
                            if (down || up) && Some(choice.id) == nav.active.as_deref() {
                                response.scroll_to_me(None);
                            }
                        }
                    }
                });
            });
        if let Some(choice) = picked {
            commit = Some(select(self.draft, choice));
            nav.open = false;
            nav.active = None;
            field.request_focus();
        }
        if matches!(commit, Some(ChoiceCommit::Selected { .. })) {
            field.mark_changed();
        }
        ui.data_mut(|data| data.insert_temp(state_id, nav));
        EditableChoiceResponse {
            response: field,
            commit,
        }
    }
}

fn navigate(choices: &[&Choice<'_>], current: Option<&str>, down: bool) -> Option<String> {
    let position = choices.iter().position(|choice| Some(choice.id) == current);
    let index = match (position, down) {
        (None, true) => 0,
        (None, false) => choices.len().saturating_sub(1),
        (Some(index), true) => (index + 1).min(choices.len().saturating_sub(1)),
        (Some(index), false) => index.saturating_sub(1),
    };
    choices.get(index).map(|choice| choice.id.to_owned())
}

fn select(draft: &mut String, choice: &Choice<'_>) -> ChoiceCommit {
    draft.clear();
    draft.push_str(choice.value);
    ChoiceCommit::Selected {
        id: choice.id.to_owned(),
        value: choice.value.to_owned(),
    }
}
