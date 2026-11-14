// SPDX-License-Identifier: MIT OR Apache-2.0
//! Text recognition through Tesseract, the open-source OCR engine.
//!
//! Tesseract is run as a separate program, so the user installs it once and nothing needs
//! compiling or linking here. [`OcrEngine`] is the seam for adding another engine later.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::settings::OcrOptions;

/// Something that can turn page images into a searchable PDF.
pub trait OcrEngine: Send + Sync {
    /// Name shown in the OCR options window.
    fn name(&self) -> String;

    /// Language codes the engine has data for.
    fn languages(&self) -> Vec<String>;

    /// Writes a PDF at `output` holding only the recognised text of each image, invisible
    /// and in the right places, one page per image. The pictures are added afterwards.
    fn text_layer_pdf(
        &self,
        images: &[PathBuf],
        dpi: u32,
        options: &OcrOptions,
        output: &Path,
    ) -> Result<()>;
}

/// Tesseract, located on this computer.
pub struct Tesseract {
    program: PathBuf,
    version: String,
}

impl Tesseract {
    /// Looks for Tesseract on the `PATH` and in its usual install folders.
    pub fn find() -> Option<Self> {
        let candidates = [
            "tesseract",
            "tesseract.exe",
            r"C:\Program Files\Tesseract-OCR\tesseract.exe",
            "/opt/homebrew/bin/tesseract",
            "/usr/local/bin/tesseract",
        ];
        for candidate in candidates {
            let Ok(output) = crate::backend::command(candidate).arg("--version").output() else {
                continue;
            };
            if output.status.success() {
                // Older versions print the version to the error stream.
                let text = [output.stdout, output.stderr].concat();
                let version = String::from_utf8_lossy(&text)
                    .lines()
                    .next()
                    .unwrap_or("tesseract")
                    .to_owned();
                return Some(Self {
                    program: PathBuf::from(candidate),
                    version,
                });
            }
        }
        None
    }

