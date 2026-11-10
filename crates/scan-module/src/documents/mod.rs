//! Optional, fully Rust OCR and PDF export. No GUI or application catalog dependency.
mod bilevel;
pub mod clean_copy;
mod content_cleanup;
pub mod ocr;
pub mod pdf;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    pub bounds: [f32; 4],
}
pub struct ExportPage {
    pub image: PathBuf,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub lines: Vec<TextLine>,
}
pub struct ExportDocument {
    pub title: String,
    pub document_type: String,
    pub tags: String,
    pub pages: Vec<ExportPage>,
}
impl ExportDocument {
    pub fn text(&self) -> String {
        self.pages
            .iter()
            .flat_map(|p| p.lines.iter().map(|l| l.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
