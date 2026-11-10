use image::{Rgb, RgbImage};
use scan_module::{presets::PresetStore, processing, session::ScanSession, *};
use std::{fs, path::Path};

fn profile() -> ScanProfile {
    let mut p = ScanProfile::default();
    p.cleanup.deskew = false;
    // Individual tests opt into corrections so they can compare each operation.
    p.cleanup.auto_crop = false;
    p.cleanup.denoise = false;
    p
}
fn fixture(path: &Path) -> RgbImage {
    let mut image = RgbImage::from_pixel(180, 240, Rgb([255, 255, 255]));
    for y in 65..175 {
        for x in 50..130 {
            image.put_pixel(x, y, Rgb([30, 60, 90]));
        }
    }
    image.save(path).unwrap();
    image
}
#[test]
fn profiles_match_csharp_json_and_reject_unknown_or_invalid_values() {
    let text = r#"{"schemaVersion":1,"name":"Office","source":"duplex","color":"blackWhite","paper":"a4","dpi":300,"cleanup":{"deskew":true,"skipBlankPages":true}}"#;
    let profile: ScanProfile = serde_json::from_str(text).unwrap();
    profile.validate().unwrap();
    assert_eq!(profile.source, ScanSource::Duplex);
    assert_eq!(profile.color, ScanColor::BlackWhite);

    let settings = ScanSettings::from(&profile);
    assert_eq!(settings.duplex, Some(true));
    assert_eq!(settings.paper, Some(Paper::A4));
    assert!(serde_json::from_str::<ScanProfile>(r#"{"unknownSetting":true}"#).is_err());
    let mut invalid = profile;
    invalid.cleanup.blank_ink_percent = f64::NAN;
    assert!(invalid.validate().is_err());
}
#[test]
fn presets_are_atomic_case_insensitive_and_names_never_become_paths() {
    let temp = tempfile::tempdir().unwrap();
    let store = PresetStore::new(temp.path().join("presets"));
    let mut p = profile();
    p.name = " ../Office/Records ".into();
    store.save(&p, false).unwrap();
    p.name = "../office/records".into();
    assert!(store.save(&p, false).is_err());
    p.dpi = 150;
    store.save(&p, true).unwrap();
    assert_eq!(store.list().unwrap()[0].dpi, 150);
    store.rename(&p.name, "Home").unwrap();
    assert_eq!(store.list().unwrap()[0].name, "Home");
    store.delete("home").unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(temp.path().join("presets/deleted").is_dir());
    assert!(!temp.path().join("Office").exists());
}
#[test]
fn corrections_keep_original_and_support_color_crop_rotation_and_blank_detection() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("page.png");
    let original = fixture(&path);
    let bytes = fs::read(&path).unwrap();
    let mut p = profile();
    p.cleanup.auto_crop = true;
    p.dpi = 75;
    p.cleanup.rotate_degrees = 90;
    let result = processing::correct(&original, &p, 0, &CancellationToken::default()).unwrap();
    assert!(result.image.width() < 240 && result.image.height() < 180);
    assert!(!result.blank);
    p.cleanup.auto_crop = false;
    p.color = ScanColor::Grayscale;
    let gray = processing::correct(&original, &p, 0, &CancellationToken::default()).unwrap();
    assert_eq!(gray.image.dimensions(), (240, 180));
    assert!(gray.image.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
    p.color = ScanColor::BlackWhite;
    let bw = processing::correct(&original, &p, 0, &CancellationToken::default()).unwrap();
    assert!(bw.image.pixels().all(|p| p[0] == 0 || p[0] == 255));
    let blank = RgbImage::from_pixel(180, 240, Rgb([255, 255, 255]));
    assert!(
        processing::correct(&blank, &p, 0, &CancellationToken::default())
            .unwrap()
            .blank
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(original.get_pixel(60, 80), &Rgb([30, 60, 90]));
}
#[test]
fn cleanup_controls_change_pixels_without_destroying_input() {
    let mut image = RgbImage::from_pixel(64, 64, Rgb([180, 180, 180]));
    image.put_pixel(20, 20, Rgb([0, 0, 0]));
    let mut p = profile();
    p.cleanup.denoise = true;
    let result = processing::correct(&image, &p, 0, &CancellationToken::default()).unwrap();
    assert_eq!(result.image.get_pixel(20, 20), &Rgb([180, 180, 180]));
    p.cleanup.denoise = false;
    p.cleanup.remove_background = true;
    let result = processing::correct(&image, &p, 0, &CancellationToken::default()).unwrap();
    assert!(result.image.get_pixel(40, 40)[0] > 245);
    p.cleanup.remove_background = false;
    p.cleanup.brightness = 20;
    let result = processing::correct(&image, &p, 0, &CancellationToken::default()).unwrap();
    assert!(result.image.get_pixel(40, 40)[0] > 200);
    p.cleanup.brightness = 0;
    p.cleanup.contrast = 30;
    let result = processing::correct(&image, &p, 0, &CancellationToken::default()).unwrap();
    assert!(result.image.get_pixel(40, 40)[0] > 180);
    let mut edges = RgbImage::from_pixel(64, 64, Rgb([140, 140, 140]));
    for y in 0..64 {
        for x in 0..32 {
            edges.put_pixel(x, y, Rgb([100, 100, 100]));
        }
    }
    p.cleanup.contrast = 0;
    p.cleanup.sharpen = true;
    let result = processing::correct(&edges, &p, 0, &CancellationToken::default()).unwrap();
    assert!(result.image.get_pixel(31, 32)[0] < 100);
    assert!(result.image.get_pixel(32, 32)[0] > 140);
}

#[test]
fn content_crop_ignores_connected_edge_bars_and_keeps_original_reversible() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("edge-marks.png");
    let mut original = RgbImage::from_pixel(1000, 1400, Rgb([255, 255, 255]));
    for y in 0..1400 {
        for x in 0..8 {
            original.put_pixel(x, y, Rgb([0, 0, 0]));
            original.put_pixel(999 - x, y, Rgb([0, 0, 0]));
        }
    }
    for y in 1392..1400 {
        for x in 0..1000 {
            original.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    for y in 140..1260 {
        for x in 120..880 {
            original.put_pixel(x, y, Rgb([30, 60, 90]));
        }
    }
    original.save(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mut p = profile();
    let token = CancellationToken::default();
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    session
        .import_images(&[path], &p, &token, &mut |_| {})
        .unwrap();
    p.cleanup.auto_crop = true;
    session.reprocess(0, &p, None, &token).unwrap();
    let cropped =
        processing::load(&session.resolve(&session.pages[0].image_path).unwrap()).unwrap();
    assert_eq!(cropped.dimensions(), (832, 1192));
    assert!(!cropped.pixels().any(|p| *p == Rgb([0, 0, 0])));
    assert_eq!(
        cropped
            .pixels()
            .filter(|p| **p == Rgb([30, 60, 90]))
            .count(),
        760 * 1120
    );
    p.cleanup.auto_crop = false;
    session.reprocess(0, &p, None, &token).unwrap();
    assert_eq!(
        processing::load(&session.resolve(&session.pages[0].image_path).unwrap()).unwrap(),
        original
    );
    assert_eq!(
        fs::read(session.resolve(&session.pages[0].original_path).unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn content_crop_preserves_compact_marks_near_edges_and_blank_pages() {
    let mut page = RgbImage::from_pixel(1000, 1400, Rgb([255, 255, 255]));
    let mut p = profile();
    p.cleanup.auto_crop = true;
    let token = CancellationToken::default();
    assert_eq!(
        processing::correct(&page, &p, 0, &token).unwrap().image,
        page
    );
    for y in 500..520 {
        for x in 2..18 {
            page.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    for y in 550..575 {
        for x in 955..970 {
            page.put_pixel(x, y, Rgb([30, 60, 90]));
        }
    }
    let cropped = processing::correct(&page, &p, 0, &token).unwrap().image;
    assert_eq!(
        cropped.pixels().filter(|p| **p == Rgb([0, 0, 0])).count(),
        16 * 20
    );
    assert_eq!(
        cropped
            .pixels()
            .filter(|p| **p == Rgb([30, 60, 90]))
            .count(),
        15 * 25
    );
}
#[test]
fn deskew_expands_canvas_and_recovers_known_angle() {
    let mut page = RgbImage::from_pixel(400, 600, Rgb([255, 255, 255]));
    for y in (80..520).step_by(40) {
        for dy in 0..4 {
            for x in 40..360 {
                page.put_pixel(x, y + dy, Rgb([0, 0, 0]));
            }
        }
    }
    let slanted = imageproc::geometric_transformations::rotate_about_center_no_crop(
        &page,
        5_f32.to_radians(),
        imageproc::geometric_transformations::Interpolation::Bilinear,
        imageproc::geometric_transformations::Border::Constant(Rgb([255, 255, 255])),
    );
    let result = processing::correct(
        &slanted,
        &ScanProfile::default(),
        0,
        &CancellationToken::default(),
    )
    .unwrap();
    assert!((result.angle + 5.0).abs() < 0.8, "{}", result.angle);
    assert!(result.image.width() >= slanted.width());
}
#[test]
fn session_reprocess_reorder_exclude_and_reopen_preserve_originals() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("page.png");
    fixture(&source);
    let bytes = fs::read(&source).unwrap();
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    let p = profile();
    session
        .add(&source, &p, 0, &CancellationToken::default())
        .unwrap();
    session
        .add(&source, &p, 0, &CancellationToken::default())
        .unwrap();
    let id = session.pages[0].id.clone();
    let old_image = session.pages[0].image_path.clone();
    session
        .reprocess(0, &p, Some(90), &CancellationToken::default())
        .unwrap();
    assert_ne!(session.pages[0].image_path, old_image);
    assert_eq!(
        processing::load(&session.resolve(&session.pages[0].image_path).unwrap())
            .unwrap()
            .dimensions(),
        (240, 180)
    );
    session.move_page(0, 1).unwrap();
    session.toggle(1).unwrap();
    let reopened = ScanSession::load(&session.directory).unwrap();
    assert_eq!(reopened.pages[1].id, id);
    assert!(reopened.pages[1].excluded);
    assert_eq!(
        fs::read(reopened.resolve(&reopened.pages[1].original_path).unwrap()).unwrap(),
        bytes
    );
    assert!(session.resolve(Path::new("../page.png")).is_err());
    let destination = temp.path().join("images");
    session
        .export_images(&destination, &CancellationToken::default())
        .unwrap();
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    assert!(
        session
            .export_images(&destination, &CancellationToken::default())
            .is_err()
    );
}
struct PartialScanner;
impl ScannerBackend for PartialScanner {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "fixture",
            driver_dialog: false,
            programmatic_settings: true,
            cancellation_note: "Immediate",
        }
    }
    fn devices(&self) -> Result<Vec<Device>> {
        Ok(vec![Device {
            id: "fixture".into(),
            name: "Fixture".into(),
        }])
    }
    fn acquire(
        &self,
        r: &ScanRequest,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch> {
        validate_request(r)?;
        assert_eq!(r.settings.dpi, Some(300));
        let path = r.output_dir.join("page.png");
        fixture(&path);
        progress(Progress::PageReady(path));
        cancel.cancel();
        Err(ScanError::Backend("Paper jam".into()))
    }
}
#[test]
fn cancellation_and_driver_failure_keep_completed_pages_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    let result = session
        .acquire(
            &PartialScanner,
            &profile(),
            &CancellationToken::default(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(result.received, 1);
    assert!(result.warning.unwrap().contains("Paper jam"));
    assert_eq!(
        ScanSession::load(&session.directory).unwrap().pages.len(),
        1
    );
    assert!(session.directory.join("last-acquisition.json").exists());
}
#[test]
fn loads_original_csharp_session_manifests_and_rejects_escaping_paths() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("page.png");
    fixture(&source);
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    session
        .add(&source, &profile(), 0, &CancellationToken::default())
        .unwrap();
    let mut old = session.pages.clone();
    old[0].original_path = session.resolve(&old[0].original_path).unwrap();
    old[0].image_path = session.resolve(&old[0].image_path).unwrap();
    fs::write(
        session.directory.join("session.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ScanSession::load(&session.directory).unwrap().pages.len(),
        1
    );
    old[0].original_path = source;
    fs::write(
        session.directory.join("session.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    assert!(ScanSession::load(&session.directory).is_err());
}
#[test]
fn multipage_tiff_imports_all_frames_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pages.tiff");
    let file = fs::File::create(&path).unwrap();
    let mut encoder = tiff::encoder::TiffEncoder::new(file).unwrap();
    encoder
        .write_image::<tiff::encoder::colortype::RGB8>(32, 40, &vec![255; 32 * 40 * 3])
        .unwrap();
    encoder
        .write_image::<tiff::encoder::colortype::RGB8>(48, 64, &vec![40; 48 * 64 * 3])
        .unwrap();
    drop(encoder);
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    session
        .import_images(
            &[path],
            &profile(),
            &CancellationToken::default(),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(session.pages.len(), 2);
    assert_eq!(
        processing::load(&session.resolve(&session.pages[1].image_path).unwrap())
            .unwrap()
            .dimensions(),
        (48, 64)
    );
}

#[test]
fn png_resolution_survives_import_and_image_export() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("300dpi.png");
    let image = RgbImage::from_pixel(80, 100, Rgb([255, 255, 255]));
    processing::save_png_with_dpi(&image, &source, 300).unwrap();
    assert_eq!(processing::resolution(&source), Some(300));
    let mut p = profile();
    p.dpi = 150;
    let mut session = ScanSession::create(temp.path().join("session")).unwrap();
    session
        .import_images(&[source], &p, &CancellationToken::default(), &mut |_| {})
        .unwrap();
    assert_eq!(session.pages[0].dpi, 300);
    let output = temp.path().join("output");
    session
        .export_images(&output, &CancellationToken::default())
        .unwrap();
    assert_eq!(
        processing::resolution(&output.join("page-0001.png")),
        Some(300)
    );
}

#[cfg(feature = "documents")]
#[test]
fn rust_pdf_export_dimensions_text_and_append_preserve_source() {
    use scan_module::documents::{ExportDocument, ExportPage, TextLine, pdf};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("page.png");
    fixture(&path);
    let doc = ExportDocument {
        title: "Test".into(),
        document_type: "Scan".into(),
        tags: "test".into(),
        pages: vec![ExportPage {
            image: path,
            width: 180,
            height: 240,
            dpi: 300,
            lines: vec![TextLine {
                text: "Résumé TEST-4827".into(),
                bounds: [10.0, 10.0, 150.0, 20.0],
            }],
        }],
    };
    let bytes = pdf::export(&doc, Some(80), &CancellationToken::default()).unwrap();
    let parsed = lopdf::Document::load_mem(&bytes).unwrap();
    assert_eq!(parsed.get_pages().len(), 1);
    assert!(
        parsed
            .extract_text(&[1])
            .unwrap()
            .contains("Résumé TEST-4827")
    );
    let page = parsed
        .get_object(*parsed.get_pages().get(&1).unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let bounds = page.get(b"MediaBox").unwrap().as_array().unwrap();
    assert!((bounds[2].as_float().unwrap() - 43.2).abs() < 0.01);
    let source = temp.path().join("original.pdf");
    pdf::save_new(&source, &bytes).unwrap();
    assert!(pdf::save_new(&source, &bytes).is_err());
    let combined = pdf::append_copy(&source, &bytes, &CancellationToken::default()).unwrap();
    let merged = lopdf::Document::load_mem(&combined).unwrap();
    assert_eq!(merged.get_pages().len(), 2);
    assert!(merged.extract_text(&[2]).unwrap().contains("TEST-4827"));
    assert_eq!(fs::read(source).unwrap(), bytes);
}
