//! OpenScan-compatible settings. Original design: OpenScan contributors, MIT.
use crate::{Result, ScanError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanFileFormat {
    #[default]
    Pdf,
    Jpeg,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanSource {
    #[default]
    Flatbed,
    Feeder,
    Duplex,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanColor {
    #[default]
    Color,
    Grayscale,
    BlackWhite,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Paper {
    #[default]
    Letter,
    A4,
    Legal,
    A5,
}
impl Paper {
    pub fn inches(self) -> (f64, f64) {
        match self {
            Self::Letter => (8.5, 11.0),
            Self::A4 => (210.0 / 25.4, 297.0 / 25.4),
            Self::Legal => (8.5, 14.0),
            Self::A5 => (148.0 / 25.4, 210.0 / 25.4),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupOptions {
    pub deskew: bool,
    pub auto_crop: bool,
    pub remove_background: bool,
    pub denoise: bool,
    pub sharpen: bool,
    pub skip_blank_pages: bool,
    pub blank_ink_percent: f64,
    pub brightness: i32,
    pub contrast: i32,
    pub rotate_degrees: u16,
}
impl Default for CleanupOptions {
    fn default() -> Self {
        Self {
            deskew: true,
            auto_crop: true,
            remove_background: true,
            denoise: true,
            sharpen: false,
            skip_blank_pages: false,
            blank_ink_percent: 0.08,
            brightness: 0,
            contrast: 0,
            rotate_degrees: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanProfile {
    pub schema_version: u32,
    pub name: String,
    pub device_id: Option<String>,
    pub source: ScanSource,
    pub color: ScanColor,
    pub paper: Paper,
    pub dpi: u32,
    pub max_pages: u32,
    pub native_interface: bool,
    pub flip_duplex_backs: bool,
    pub searchable_pdf: bool,
    pub jpeg_quality: u8,
    pub cleanup: CleanupOptions,
}
impl Default for ScanProfile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: "Document".into(),
            device_id: None,
            source: ScanSource::Flatbed,
            color: ScanColor::BlackWhite,
            paper: Paper::Letter,
            dpi: 300,
            max_pages: 100,
            native_interface: false,
            flip_duplex_backs: false,
            searchable_pdf: false,
            jpeg_quality: 90,
            cleanup: CleanupOptions::default(),
        }
    }
}
impl ScanProfile {
    pub fn validate(&self) -> Result<()> {
        let problem = if self.schema_version != 1 {
            Some("Unsupported profile version")
        } else if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            Some("Preset name must contain 1–80 characters")
        } else if !(75..=600).contains(&self.dpi) {
            Some("Resolution must be 75–600 DPI")
        } else if !(1..=250).contains(&self.max_pages) {
            Some("Maximum pages must be 1–250")
        } else if !(20..=100).contains(&self.jpeg_quality) {
            Some("JPEG quality must be 20–100")
        } else if !(-100..=100).contains(&self.cleanup.brightness)
            || !(-100..=100).contains(&self.cleanup.contrast)
        {
            Some("Brightness and contrast must be -100–100")
        } else if ![0, 90, 180, 270].contains(&self.cleanup.rotate_degrees) {
            Some("Rotation must be 0, 90, 180 or 270 degrees")
        } else if !self.cleanup.blank_ink_percent.is_finite()
            || !(0.0..=5.0).contains(&self.cleanup.blank_ink_percent)
        {
            Some("Blank coverage must be 0–5 percent")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(ScanError::InvalidRequest(problem.into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCapabilities {
    pub source: ScanSource,
    pub resolutions: Vec<u32>,
}
#[derive(Clone, Debug)]
pub struct DeviceCapabilities {
    pub device_id: String,
    pub sources: Vec<SourceCapabilities>,
    pub note: String,
}