    /// Recognises one image and returns its plain text.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn text(&self, image: &Path, options: &OcrOptions) -> Result<String> {
        let output = crate::backend::command(&self.program)
            .arg(image)
            .arg("stdout")
            .args([
                "-l",
                &options.language,
                "--psm",
                &options.page_seg_mode.to_string(),
            ])
            .output()
            .context("starting tesseract")?;
        if !output.status.success() {
            bail!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl OcrEngine for Tesseract {
    fn name(&self) -> String {
        self.version.clone()
    }

    fn languages(&self) -> Vec<String> {
        let Ok(output) = crate::backend::command(&self.program)
            .arg("--list-langs")
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1) // heading line
            .map(str::trim)
            .filter(|line| !line.is_empty() && *line != "osd")
            .map(str::to_owned)
            .collect()
    }

    fn text_layer_pdf(
        &self,
        images: &[PathBuf],
        dpi: u32,
        options: &OcrOptions,
        output: &Path,
    ) -> Result<()> {
        if images.is_empty() {
            bail!("no pages to recognise");
        }
        // Tesseract makes one multi-page PDF when given a text file listing the images.
        let list = output.with_extension("pages.txt");
        let lines: Vec<String> = images
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        std::fs::write(&list, lines.join("\n"))
            .with_context(|| format!("writing {}", list.display()))?;

        // Tesseract adds ".pdf" to the output name itself.
        let base = output.with_extension("");
        let result = crate::backend::command(&self.program)
            .arg(&list)
            .arg(&base)
            .args(["-l", &options.language])
            .args(["--psm", &options.page_seg_mode.to_string()])
            .args(["--dpi", &dpi.to_string()])
            .args(["-c", "textonly_pdf=1"])
            .arg("pdf")
            .output()
            .context("starting tesseract");
        let _ = std::fs::remove_file(&list);
        let result = result?;
        if !result.status.success() {
            bail!(
                "tesseract failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }
        let written = base.with_extension("pdf");
        if written != output {
            std::fs::rename(&written, output)
                .with_context(|| format!("moving {} to {}", written.display(), output.display()))?;
        }
        Ok(())
    }
}

/// Prefer the accepted Tesseract path, with the host's existing Rust OCR models
/// as a fallback. Detection is cheap; model loading and recognition stay on workers.
pub fn find(models: Option<&Path>) -> Option<std::sync::Arc<dyn OcrEngine>> {
    if let Some(engine) = Tesseract::find() {
        return Some(std::sync::Arc::new(engine));
    }
    #[cfg(feature = "rust-ocr")]
    if let Some(models) = models
        && scan_module::documents::ocr::MODELS
            .iter()
            .all(|(name, _)| models.join(name).is_file())
    {
        return Some(std::sync::Arc::new(RustOcr {
            models: models.into(),
        }));
    }
    #[cfg(not(feature = "rust-ocr"))]
    let _ = models;
    None
}

/// Pure-Rust OCR for hosts that already ship the hash-verified ocrs/RTen models.
#[cfg(feature = "rust-ocr")]
pub struct RustOcr {
    pub models: PathBuf,
}

#[cfg(feature = "rust-ocr")]
impl OcrEngine for RustOcr {
    fn name(&self) -> String {
        "Rust OCR (ocrs/RTen)".into()
    }
    fn languages(&self) -> Vec<String> {
        vec!["eng".into()]
    }
    fn text_layer_pdf(
        &self,
        images: &[PathBuf],
        dpi: u32,
        options: &OcrOptions,
        output: &Path,
    ) -> Result<()> {
        use scan_module::documents::{ExportDocument, ExportPage, ocr::LocalOcr};
        anyhow::ensure!(
            options.language == "eng",
            "The installed Rust OCR models support English"
        );
        let engine = LocalOcr::load(&self.models)?;
        let mut pages = Vec::new();
        for path in images {
            let image = image::open(path)?.to_rgb8();
            pages.push(ExportPage {
                image: path.clone(),
                width: image.width(),
                height: image.height(),
                dpi,
                lines: engine.recognize_words(&image)?,
            });
        }
        let bytes = scan_module::documents::pdf::export(
            &ExportDocument {
                title: "Scan text".into(),
                document_type: "Scan".into(),
                tags: String::new(),
                pages,
            },
            None,
            &scan_module::CancellationToken::default(),
        )?;
        let mut document = lopdf::Document::load_mem(&bytes)?;
        // The compatibility exporter supplies correctly positioned invisible text.
        // Remove only its own image drawing/resource before adding the new pipeline's
        // compressed image; otherwise each PDF would embed two copies of every scan.
        for id in document.get_pages().into_values() {
            let mut content = lopdf::content::Content::decode(&document.get_page_content(id)?)?;
            content.operations.retain(|op| op.operator != "Do");
            document.change_page_content(id, content.encode()?)?;
            document
                .get_object_mut(id)?
                .as_dict_mut()?
                .get_mut(b"Resources")?
                .as_dict_mut()?
                .remove(b"XObject");
        }
        document.prune_objects();
        document.save(output)?;
        Ok(())
    }
}

#[cfg(all(test, feature = "rust-ocr"))]
mod rust_tests {
    use super::*;
    #[test]
    fn installed_rust_models_create_searchable_text_without_an_image_duplicate() {
        let Some(models) = std::env::var_os("OPENSCAN_MODELS") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("invoice.png");
        std::fs::write(&input, include_bytes!("../tests/data/skewed_page.png")).unwrap();
        let output = dir.path().join("text.pdf");
        RustOcr {
            models: models.into(),
        }
        .text_layer_pdf(&[input], 300, &OcrOptions::default(), &output)
        .unwrap();
        let pdf = lopdf::Document::load(output).unwrap();
        let text = pdf.extract_text(&[1]).unwrap().to_lowercase();
        for word in ["invoice", "customer", "total"] {
            assert!(text.contains(word), "Missing {word} in {text}");
        }
        assert!(!pdf.objects.values().any(|o| o.as_stream().is_ok_and(|s| {
            s.dict
                .get(b"Subtype")
                .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
        })));
    }
}
