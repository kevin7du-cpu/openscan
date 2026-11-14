use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use scan_module::{CancellationToken, Capabilities, ScanBatch, ScanRequest};
use std::time::Instant;

struct NoScanner;
impl ScannerBackend for NoScanner {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "No hardware",
            driver_dialog: false,
            programmatic_settings: false,
            cancellation_note: "None",
        }
    }
    fn devices(&self) -> scan_module::Result<Vec<Device>> {
        Ok(vec![])
    }
    fn acquire(
        &self,
        _: &ScanRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(scan_module::Progress),
    ) -> scan_module::Result<ScanBatch> {
        unreachable!()
    }
}
fn settle(h: &mut Harness<'_, LegacyOpenScanUi>) {
    let start = Instant::now();
    loop {
        h.step();
        if !h.state().is_busy() {
            h.step();
            if !h.state().is_busy() {
                break;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "Worker stalled: {:?}",
            h.state().error()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    h.run();
}
fn panel(temp: &tempfile::TempDir) -> Harness<'static, LegacyOpenScanUi> {
    let source = temp.path().join("source.png");
    image::RgbImage::from_fn(80, 120, |x, y| image::Rgb([x as u8, y as u8, 150]))
        .save(&source)
        .unwrap();
    let mut panel = LegacyOpenScanUi::with_backend(
        temp.path().join("scan"),
        temp.path().join("models"),
        Box::new(NoScanner),
    );
    panel.profile.cleanup.deskew = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180., 840.))
        .build_ui_state(|ui, p: &mut LegacyOpenScanUi| p.show(ui), panel);
    settle(&mut h);
    assert!(
        !h.state().profile.cleanup.deskew,
        "Respect the host's explicit initial settings"
    );
    h.state_mut().import_images(vec![source.clone(), source]);
    settle(&mut h);
    h
}

#[test]
fn correction_dialog_saves_reloads_and_cancel_keeps_previous_choices() {
    let temp = tempfile::tempdir().unwrap();
    let mut h = panel(&temp);
    let before = h.state().session().unwrap().pages.clone();
    assert!(h.state().profile.cleanup.auto_crop && h.state().profile.cleanup.denoise);
    assert!(h.query_by_label("Crop to content").is_none());
    h.get_by_label("Image correction").click();
    h.run();
    h.get_by_label("Crop to content").click();
    h.get_by_label("Remove dust / speckles").click();
    h.run();
    h.get_by_label("Save settings").click();
    settle(&mut h);
    assert!(h.state().correction_editor.is_none());
    assert!(!h.state().profile.cleanup.auto_crop && !h.state().profile.cleanup.denoise);
    assert_eq!(
        h.state().session().unwrap().pages,
        before,
        "Saving settings must not alter existing scans"
    );
    let saved = h.state().profile.cleanup.clone();
    h.get_by_label("Image correction").click();
    h.run();
    h.get_by_label("Crop to content").click();
    h.run();
    h.get_by_label("Close window").click();
    h.run();
    assert_eq!(h.state().profile.cleanup, saved);
    drop(h);
    let panel = LegacyOpenScanUi::with_backend(
        temp.path().join("scan"),
        temp.path().join("models"),
        Box::new(NoScanner),
    );
    let mut reopened = Harness::builder()
        .with_size(egui::vec2(1180., 840.))
        .build_ui_state(|ui, p: &mut LegacyOpenScanUi| p.show(ui), panel);
    settle(&mut reopened);
    assert_eq!(reopened.state().profile.cleanup, saved);
    // A blocked settings destination reports failure and keeps the editor/draft.
    let path = temp.path().join("scan/image-correction.json");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    reopened.get_by_label("Image correction").click();
    reopened.run();
    reopened.get_by_label("Crop to content").click();
    reopened.run();
    reopened.get_by_label("Save settings").click();
    settle(&mut reopened);
    assert!(reopened.state().error().is_some());
    assert!(reopened.state().correction_editor.is_some());
    assert_eq!(reopened.state().profile.cleanup, saved);
}

