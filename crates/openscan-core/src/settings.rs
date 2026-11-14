// SPDX-License-Identifier: MIT OR Apache-2.0
//! Scan settings, presets and their storage on disk.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Declares a simple enum whose variants each have a label for the interface.
macro_rules! labelled_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $label:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $($(#[$vmeta])* $variant),+ }

        impl $name {
            /// Every variant, in the order shown in drop-down lists.
            #[allow(dead_code)] // not every list is shown as a drop-down
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// Text shown to the user.
            pub fn label(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }
        }
    };
}

labelled_enum!(
    /// Which sides of each sheet to scan.
    Sides {
        #[default]
        Front => "Front Sides",
        Both => "Both Sides",
    }
);

labelled_enum!(
    /// Where the scanner takes paper from.
    Source {
        #[default]
        Auto => "Automatic",
        Flatbed => "Flatbed",
        Feeder => "Document Feeder",
    }
);

labelled_enum!(
    /// Colour depth of the scan.
    ColorMode {
        Auto => "Automatic",
        Color => "Color",
        Grayscale => "Grayscale",
        #[default]
        BlackWhite => "Black & White",
    }
);

labelled_enum!(
    /// Paper size requested from the scanner.
    PaperSize {
        #[default]
        Auto => "Automatic",
        Letter => "Letter (8.5 x 11 in)",
        Legal => "Legal (8.5 x 14 in)",
        A4 => "A4 (210 x 297 mm)",
        A5 => "A5 (148 x 210 mm)",
        B5 => "B5 (176 x 250 mm)",
    }
);

impl PaperSize {
    /// Width and height in millimetres, or `None` to let the scanner decide.
    pub fn size_mm(self) -> Option<(f32, f32)> {
        match self {
            PaperSize::Auto => None,
            PaperSize::Letter => Some((215.9, 279.4)),
            PaperSize::Legal => Some((215.9, 355.6)),
            PaperSize::A4 => Some((210.0, 297.0)),
            PaperSize::A5 => Some((148.0, 210.0)),
            PaperSize::B5 => Some((176.0, 250.0)),
        }
    }
}

labelled_enum!(
    /// How image data travels from the scanner driver to the application.
    TransferMechanism {
        #[default]
        Native => "Native mode",
        BufferedMemory => "Buffered memory mode",
    }
);

labelled_enum!(
    /// What happens to the scanned pages.
    OutputMode {
        #[default]
        NewPdf => "New PDF Document",
        Append => "Append",
        MultipleFiles => "Save Multiple Files",
    }
);

labelled_enum!(
    /// Strength of a filter that has more than an on/off setting.
    Level {
        #[default]
        Off => "Off",
        Low => "Low",
        Medium => "Medium",
        High => "High",
    }
);

labelled_enum!(
    /// Which arrangement of the Scan dialog is shown.
    DialogLayout {
        #[default]
        Classic => "Layout 1",
        Compact => "Layout 2",
    }
);

/// Resolutions offered in the drop-down list, in dots per inch.
pub const RESOLUTIONS: &[u32] = &[75, 100, 150, 200, 300, 400, 600];

/// Options for "Save Multiple Files".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiFileOptions {
    /// Folder the files are written to.
    pub folder: Option<PathBuf>,
    /// Start of each file name; a number is added after it.
    pub prefix: String,
    /// Number of pages in each file.
    pub pages_per_file: u32,
}

impl Default for MultiFileOptions {
    fn default() -> Self {
        Self {
            folder: None,
            prefix: "Scan".to_owned(),
            pages_per_file: 1,
        }
    }
}

/// Compression settings and the image corrections, each with its own on/off switch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptimizeOptions {
    /// JPEG quality for grey and colour pages, from 1 (smallest) to 100 (best).
    pub jpeg_quality: u8,
    /// Store grey and colour pages at no more than this resolution; 0 keeps the scan
    /// resolution. Black-and-white pages always keep it, and text is always read from the
    /// full-resolution scan.
    pub downsample_dpi: u32,
    /// Straighten pages that were fed at an angle.
    pub deskew: bool,
    /// Remove isolated dots.
    pub despeckle: bool,
    /// Even out shadows from creases, wrinkles and uneven lighting, and whiten the paper.
    pub background_removal: Level,
    /// Smooth away the dot pattern of printed photographs (halftone).
    pub descreen: bool,
    /// Make the edges of letters crisper.
    pub text_sharpening: Level,
    /// Trim black scanner borders from the page edges.
    pub crop_borders: bool,
    /// Drop pages that contain nothing.
    pub remove_blank_pages: bool,
}

impl Default for OptimizeOptions {
    fn default() -> Self {
        Self {
            // Chosen by measurement so a 30-page document is small enough to email while
            // print stays crisp on screen; see "Default sizes" in the README.
            jpeg_quality: 50,
            downsample_dpi: 150,
            deskew: true,
            despeckle: true,
            background_removal: Level::Low,
            descreen: false,
            text_sharpening: Level::Off,
            crop_borders: true,
            remove_blank_pages: false,
        }
    }
}

/// Options behind "Make Searchable (run OCR)".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrOptions {
    /// Tesseract language code, such as `eng` or `eng+fra`.
    pub language: String,
    /// Tesseract page segmentation mode; 3 is fully automatic.
    pub page_seg_mode: u8,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            language: "eng".to_owned(),
            page_seg_mode: 3,
        }
    }
}

