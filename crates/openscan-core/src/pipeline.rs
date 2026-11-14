// SPDX-License-Identifier: MIT OR Apache-2.0
//! The work done away from the interface: scan and clean pages, then write the output.
//!
//! Both jobs run on their own thread and report back through a channel, so the window
//! stays responsive however long a scan or OCR takes.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use anyhow::{Context, Result, bail};

use crate::backend::{self, Device, ScanRequest};
use crate::cleanup::{Page, clean_page};
use crate::ocr::OcrEngine;
use crate::pdf::{EncodedPage, add_page_images, append_pdf, encode_page, write_image_pdf};
use crate::settings::ScanSettings;

/// JPEG quality used when "Reduce size and optimize" is off.
const UNOPTIMIZED_JPEG_QUALITY: u8 = 92;

/// Messages sent from a job to the interface.
pub enum Event {
    /// A line for the activity log.
    Log(String),
    /// Scanning and clean-up finished with these pages.
    Scanned(Vec<Page>),
    /// Output was written to these files.
    Saved(Vec<PathBuf>),
    /// The job stopped with this error.
    Failed(String),
}

/// Where the finished document goes.
#[derive(Clone, Debug)]
pub enum Target {
    /// A new PDF at this path.
    NewPdf(PathBuf),
    /// Added to the end of this existing PDF.
    Append(PathBuf),
    /// Several PDFs in `folder`, named `prefix` plus a number.
    MultipleFiles {
        folder: PathBuf,
        prefix: String,
        pages_per_file: usize,
    },
}

/// A fresh empty folder for one job's temporary files, removed when dropped.
struct WorkDir(tempfile::TempDir);
impl WorkDir {
    fn new() -> Result<Self> {
        Ok(Self(
            tempfile::Builder::new().prefix("openscan-").tempdir()?,
        ))
    }
}

/// Scans from `device` and cleans each page. Call from a worker thread.
pub fn scan_and_clean(
    device: &Device,
    settings: &ScanSettings,
    input_files: &[PathBuf],
    log: &dyn Fn(String),
) -> Result<Vec<Page>> {
    let backend = backend::all_backends()
        .into_iter()
        .find(|b| b.name() == device.backend)
        .with_context(|| format!("No backend named {}", device.backend))?;
    scan_with_backend(backend.as_ref(), device, settings, input_files, log)
}

/// Run a host-supplied backend through precisely the same cleanup pipeline.
pub fn scan_with_backend(
    backend: &dyn backend::ScannerBackend,
    device: &Device,
    settings: &ScanSettings,
    input_files: &[PathBuf],
    log: &dyn Fn(String),
) -> Result<Vec<Page>> {
    anyhow::ensure!(
        (75..=600).contains(&settings.resolution),
        "Resolution must be 75–600 DPI"
    );
    anyhow::ensure!(input_files.len() <= 250, "Import at most 250 images");
    let work_dir = WorkDir::new()?;
    let request = ScanRequest {
        device,
        settings,
        input_files,
        work_dir: work_dir.0.path(),
    };
    let images = backend.scan(&request, log)?;
    let total = images.len();
    let mut pages = Vec::with_capacity(total);
    for (index, image) in images.into_iter().enumerate() {
        log(format!("Cleaning page {} of {total}", index + 1));
        match clean_page(image, settings) {
            Some(mut page) => {
                if device.backend == backend::FILE_IMPORT_BACKEND {
                    page.source = input_files.get(index).cloned();
                }
                pages.push(page);
            }
            None => log(format!("Page {} is blank and was removed", index + 1)),
        }
    }
    if pages.is_empty() {
        bail!("every scanned page was blank");
    }
    Ok(pages)
}

