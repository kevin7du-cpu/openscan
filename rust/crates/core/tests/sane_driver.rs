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
    let profile = Profile {
        device_id: Some(device.id.clone()),
        dpi: 75,
        paper: Paper::A5,
        max_pages: 1,
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
    let output = directory.path().join("scan.pdf");
    pdf::export(&session.pages, &output, &profile, false, &cancel).expect("PDF");
    assert_eq!(
        lopdf::Document::load(output)
            .expect("PDF")
            .get_pages()
            .len(),
        1
    );
}