/// Everything the Scan dialog lets the user choose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanSettings {
    /// Key of the chosen scanner (see `Device::key`); empty when none is chosen.
    pub device: String,
    pub transfer: TransferMechanism,
    /// Let the scanner driver show its own settings window.
    pub use_native_ui: bool,
    /// After each batch, ask whether to scan more pages into the same document.
    pub prompt_more_pages: bool,
    pub sides: Sides,
    pub source: Source,
    pub color_mode: ColorMode,
    pub paper_size: PaperSize,
    pub resolution: u32,
    pub output: OutputMode,
    /// Existing PDF to add pages to when `output` is `Append`.
    pub append_path: Option<PathBuf>,
    pub multi: MultiFileOptions,
    pub optimize: bool,
    pub optimize_opts: OptimizeOptions,
    pub ocr: bool,
    pub ocr_opts: OcrOptions,
    /// Record a title, the producing program and the date inside new PDF files.
    pub add_metadata: bool,
}

impl Default for ScanSettings {
    fn default() -> Self {
        Self {
            device: String::new(),
            transfer: TransferMechanism::default(),
            use_native_ui: false,
            prompt_more_pages: false,
            sides: Sides::default(),
            source: Source::default(),
            color_mode: ColorMode::default(),
            paper_size: PaperSize::default(),
            resolution: 300,
            output: OutputMode::default(),
            append_path: None,
            multi: MultiFileOptions::default(),
            optimize: true,
            optimize_opts: OptimizeOptions::default(),
            ocr: false,
            ocr_opts: OcrOptions::default(),
            add_metadata: true,
        }
    }
}

/// A named set of scan settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub settings: ScanSettings,
}

/// The user's presets, in the order they chose.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetStore {
    pub presets: Vec<Preset>,
    /// The Scan dialog arrangement the user prefers.
    pub layout: DialogLayout,
}

impl PresetStore {
    /// File the presets are kept in, inside the user's configuration folder.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("scan-module")
            .join("presets.json")
    }

    /// Reads presets from `path`; a missing file gives an empty store.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Writes presets to `path`, creating the folder if needed.
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Adds a preset, replacing any existing one with the same name.
    pub fn upsert(&mut self, preset: Preset) {
        match self.presets.iter_mut().find(|p| p.name == preset.name) {
            Some(existing) => *existing = preset,
            None => self.presets.push(preset),
        }
    }

    /// Moves the preset at `index` one place earlier; returns its new index.
    pub fn move_up(&mut self, index: usize) -> usize {
        if index > 0 && index < self.presets.len() {
            self.presets.swap(index, index - 1);
            index - 1
        } else {
            index
        }
    }

    /// Moves the preset at `index` one place later; returns its new index.
    pub fn move_down(&mut self, index: usize) -> usize {
        if index + 1 < self.presets.len() {
            self.presets.swap(index, index + 1);
            index + 1
        } else {
            index
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_defaults_apply_to_fresh_and_legacy_settings_without_overriding_presets() {
        for settings in [ScanSettings::default(), serde_json::from_str("{}").unwrap()] {
            assert_eq!(settings.color_mode, ColorMode::BlackWhite);
            assert!(settings.optimize_opts.deskew);
            assert!(settings.optimize_opts.despeckle);
            assert_eq!(settings.optimize_opts.background_removal, Level::Low);
            assert!(settings.optimize_opts.crop_borders);
        }
        let saved: ScanSettings = serde_json::from_str(
            r#"{"color_mode":"Color","optimize_opts":{"deskew":false,"despeckle":false,"background_removal":"Off","crop_borders":false}}"#,
        )
        .unwrap();
        assert_eq!(saved.color_mode, ColorMode::Color);
        assert!(!saved.optimize_opts.deskew);
        assert!(!saved.optimize_opts.despeckle);
        assert_eq!(saved.optimize_opts.background_removal, Level::Off);
        assert!(!saved.optimize_opts.crop_borders);
    }

    fn named(name: &str) -> Preset {
        Preset {
            name: name.to_owned(),
            settings: ScanSettings::default(),
        }
    }

    #[test]
    fn presets_round_trip_through_a_file() {
        let path =
            std::env::temp_dir().join(format!("scan-module-test-{}.json", std::process::id()));
        let mut store = PresetStore::default();
        store.upsert(named("Invoices"));
        store.upsert(named("Photos"));
        store.save(&path).unwrap();
        assert_eq!(PresetStore::load(&path).unwrap(), store);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn upsert_replaces_a_preset_with_the_same_name() {
        let mut store = PresetStore::default();
        store.upsert(named("Invoices"));
        let mut changed = named("Invoices");
        changed.settings.resolution = 600;
        store.upsert(changed);
        assert_eq!(store.presets.len(), 1);
        assert_eq!(store.presets[0].settings.resolution, 600);
    }

    #[test]
    fn moving_stops_at_the_ends() {
        let mut store = PresetStore {
            presets: vec![named("a"), named("b"), named("c")],
            ..Default::default()
        };
        assert_eq!(store.move_up(0), 0);
        assert_eq!(store.move_down(2), 2);
        assert_eq!(store.move_down(0), 1);
        let order: Vec<_> = store.presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(order, ["b", "a", "c"]);
    }

    #[test]
    fn old_preset_files_missing_newer_fields_still_load() {
        let store: PresetStore =
            serde_json::from_str(r#"{"presets":[{"name":"Old","settings":{"resolution":200}}]}"#)
                .unwrap();
        assert_eq!(store.presets[0].settings.resolution, 200);
        assert!(store.presets[0].settings.optimize);
    }
}
