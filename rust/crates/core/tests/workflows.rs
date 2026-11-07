#![allow(clippy::expect_used)] // Synthetic test fixture helper.
use image::{Rgb, RgbImage};
use openscan_core::*;
use std::{fs, path::Path};

fn image(path: &Path) {
    let mut raster = RgbImage::from_pixel(300, 450, Rgb([255; 3]));
    for y in 100..110 {
        for x in 50..250 {
            raster.put_pixel(x, y, Rgb([20, 20, 20]));
        }
    }
    raster.save(path).expect("fixture");
}
#[test]
fn correction_reorder_export_and_append_preserve_originals() {
    let temp = tempfile::tempdir().expect("temp");
    let first = temp.path().join("first.png");
    image(&first);
    let original = fs::read(&first).expect("read");
    let profile = Profile {
        dpi: 150,
        ..Default::default()
    };
    let cancel = Cancellation::default();
    let mut session = Session::new(Some(temp.path().join("session"))).expect("session");
    session
        .add(
            RawPage {
                path: first.clone(),
                dpi: 150,
            },
            &profile,
            &cancel,
        )
        .expect("add");
    session.reprocess(0, &profile, 90, &cancel).expect("rotate");
    assert_eq!(fs::read(&first).expect("read"), original);
    assert_eq!(
        fs::read(&session.pages[0].original_path).expect("read"),
        original
    );
    assert_eq!(
        correction::read(&session.pages[0].image_path)
            .expect("image")
            .width(),
        450
    );
    let pdf = temp.path().join("scan.pdf");
    pdf::export(&session.pages, &pdf, &profile, false, &cancel).expect("export");
    assert!(pdf::export(&session.pages, &pdf, &profile, false, &cancel).is_err());
    let existing = fs::read(&pdf).expect("read");
    let appended = temp.path().join("appended.pdf");
    pdf::append(&pdf, &session.pages, &appended, &profile, &cancel).expect("append");
    assert_eq!(fs::read(&pdf).expect("read"), existing);
    assert_eq!(
        lopdf::Document::load(appended)
            .expect("PDF")
            .get_pages()
            .len(),
        2
    );
    assert_eq!(
        Session::load(&session.directory).expect("load").pages.len(),
        1
    );
}
#[test]
fn cancellation_still_records_received_original() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("input.png");
    image(&path);
    let mut session = Session::new(Some(temp.path().join("session"))).expect("session");
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(
        session
            .add(
                RawPage {
                    path: path.clone(),
                    dpi: 300
                },
                &Profile::default(),
                &cancel
            )
            .is_err()
    );
    let restored = Session::load(&session.directory).expect("recovery");
    assert_eq!(restored.pages.len(), 1);
    assert_eq!(
        fs::read(&restored.pages[0].original_path).expect("original"),
        fs::read(path).expect("input")
    );
}
#[test]
fn forged_session_paths_cannot_read_other_files() {
    let temp = tempfile::tempdir().expect("temp");
    let mut session = Session::new(Some(temp.path().join("session"))).expect("session");
    let external = temp.path().join("external.png");
    image(&external);
    session.pages.push(Page {
        id: "outside".into(),
        original_path: external.clone(),
        image_path: external,
        dpi: 300,
        deskew_angle: 0.,
        ink_percent: 0.,
        excluded: false,
        rotation: 0,
    });
    session.save().expect("save");
    assert!(Session::load(&session.directory).is_err());
}
#[test]
fn grayscale_pdf_declares_the_channels_it_actually_encodes() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("input.png");
    image(&path);
    let profile = Profile {
        color: Color::Grayscale,
        ..Default::default()
    };
    let mut session = Session::new(Some(temp.path().join("session"))).expect("session");
    session
        .add(
            RawPage { path, dpi: 300 },
            &profile,
            &Cancellation::default(),
        )
        .expect("add");
    let pdf = temp.path().join("gray.pdf");
    pdf::export(
        &session.pages,
        &pdf,
        &profile,
        false,
        &Cancellation::default(),
    )
    .expect("export");
    let doc = lopdf::Document::load(pdf).expect("PDF");
    let stream = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find(|s| s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image"))
        .expect("image stream");
    assert_eq!(
        stream
            .dict
            .get(b"ColorSpace")
            .expect("color space")
            .as_name()
            .expect("name"),
        b"DeviceRGB"
    );
    let decoded = image::load_from_memory(&stream.content).expect("JPEG");
    assert_eq!(decoded.color().channel_count(), 3);
}

#[test]
fn multipage_tiff_import_keeps_all_frames_and_source() {
    let temp = tempfile::tempdir().expect("temp");
    let input = temp.path().join("pages.tiff");
    {
        let file = fs::File::create(&input).expect("file");
        let mut encoder = tiff::encoder::TiffEncoder::new(file).expect("encoder");
        encoder
            .write_image::<tiff::encoder::colortype::Gray8>(32, 40, &vec![255; 32 * 40])
            .expect("gray");
        encoder
            .write_image::<tiff::encoder::colortype::RGB8>(48, 32, &vec![120; 48 * 32 * 3])
            .expect("color");
    }
    let original = fs::read(&input).expect("read");
    let mut session = Session::new(Some(temp.path().join("session"))).expect("session");
    session
        .import(&input, &Profile::default(), &Cancellation::default())
        .expect("import");
    assert_eq!(session.pages.len(), 2);
    assert_eq!(
        correction::read(&session.pages[1].original_path)
            .expect("image")
            .width(),
        48
    );
    assert_eq!(fs::read(&input).expect("read"), original);
    assert_eq!(
        Session::load(&session.directory)
            .expect("reload")
            .pages
            .len(),
        2
    );
}

#[test]
fn invalid_page_resolution_cannot_create_a_pdf() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("input.png");
    image(&path);
    let mut s = Session::new(Some(temp.path().join("session"))).expect("session");
    s.add(
        RawPage { path, dpi: 300 },
        &Profile::default(),
        &Cancellation::default(),
    )
    .expect("add");
    s.pages[0].dpi = 0;
    let output = temp.path().join("invalid.pdf");
    assert!(
        pdf::export(
            &s.pages,
            &output,
            &Profile::default(),
            false,
            &Cancellation::default()
        )
        .is_err()
    );
    assert!(!output.exists());
}
