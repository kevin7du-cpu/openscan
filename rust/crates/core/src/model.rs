use crate::{Result, error};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Flatbed,
    Feeder,
    Duplex,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Color {
    #[default]
    Color,
    Grayscale,
    BlackWhite,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Paper {
    #[default]
    Letter,
    #[serde(rename = "a4")]
    A4,
    Legal,
    #[serde(rename = "a5")]
    A5,
}
impl Paper {
    pub fn millimeters(self) -> (f32, f32) {
        match self {
            Self::Letter => (215.9, 279.4),
            Self::A4 => (210., 297.),
            Self::Legal => (215.9, 355.6),
            Self::A5 => (148., 210.),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Cleanup {
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
impl Default for Cleanup {
    fn default() -> Self {
        Self {
            deskew: true,
            auto_crop: false,
            remove_background: false,
            denoise: false,
            sharpen: false,
            skip_blank_pages: false,
            blank_ink_percent: 0.08,
            brightness: 0,
            contrast: 0,
            rotate_degrees: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub device_id: Option<String>,
    pub source: Source,
    pub color: Color,
    pub paper: Paper,
    pub dpi: u32,
    pub max_pages: u32,
    pub native_interface: bool,
    pub flip_duplex_backs: bool,
    pub searchable_pdf: bool,
    pub jpeg_quality: u8,
    pub cleanup: Cleanup,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: "Document".into(),
            device_id: None,
            source: Source::Flatbed,
            color: Color::Color,
            paper: Paper::Letter,
            dpi: 300,
            max_pages: 100,
            native_interface: false,
            flip_duplex_backs: false,
            searchable_pdf: false,
            jpeg_quality: 90,
            cleanup: Cleanup::default(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(error("Unsupported preset version; expected 1."));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err(error("Preset name must contain 1–80 characters."));
        }
        if !(75..=600).contains(&self.dpi) || !(1..=250).contains(&self.max_pages) {
            return Err(error("Choose 75–600 DPI and 1–250 pages."));
        }
        if !(20..=100).contains(&self.jpeg_quality) {
            return Err(error("JPEG quality must be 20–100."));
        }
        if !(-100..=100).contains(&self.cleanup.brightness)
            || !(-100..=100).contains(&self.cleanup.contrast)
        {
            return Err(error("Brightness and contrast must be -100–100."));
        }
        if ![0, 90, 180, 270].contains(&self.cleanup.rotate_degrees) {
            return Err(error("Rotation must be 0, 90, 180 or 270 degrees."));
        }
        if !self.cleanup.blank_ink_percent.is_finite()
            || !(0.0..=5.0).contains(&self.cleanup.blank_ink_percent)
        {
            return Err(error("Blank coverage must be 0–5 percent."));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub driver: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCaps {
    pub source: Source,
    pub dpi: Vec<u32>,
    pub min_dpi: u32,
    pub max_dpi: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub device_id: String,
    pub sources: Vec<SourceCaps>,
    pub note: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub original_path: PathBuf,
    pub image_path: PathBuf,
    pub dpi: u32,
    pub deskew_angle: f64,
    pub ink_percent: f64,
    pub excluded: bool,
    pub rotation: u16,
}
#[derive(Debug, Clone)]
pub struct RawPage {
    pub path: PathBuf,
    pub dpi: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: String,
    pub page: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_profiles_are_rejected() {
        let mut p = Profile::default();
        p.cleanup.blank_ink_percent = f64::NAN;
        assert!(p.validate().is_err());
        p.cleanup.blank_ink_percent = 0.08;
        p.max_pages = 0;
        assert!(p.validate().is_err());
        assert!(serde_json::from_str::<Profile>(r#"{"unknown":true}"#).is_err());
    }
}
