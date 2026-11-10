use image::{Rgb, RgbImage};
use scan_module::{CancellationToken, ScanProfile, history::ScanHistory, session::ScanSession};

#[test]
fn balanced_reprocessing_preserves_saved_original_and_restores_exact_gray_with_undo() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("mottled.png");
    let original = RgbImage::from_fn(160, 200, |x, y| {
        Rgb([if (60..63).contains(&x) && (70..120).contains(&y) {
            30
        } else {
            200 + ((x * 31 + y * 17) % 35) as u8
        }; 3])
    });
    original.save(&source).unwrap();
    let original_bytes = std::fs::read(&source).unwrap();
    let mut profile = ScanProfile {
        color: scan_module::ScanColor::Grayscale,
        ..Default::default()
    };
    profile.cleanup.auto_crop = false;
    profile.cleanup.deskew = false;
    profile.cleanup.denoise = false;
    let token = CancellationToken::default();
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    session.add(&source, &profile, 0, &token).unwrap();
    let gray = session.clone();
    profile.color = scan_module::ScanColor::BlackWhite;
    session.reprocess(0, &profile, None, &token).unwrap();
    let cleaned = session.clone();
    let mut history = ScanHistory::default();
    history.record(&gray, &session, "Apply corrections");
    let pixels =
        scan_module::processing::load(&session.resolve(&session.pages[0].image_path).unwrap())
            .unwrap();
    assert_ne!(pixels, original);
    assert!(pixels.pixels().all(|p| p.0 == [0; 3] || p.0 == [255; 3]));
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, gray.pages);
    assert_eq!(
        scan_module::processing::load(&session.resolve(&session.pages[0].image_path).unwrap())
            .unwrap(),
        original
    );
    history.redo(&mut session).unwrap();
    assert_eq!(session.pages, cleaned.pages);
    let reopened = ScanSession::load(&session.directory).unwrap();
    assert_eq!(reopened.pages, cleaned.pages);
    assert_eq!(
        std::fs::read(session.resolve(&session.pages[0].original_path).unwrap()).unwrap(),
        original_bytes
    );
    assert_eq!(std::fs::read(source).unwrap(), original_bytes);
}

fn fixture(root: &std::path::Path) -> (ScanSession, ScanProfile) {
    let source = root.join("page.png");
    RgbImage::from_fn(80, 120, |x, y| Rgb([x as u8, y as u8, 125]))
        .save(&source)
        .unwrap();
    let mut profile = ScanProfile::default();
    profile.cleanup.deskew = false;
    let mut session = ScanSession::create(root.join("session")).unwrap();
    for _ in 0..2 {
        session
            .add(&source, &profile, 0, &CancellationToken::default())
            .unwrap();
    }
    (session, profile)
}

#[test]
fn undo_redo_restore_exact_images_corrections_deletion_order_and_saved_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, mut profile) = fixture(temp.path());
    let initial = session.clone();
    let original =
        std::fs::read(session.resolve(&session.pages[1].original_path).unwrap()).unwrap();
    let clean = RgbImage::from_fn(80, 120, |x, y| Rgb([255, (x + y) as u8, 255]));
    let mut history = ScanHistory::default();
    let page = session.pages[1].clone();
    session
        .apply_image(
            &page.id,
            &page.image_path,
            &clean,
            &CancellationToken::default(),
        )
        .unwrap();
    history.record(&initial, &session, "Apply clean copy");
    let applied = session.clone();
    assert_eq!(session.pages[0], initial.pages[0]);
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, initial.pages);
    history.redo(&mut session).unwrap();
    assert_eq!(session.pages, applied.pages);
    assert_eq!(
        scan_module::processing::load(&session.resolve(&session.pages[1].image_path).unwrap())
            .unwrap(),
        clean
    );

    let before = session.clone();
    session
        .rotate_page(1, &CancellationToken::default())
        .unwrap();
    history.record(&before, &session, "Rotate page");
    assert_eq!(
        scan_module::processing::load(&session.resolve(&session.pages[1].image_path).unwrap())
            .unwrap(),
        image::imageops::rotate90(&clean)
    );
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, applied.pages);

    // Corrections may recompute from the acquisition original. Undo must bring
    // back the applied clean image, not simply reverse the correction settings.
    profile.cleanup.brightness = 30;
    let before = session.clone();
    session
        .reprocess(1, &profile, None, &CancellationToken::default())
        .unwrap();
    history.record(&before, &session, "Apply corrections");
    let corrected = session.clone();
    assert!(history.redo_label().is_none());
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, applied.pages);
    history.redo(&mut session).unwrap();
    assert_eq!(session.pages, corrected.pages);

    let before = session.clone();
    session.delete_page(1).unwrap();
    history.record(&before, &session, "Delete page");
    assert_eq!(session.pages.len(), 1);
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, corrected.pages);
    let before = session.clone();
    session.move_page(1, 0).unwrap();
    history.record(&before, &session, "Reorder pages");
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, corrected.pages);
    assert_eq!(
        ScanSession::load(&session.directory).unwrap().pages,
        session.pages
    );
    assert_eq!(
        std::fs::read(session.resolve(&page.original_path).unwrap()).unwrap(),
        original
    );
}

#[test]
fn failed_or_cancelled_edits_and_failed_undo_leave_current_state_and_history_intact() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _) = fixture(temp.path());
    let initial = session.clone();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(session.rotate_page(0, &cancel).is_err());
    let page = session.pages[0].clone();
    assert!(
        session
            .apply_image(&page.id, &page.image_path, &RgbImage::new(80, 120), &cancel)
            .is_err()
    );
    assert_eq!(session.pages, initial.pages);

    let mut history = ScanHistory::default();
    session.delete_page(0).unwrap();
    history.record(&initial, &session, "Delete page");
    let deleted = session.clone();
    let manifest = session.directory.join("session.json");
    let saved = session.directory.join("session.saved.json");
    std::fs::rename(&manifest, &saved).unwrap();
    std::fs::create_dir(&manifest).unwrap();
    assert!(history.undo(&mut session).is_err());
    assert_eq!(session.pages, deleted.pages);
    assert_eq!(history.undo_label(), Some("Delete page"));
    assert!(history.redo_label().is_none());
    std::fs::remove_dir(&manifest).unwrap();
    std::fs::rename(saved, manifest).unwrap();
    history.undo(&mut session).unwrap();
    assert_eq!(session.pages, initial.pages);
}

#[test]
fn stale_review_and_history_for_a_different_session_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (mut session, _) = fixture(temp.path());
    let before = session.clone();
    session
        .rotate_page(0, &CancellationToken::default())
        .unwrap();
    let changed = session.clone();
    let old = &before.pages[0];
    assert!(
        session
            .apply_image(
                &old.id,
                &old.image_path,
                &RgbImage::new(80, 120),
                &CancellationToken::default()
            )
            .is_err()
    );
    assert_eq!(session.pages, changed.pages);
    let mut history = ScanHistory::default();
    history.record(&before, &session, "Rotate page");
    let mut other = ScanSession::create(temp.path().join("other")).unwrap();
    assert!(history.undo(&mut other).is_err());
    assert!(other.pages.is_empty());
    assert_eq!(history.undo_label(), Some("Rotate page"));
}