#[test]
fn bottom_corrections_undo_redo_tooltips_and_right_click_delete_work() {
    let temp = tempfile::tempdir().unwrap();
    let mut h = panel(&temp);
    let before = h.state().session().unwrap().clone();
    h.state_mut().enable_scan_notes();
    h.run();
    assert!(h.query_by_label("OCR clean copy…").is_none());
    assert!(
        h.get_by_label("OCR Clean").rect().right() <= h.get_by_label("Scan Note").rect().left()
    );
    h.get_by_label("Scan Note").click();
    h.run();
    assert!(
        h.state_mut()
            .take_events()
            .iter()
            .any(|e| matches!(e, OpenScanEvent::ScanNote(s) if s.pages.len() == 2))
    );
    assert!(h.query_by_label("Include / exclude").is_none());
    assert!(h.get_by_label("Undo").rect().right() <= h.get_by_label("Redo").rect().left());
    assert!(h.get_by_label("Redo").rect().right() <= h.get_by_label("Rotate 90°").rect().left());
    assert!(
        h.get_by_label("Apply corrections").rect().top()
            > h.get_by_label("Fit scanned page").rect().bottom()
    );
    h.state_mut().profile.cleanup.brightness = 30;
    h.get_by_label("Apply corrections").click();
    settle(&mut h);
    let corrected = h.state().session().unwrap().clone();
    assert_ne!(corrected.pages, before.pages);
    h.get_by_label("Undo").hover();
    h.run_steps(90);
    assert!(h.query_by_label("Undo: Apply corrections").is_some());
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages, before.pages);
    h.get_by_label("Redo").hover();
    h.run_steps(90);
    assert!(h.query_by_label("Redo: Apply corrections").is_some());
    h.get_by_label("Redo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages, corrected.pages);

    h.get_by_label("Page 2").click();
    settle(&mut h);
    h.get_by_label("Page 1").click_secondary();
    h.run();
    h.get_by_label("Delete page").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 1);
    assert_eq!(h.state().session().unwrap().pages[0].id, before.pages[1].id);
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages, corrected.pages);
    h.get_by_label("Scanned page preview").click_secondary();
    h.run();
    h.get_by_label("Delete page").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 1);
    h.get_by_label("Delete selected page").click();
    settle(&mut h);
    assert!(h.state().session().unwrap().pages.is_empty());
    assert!(h.state().texture.is_none());
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 1);
    assert!(h.state().texture.is_some());
}

#[test]
fn apply_clean_copy_updates_only_reviewed_page_closes_review_and_can_be_undone() {
    let temp = tempfile::tempdir().unwrap();
    let mut h = panel(&temp);
    h.get_by_label("Page 2").click();
    settle(&mut h);
    let before = h.state().session().unwrap().clone();
    let page = &before.pages[1];
    let original = std::fs::read(before.resolve(&page.original_path).unwrap()).unwrap();
    let cleaned = image::RgbImage::from_pixel(80, 120, image::Rgb([255, 255, 255]));
    let image_path = PathBuf::from("processed/reviewed-fixture.png");
    cleaned.save(before.directory.join(&image_path)).unwrap();
    let data = worker::CleanPreview {
        draft: worker::CleanDraft {
            page_id: page.id.clone(),
            source: page.image_path.clone(),
            dpi: 300,
            size: [80, 120],
            lines: vec![],
            options: Default::default(),
            erasures: vec![],
        },
        image_path,
        original: image::RgbaImage::new(80, 120),
        cleaned: image::DynamicImage::ImageRgb8(cleaned.clone()).to_rgba8(),
    };
    let review = clean_copy::Review::new(&h.ctx, data);
    h.state_mut().clean_review = Some(review);
    h.run();
    assert!(h.query_by_label("Save clean PDF…").is_none());
    h.get_by_label("Apply").click();
    settle(&mut h);
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    assert!(h.state().clean_review.is_none());
    let applied = h.state().session().unwrap().clone();
    assert_eq!(applied.pages[0], before.pages[0]);
    assert_ne!(applied.pages[1].image_path, before.pages[1].image_path);
    assert_eq!(h.state().selected, 1);
    assert_eq!(
        h.state().texture_path,
        Some(applied.directory.join(&applied.pages[1].image_path))
    );
    assert_eq!(
        scan_module::processing::load(&applied.resolve(&applied.pages[1].image_path).unwrap())
            .unwrap(),
        cleaned
    );
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages, before.pages);
    h.get_by_label("Redo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages, applied.pages);
    assert_eq!(
        std::fs::read(before.resolve(&page.original_path).unwrap()).unwrap(),
        original
    );
}
