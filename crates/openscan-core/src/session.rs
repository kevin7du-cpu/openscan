//! Compatibility with existing OpenScan sessions, without reprocessing their images.
use crate::{ScanSession, cleanup::Page};
use anyhow::{Result, ensure};
use scan_module::session::PageRecord;
use std::path::{Path, PathBuf};

/// Commit an entire completed batch. The previous manifest remains valid on failure.
pub fn append(root: &Path, previous: Option<&ScanSession>, pages: &[Page]) -> Result<ScanSession> {
    ensure!(!pages.is_empty(), "No pages to retain");
    let mut session = match previous {
        Some(s) => s.clone(),
        None => ScanSession::create(root.join("Sessions").join(uuid::Uuid::new_v4().to_string()))?,
    };
    ensure!(
        session.pages.len() + pages.len() <= 250,
        "A session holds at most 250 pages"
    );
    for page in pages {
        ensure!(
            (75..=600).contains(&page.dpi),
            "Scan DPI must be between 75 and 600"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let ext = page
            .source
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|s| s.to_str())
            .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("png");
        let original_path = PathBuf::from(format!("originals/{id}.{ext}"));
        let image_path = PathBuf::from(format!("processed/{id}.png"));
        if let Some(source) = &page.source {
            std::fs::copy(source, session.directory.join(&original_path))?;
        } else {
            page.original.save(session.directory.join(&original_path))?;
        }
        page.image.save(session.directory.join(&image_path))?;
        session.pages.push(PageRecord {
            id,
            original_path,
            image_path,
            dpi: page.dpi,
            deskew_angle: 0.0,
            ink_percent: 0.0,
            excluded: false,
            rotation: 0,
        });
    }
    session.save()?;
    Ok(session)
}

/// Replace only the last acquired batch, committing its replacement before removing old files.
/// A failed acquisition or append leaves the previous manifest and files intact.
pub fn replace_last_batch(
    previous: &ScanSession,
    old_ids: &[String],
    pages: &[Page],
) -> Result<(ScanSession, Vec<String>)> {
    ensure!(!old_ids.is_empty(), "No scan batch to replace");
    ensure!(
        old_ids.len() <= previous.pages.len(),
        "Scan batch no longer exists"
    );
    let start = previous.pages.len() - old_ids.len();
    ensure!(
        previous.pages[start..]
            .iter()
            .map(|p| &p.id)
            .eq(old_ids.iter()),
        "The latest scan batch changed"
    );
    // Resolve before committing so an invalid legacy path cannot trigger a partial replacement.
    let old_paths = previous.pages[start..]
        .iter()
        .flat_map(|p| [&p.original_path, &p.image_path])
        .map(|p| previous.resolve(p))
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()?;
    let mut prefix = previous.clone();
    prefix.pages.truncate(start);
    let session = append(&previous.directory, Some(&prefix), pages)?;
    let retained_paths = session
        .pages
        .iter()
        .flat_map(|p| [&p.original_path, &p.image_path])
        .map(|p| session.resolve(p))
        .collect::<std::result::Result<std::collections::BTreeSet<_>, _>>()?;
    let mut warnings = Vec::new();
    for path in old_paths.difference(&retained_paths) {
        if let Err(error) = std::fs::remove_file(path) {
            warnings.push(format!(
                "Rescan saved, but an old scan file could not be removed: {error}"
            ));
        }
    }
    Ok((session, warnings))
}

/// Load previews from existing corrected images. Do not change the old manifest.
pub fn pages(session: &ScanSession) -> Result<Vec<Page>> {
    session
        .pages
        .iter()
        .filter(|p| !p.excluded)
        .map(|record| {
            let mut page = Page::from_image(
                image::open(session.resolve(&record.image_path)?)?,
                record.dpi,
            );
            page.source = Some(session.resolve(&record.original_path)?);
            Ok(page)
        })
        .collect()
}