/// Writes `pages` to `target`, running OCR when the settings ask for it.
/// Returns the files written. Call from a worker thread.
pub fn save(
    pages: &[Page],
    settings: &ScanSettings,
    target: &Target,
    ocr: Option<&dyn OcrEngine>,
    log: &dyn Fn(String),
) -> Result<Vec<PathBuf>> {
    if pages.is_empty() {
        bail!("there are no pages to save");
    }
    let ocr = match (settings.ocr, ocr) {
        (true, Some(engine)) => Some(engine),
        (true, None) => bail!("OCR was requested but no OCR engine is available"),
        (false, _) => None,
    };
    // With optimizing off, pages keep their full resolution and a high image quality.
    let (quality, max_dpi) = if settings.optimize {
        (
            settings.optimize_opts.jpeg_quality,
            settings.optimize_opts.downsample_dpi,
        )
    } else {
        (UNOPTIMIZED_JPEG_QUALITY, 0)
    };
    let work_dir = WorkDir::new()?;

    log("Compressing pages".to_owned());
    let encoded: Vec<EncodedPage> = pages
        .iter()
        .map(|page| encode_page(page, quality, max_dpi))
        .collect::<Result<_>>()?;

    // Builds one PDF from a run of pages, with or without a text layer.
    let build = |range: std::ops::Range<usize>, output: &Path| -> Result<()> {
        let Some(engine) = ocr else {
            return write_image_pdf(&encoded[range], output);
        };
        // Text is read from the untouched pages, not from the compressed copies.
        let mut files = Vec::with_capacity(range.len());
        for index in range.clone() {
            let file = work_dir.0.path().join(format!("page{index:04}.png"));
            pages[index]
                .image
                .save(&file)
                .with_context(|| format!("writing {}", file.display()))?;
            files.push(file);
        }
        log(format!("Recognising text on {} page(s)", range.len()));
        let text_pdf = work_dir
            .0
            .path()
            .join(format!("text{:04}.pdf", range.start));
        engine.text_layer_pdf(
            &files,
            pages[range.start].dpi,
            &settings.ocr_opts,
            &text_pdf,
        )?;
        for file in files {
            let _ = std::fs::remove_file(file);
        }
        add_page_images(&text_pdf, &encoded[range], output)
    };

    match target {
        Target::NewPdf(path) => {
            atomic_pdf(path, false, |stage| {
                build(0..pages.len(), stage)?;
                if settings.add_metadata {
                    crate::pdf::add_named_metadata(stage, path)?;
                }
                Ok(())
            })?;
            Ok(vec![path.clone()])
        }
        Target::Append(path) => {
            let addition = work_dir.0.path().join("addition.pdf");
            let merged = work_dir.0.path().join("merged.pdf");
            build(0..pages.len(), &addition)?;
            log(format!("Appending to {}", path.display()));
            append_pdf(path, &addition, &merged)?;
            // Replace the original only once the merged file is complete.
            atomic_pdf(path, true, |stage| {
                std::fs::copy(&merged, stage)?;
                Ok(())
            })?;
            Ok(vec![path.clone()])
        }
        Target::MultipleFiles {
            folder,
            prefix,
            pages_per_file,
        } => {
            anyhow::ensure!(
                !prefix.is_empty()
                    && !prefix
                        .chars()
                        .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
                    && prefix != "."
                    && prefix != "..",
                "Choose a file name prefix without path separators"
            );
            std::fs::create_dir_all(folder)
                .with_context(|| format!("creating {}", folder.display()))?;
            let mut written = Vec::new();
            let per_file = (*pages_per_file).max(1);
            for (file_index, start) in (0..pages.len()).step_by(per_file).enumerate() {
                let path = unused_name(folder, prefix, file_index + 1);
                atomic_pdf(&path, false, |stage| {
                    build(start..(start + per_file).min(pages.len()), stage)?;
                    if settings.add_metadata {
                        crate::pdf::add_named_metadata(stage, &path)?;
                    }
                    Ok(())
                })?;
                written.push(path);
            }
            Ok(written)
        }
    }
}

