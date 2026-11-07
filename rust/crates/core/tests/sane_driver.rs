//! Enabled explicitly with SANE's own synthetic test driver on Linux CI.
#![cfg(target_os = "linux")]
use openscan_core::{
    backend::{Backend, sane::Sane},
    *,
};

#[test]
#[ignore = "requires sane-utils and SANE_CONFIG_DIR enabling the synthetic test backend"]
fn installed_sane_test_driver_acquires_and_exports() {
    let directory = tempfile::tempdir().expect("temp");
    let backend = Sane::system().expect("installed scanimage");
    let cancel = Cancellation::default();
    let devices = backend.devices(&cancel).expect("discovery");
    let device = devices
        .iter()
        .find(|d| d.id == "sane:test:0")
        .expect("test device");
    let caps = backend.capabilities(&device.id).expect("capabilities");
    assert!(caps.sources.iter().any(|s| s.source == Source::Flatbed));
    let flatbed = caps
        .sources
        .iter()
        .find(|s| s.source == Source::Flatbed)
        .expect("flatbed");
    // CI configures a 75-DPI step: 100/200/400 must not be advertised or accepted.
    assert_eq!(flatbed.dpi, vec![75, 150, 300, 600]);
    let profile = Profile {
        device_id: Some(device.id.clone()),
        dpi: 75,
        paper: Paper::A5,
        max_pages: 1,
        cleanup: Cleanup {
            deskew: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut session = Session::new(Some(directory.path().join("session"))).expect("session");
    let raw = session.acquisition_directory().expect("raw");
    backend
        .acquire(&profile, &raw, &cancel, &mut |page| {
            session.add(page, &profile, &cancel)
        })
        .expect("scan");
    assert_eq!(session.pages.len(), 1);
    let raster = image::open(&session.pages[0].original_path).expect("full A5 raster");
    let (width, height) = profile.paper.millimeters();
    let expected_width = (width * profile.dpi as f32 / 25.4).round() as u32;
    let expected_height = (height * profile.dpi as f32 / 25.4).round() as u32;
    assert!(
        raster.width().abs_diff(expected_width) <= 1,
        "paper width was silently clamped"
    );
    assert!(
        raster.height().abs_diff(expected_height) <= 1,
        "paper height was silently clamped"
    );
    let output = directory.path().join("scan.pdf");
    pdf::export(&session.pages, &output, &profile, false, &cancel).expect("PDF");
    assert_eq!(
        lopdf::Document::load(output)
            .expect("PDF")
            .get_pages()
            .len(),
        1
    );

    let unsupported = Profile {
        dpi: 100,
        ..profile
    };
    let rejected = directory.path().join("unsupported-dpi");
    let error = backend
        .acquire(&unsupported, &rejected, &cancel, &mut |_| {
            panic!("unsupported resolution must not scan")
        })
        .expect_err("unsupported DPI");
    assert!(
        error.to_string().contains("do not support 100 DPI"),
        "{error}"
    );
    assert!(
        !rejected.exists(),
        "validation must happen before acquisition"
    );
}