/// Versioned export settings alongside a compatible v1 session manifest.
pub fn write_settings(
    session: &ScanSession,
    settings: &crate::settings::ScanSettings,
) -> Result<()> {
    use std::io::Write;
    let mut temp = tempfile::NamedTempFile::new_in(&session.directory)?;
    temp.write_all(&serde_json::to_vec_pretty(settings)?)?;
    temp.as_file().sync_all()?;
    temp.persist(session.directory.join("openscan-v3.json"))
        .map_err(|e| e.error)?;
    Ok(())
}
pub fn settings(session: &ScanSession) -> Result<Option<crate::settings::ScanSettings>> {
    let path = session.directory.join("openscan-v3.json");
    if !path.exists() {
        return Ok(None);
    }
    ensure!(
        std::fs::metadata(&path)?.len() <= 1024 * 1024,
        "Oversized scan settings file"
    );
    Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
}
/// Export a retained session using the same compression/OCR pipeline as standalone OpenScan.
pub fn pdf_bytes(
    session: &ScanSession,
    settings: &crate::settings::ScanSettings,
) -> Result<Vec<u8>> {
    pdf_bytes_with_models(session, settings, None)
}
pub fn pdf_bytes_with_models(
    session: &ScanSession,
    settings: &crate::settings::ScanSettings,
    models: Option<&Path>,
) -> Result<Vec<u8>> {
    let pages = pages(session)?;
    let stage = tempfile::tempdir()?;
    let path = stage.path().join("scan.pdf");
    let engine = crate::ocr::find(models);
    crate::pipeline::save(
        &pages,
        settings,
        &crate::pipeline::Target::NewPdf(path.clone()),
        engine.as_deref(),
        &|_| {},
    )?;
    Ok(std::fs::read(path)?)
}