fn atomic_pdf(path: &Path, replace: bool, build: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stage = tempfile::NamedTempFile::new_in(parent)?;
    build(stage.path())?;
    // Validate the completed PDF before publishing it at the chosen destination.
    crate::pdf::page_count(stage.path())?;
    stage.as_file().sync_all()?;
    if replace {
        stage.persist(path).map_err(|e| e.error)?;
    } else {
        stage.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}

/// A file name in `folder` that does not exist yet, starting from `prefix_NNN.pdf`.
fn unused_name(folder: &Path, prefix: &str, number: usize) -> PathBuf {
    let plain = folder.join(format!("{prefix}_{number:03}.pdf"));
    if !plain.exists() {
        return plain;
    }
    (2..)
        .map(|copy| folder.join(format!("{prefix}_{number:03} ({copy}).pdf")))
        .find(|path| !path.exists())
        .expect("an unused file name always exists")
}

/// Sends an event, ignoring the error if the window has closed.
pub fn send(events: &Sender<Event>, event: Event) {
    let _ = events.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::Tesseract;
    use crate::pdf::page_count;
    use crate::settings::{ColorMode, OcrOptions};
    use image::{DynamicImage, GrayImage, Luma};

    fn dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("scan-module-pipe-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn pages(count: usize) -> Vec<Page> {
        let image = DynamicImage::ImageLuma8(GrayImage::from_pixel(425, 550, Luma([245])));
        vec![Page::from_image(image, 50); count]
    }

    #[test]
    fn multiple_files_split_pages_and_never_overwrite() {
        let folder = dir("multi");
        let target = Target::MultipleFiles {
            folder: folder.clone(),
            prefix: "Scan".into(),
            pages_per_file: 2,
        };
        let settings = ScanSettings::default();
        let first = save(&pages(5), &settings, &target, None, &|_| {}).unwrap();
        let counts: Vec<usize> = first.iter().map(|path| page_count(path).unwrap()).collect();
        assert_eq!(counts, [2, 2, 1]);

        let second = save(&pages(1), &settings, &target, None, &|_| {}).unwrap();
        assert!(
            !first.contains(&second[0]),
            "an existing file was overwritten"
        );
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn append_adds_to_an_existing_pdf() {
        let folder = dir("append");
        let path = folder.join("doc.pdf");
        let settings = ScanSettings::default();
        save(
            &pages(2),
            &settings,
            &Target::NewPdf(path.clone()),
            None,
            &|_| {},
        )
        .unwrap();
        save(
            &pages(3),
            &settings,
            &Target::Append(path.clone()),
            None,
            &|_| {},
        )
        .unwrap();
        assert_eq!(page_count(&path).unwrap(), 5);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn requesting_ocr_without_an_engine_is_an_error() {
        let settings = ScanSettings {
            ocr: true,
            ..ScanSettings::default()
        };
        let target = Target::NewPdf(dir("noocr").join("x.pdf"));
        assert!(save(&pages(1), &settings, &target, None, &|_| {}).is_err());
    }

    /// End to end on the sample page in `tests/data`, when Tesseract is installed:
    /// import, clean, OCR, and check the words come back out of the PDF.
    #[test]
    fn sample_page_becomes_a_searchable_pdf() {
        let Some(tesseract) = Tesseract::find() else {
            eprintln!("skipped: Tesseract is not installed");
            return;
        };
        let sample_dir = tempfile::tempdir().unwrap();
        let sample = sample_dir.path().join("skewed_page.png");
        std::fs::write(&sample, include_bytes!("../tests/data/skewed_page.png")).unwrap();
        let device = backend::discover()
            .0
            .into_iter()
            .find(|d| d.backend == backend::FILE_IMPORT_BACKEND)
            .unwrap();
        let mut settings = ScanSettings {
            ocr: true,
            color_mode: ColorMode::Grayscale,
            ..ScanSettings::default()
        };
        settings.optimize_opts.background_removal = crate::settings::Level::Medium;

        let cleaned = scan_and_clean(&device, &settings, &[sample], &|_| {}).unwrap();
        let folder = dir("ocr");
        let output = folder.join("searchable.pdf");
        save(
            &cleaned,
            &settings,
            &Target::NewPdf(output.clone()),
            Some(&tesseract),
            &|_| {},
        )
        .unwrap();
        assert_eq!(page_count(&output).unwrap(), 1);

        // Read the recognised text straight back from the cleaned page.
        let png = folder.join("cleaned.png");
        cleaned[0].image.save(&png).unwrap();
        let text = tesseract
            .text(&png, &OcrOptions::default())
            .unwrap()
            .to_lowercase();
        for word in ["invoice", "customer", "total"] {
            assert!(text.contains(word), "missing {word:?} in {text:?}");
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
}
