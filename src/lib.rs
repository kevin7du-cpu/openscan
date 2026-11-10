//! Reusable office-style controls composed from standard egui interactions.
//!
//! Controls emit responses and values; the application owns commands, storage,
//! validation, fonts, and document state. See the `gallery` example.

mod editable_choice;
mod palette;
mod ribbon_group;
mod split_button;
mod task_progress;

pub use editable_choice::{Choice, ChoiceCommit, EditableChoice, EditableChoiceResponse};
pub use palette::{ColorSwatchPalette, PaletteResponse, Swatch};
pub use ribbon_group::RibbonGroup;
pub use split_button::{SplitButton, SplitButtonResponse};
pub use task_progress::{TaskProgress, task_progress_popup};