/// Map the public host/automation profile to the accepted processing settings.
pub fn from_profile(profile: &scan_module::ScanProfile) -> crate::settings::ScanSettings {
    use crate::settings::*;
    let mut settings = ScanSettings {
        resolution: profile.dpi,
        color_mode: match profile.color {
            scan_module::ScanColor::BlackWhite => ColorMode::BlackWhite,
            scan_module::ScanColor::Grayscale => ColorMode::Grayscale,
            scan_module::ScanColor::Color => ColorMode::Color,
        },
        source: match profile.source {
            scan_module::ScanSource::Flatbed => Source::Flatbed,
            _ => Source::Feeder,
        },
        sides: if profile.source == scan_module::ScanSource::Duplex {
            Sides::Both
        } else {
            Sides::Front
        },
        paper_size: match profile.paper {
            scan_module::Paper::Letter => PaperSize::Letter,
            scan_module::Paper::Legal => PaperSize::Legal,
            scan_module::Paper::A4 => PaperSize::A4,
            scan_module::Paper::A5 => PaperSize::A5,
        },
        use_native_ui: profile.native_interface,
        ocr: profile.searchable_pdf,
        ..Default::default()
    };
    settings.optimize_opts.deskew = profile.cleanup.deskew;
    settings.optimize_opts.despeckle = profile.cleanup.denoise;
    settings.optimize_opts.text_sharpening = if profile.cleanup.sharpen {
        Level::Low
    } else {
        Level::Off
    };
    settings.optimize_opts.crop_borders = profile.cleanup.auto_crop;
    settings.optimize_opts.background_removal = if profile.cleanup.remove_background {
        Level::Low
    } else {
        Level::Off
    };
    settings.optimize_opts.remove_blank_pages = profile.cleanup.skip_blank_pages;
    settings.optimize_opts.jpeg_quality = profile.jpeg_quality;
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend, pipeline, settings::ScanSettings};

    #[test]
    fn two_batches_survive_reopen_with_exact_originals_and_bitonal_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("original.png");
        image::GrayImage::from_fn(240, 320, |x, y| {
            image::Luma([
                if (30..210).contains(&x) && (40..280).contains(&y) && y % 24 < 6 {
                    0
                } else {
                    240
                },
            ])
        })
        .save(&input)
        .unwrap();
        let original = std::fs::read(&input).unwrap();
        let settings = ScanSettings::default();
        let device = backend::Device {
            backend: backend::FILE_IMPORT_BACKEND,
            id: "import".into(),
            name: "Synthetic input".into(),
        };
        let batch = pipeline::scan_and_clean(&device, &settings, &[input], &|_| {}).unwrap();
        assert!(
            batch[0]
                .image
                .to_luma8()
                .pixels()
                .all(|p| matches!(p[0], 0 | 255))
        );
        let first = append(dir.path(), None, &batch).unwrap();
        let second = append(dir.path(), Some(&first), &batch).unwrap();
        write_settings(&second, &settings).unwrap();
        let reopened = ScanSession::load(&second.directory).unwrap();
        assert_eq!(reopened.pages.len(), 2);
        for page in &reopened.pages {
            assert_eq!(
                std::fs::read(reopened.resolve(&page.original_path).unwrap()).unwrap(),
                original
            );
        }
        let loaded = pages(&reopened).unwrap();
        assert_eq!(loaded[0].image, batch[0].image);
        let pdf = lopdf::Document::load_mem(&pdf_bytes(&reopened, &settings).unwrap()).unwrap();
        assert_eq!(pdf.get_pages().len(), 2);
        assert!(pdf.objects.values().any(|o| o.as_stream().is_ok_and(|s| {
            s.dict
                .get(b"Filter")
                .is_ok_and(|f| f.as_name().is_ok_and(|n| n == b"CCITTFaxDecode"))
        })));
    }

    #[test]
    fn rescan_commit_removes_only_replaced_owned_files_and_preserves_failed_batches() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("user-original.png");
        image::RgbImage::from_pixel(30, 40, image::Rgb([255; 3]))
            .save(&input)
            .unwrap();
        let mut page = Page::from_image(image::open(&input).unwrap(), 300);
        page.source = Some(input.clone());
        let first = append(dir.path(), None, std::slice::from_ref(&page)).unwrap();
        let held = append(dir.path(), Some(&first), &[page.clone(), page.clone()]).unwrap();
        let ids = held.pages[1..]
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>();
        let files = held.pages[1..]
            .iter()
            .flat_map(|p| [&p.original_path, &p.image_path])
            .map(|p| held.resolve(p).unwrap())
            .collect::<Vec<_>>();
        let manifest = std::fs::read(held.directory.join("session.json")).unwrap();
        assert!(replace_last_batch(&held, &ids, &[]).is_err());
        let mut missing = page.clone();
        missing.source = Some(dir.path().join("missing.png"));
        assert!(replace_last_batch(&held, &ids, &[missing]).is_err());
        assert_eq!(
            std::fs::read(held.directory.join("session.json")).unwrap(),
            manifest
        );
        assert!(files.iter().all(|p| p.is_file()));
        assert!(
            replace_last_batch(
                &held,
                &[first.pages[0].id.clone()],
                std::slice::from_ref(&page)
            )
            .is_err()
        );
        let (replacement, warnings) = replace_last_batch(&held, &ids, &[page]).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(replacement.pages.len(), 2);
        assert_eq!(replacement.pages[0], first.pages[0]);
        assert!(files.iter().all(|p| !p.exists()));
        assert!(input.is_file());
        assert_eq!(
            ScanSession::load(&held.directory).unwrap().pages,
            replacement.pages
        );
    }

    #[test]
    fn failed_append_does_not_change_committed_session() {
        let dir = tempfile::tempdir().unwrap();
        let image = image::DynamicImage::ImageLuma8(image::GrayImage::new(20, 20));
        let page = Page::from_image(image, 300);
        let session = append(dir.path(), None, std::slice::from_ref(&page)).unwrap();
        let manifest = std::fs::read(session.directory.join("session.json")).unwrap();
        let mut missing = page;
        missing.source = Some(dir.path().join("missing.png"));
        assert!(append(dir.path(), Some(&session), &[missing]).is_err());
        assert_eq!(
            std::fs::read(session.directory.join("session.json")).unwrap(),
            manifest
        );
        assert_eq!(
            ScanSession::load(&session.directory).unwrap().pages.len(),
            1
        );
    }
}
