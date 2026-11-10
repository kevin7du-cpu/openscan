use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use openscan_egui::{OpenScanEvent, OpenScanUi};
use scan_module::*;
use std::time::{Duration, Instant};
struct FixtureScanner;
impl ScannerBackend for FixtureScanner {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "Fixture",
            driver_dialog: true,
            programmatic_settings: true,
            cancellation_note: "Immediate",
        }
    }
    fn devices(&self) -> Result<Vec<Device>> {
        Ok(vec![Device {
            id: "fixture".into(),
            name: "Fixture scanner".into(),
        }])
    }
    fn device_capabilities(&self, id: &str) -> Result<DeviceCapabilities> {
        Ok(DeviceCapabilities {
            device_id: id.into(),
            sources: vec![SourceCapabilities {
                source: ScanSource::Flatbed,
                resolutions: vec![150, 300],
            }],
            note: "Synthetic scanner".into(),
        })
    }
    fn acquire(
        &self,
        r: &ScanRequest,
        c: &CancellationToken,
        p: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch> {
        validate_request(r)?;
        c.check()?;
        let path = r.output_dir.join("page.png");
        image::RgbImage::from_pixel(80, 120, image::Rgb([230, 240, 250]))
            .save(&path)
            .unwrap();
        p(Progress::PageReady(path.clone()));
        Ok(ScanBatch {
            pages: vec![path],
            cancelled: false,
            warning: None,
            dpi: Some(300),
        })
    }
}
fn settle(h: &mut Harness<'_, OpenScanUi>) {
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
            start.elapsed() < Duration::from_secs(20),
            "Worker stalled: {:?}",
            h.state().error()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    h.run();
}

#[test]
fn preview_zoom_scroll_and_fit_leave_page_pixels_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("large-page.png");
    image::RgbImage::from_pixel(1200, 1800, image::Rgb([240, 245, 250]))
        .save(&source)
        .unwrap();
    let mut panel = OpenScanUi::with_backend(
        temp.path().join("storage"),
        temp.path().join("models"),
        Box::new(FixtureScanner),
    );
    panel.profile.cleanup.deskew = false;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180.0, 840.0))
        .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
    settle(&mut h);
    assert!(
        h.get_by_label("Fit scanned page")
            .accesskit_node()
            .is_disabled()
    );
    h.state_mut().import_images(vec![source]);
    settle(&mut h);
    let session = h.state().session().unwrap();
    let path = session.resolve(&session.pages[0].image_path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let fit = h.get_by_label("Scanned page preview").rect();
    let controls = h.get_by_label("Scan preview zoom slider").rect();
    assert!(controls.top() >= fit.bottom());
    h.get_by_label("Zoom in scanned page").click();
    h.run();
    assert!(h.get_by_label("Scanned page preview").rect().height() > fit.height());
    h.get_by_label("Zoom out scanned page").click();
    h.run();
    assert!(
        (h.get_by_label("Scanned page preview").rect().height() - fit.height()).abs() < 2.0,
        "before {fit:?}, after {:?}, percent {:?}",
        h.get_by_label("Scanned page preview").rect(),
        h.get_by_label("Scan preview zoom percent").value()
    );
    h.get_by_label("Scan preview zoom slider")
        .set_value_accesskit(400.0);
    h.run();
    let enlarged = h.get_by_label("Scanned page preview").rect();
    assert!((enlarged.width() - 4800.0).abs() < 2.0);
    assert!((enlarged.height() - 7200.0).abs() < 2.0);
    // Large images must not push controls off the screen.
    assert!(h.get_by_label("Fit scanned page").rect().right() <= 1180.0);
    h.get_by_label("Fit scanned page").click();
    h.run();
    assert!(
        (h.get_by_label("Scanned page preview").rect().height() - fit.height()).abs() < 2.0,
        "fit before {fit:?}, after {:?}, percent {:?}",
        h.get_by_label("Scanned page preview").rect(),
        h.get_by_label("Scan preview zoom percent").value()
    );
    h.set_size(egui::vec2(1000.0, 650.0));
    h.run();
    assert!(h.get_by_label("Scanned page preview").rect().height() < fit.height());
    assert!(h.get_by_label("Fit scanned page").rect().right() <= 1000.0);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn filing_format_defaults_to_pdf_and_toolbar_actions_follow_requested_order() {
    let temp = tempfile::tempdir().unwrap();
    let mut panel = OpenScanUi::with_backend(
        temp.path().join("storage"),
        temp.path().join("models"),
        Box::new(FixtureScanner),
    );
    panel.enable_filing_formats();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1900.0, 900.0))
        .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
    settle(&mut h);
    assert_eq!(h.state().filing_format(), Some(ScanFileFormat::Pdf));
    let labels = [
        "Manage presets",
        "Scan",
        "File as",
        "File as format",
        "Import images",
        "Save PDF",
        "Append to PDF copy",
        "Cancel",
    ];
    for pair in labels.windows(2) {
        assert!(h.get_by_label(pair[0]).rect().right() <= h.get_by_label(pair[1]).rect().left());
    }
    h.get_by_label("File as format").click();
    h.run();
    h.get_by_label("Image (JPG)").click();
    h.run();
    assert_eq!(h.state().filing_format(), Some(ScanFileFormat::Jpeg));
}
#[test]
fn established_layout_and_scan_edit_delete_undo_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let mut panel = OpenScanUi::with_backend(
        temp.path().join("storage"),
        temp.path().join("models"),
        Box::new(FixtureScanner),
    );
    panel.profile.cleanup.deskew = false;
    panel.set_accept_label("Use scanned pages");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180.0, 840.0))
        .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
    settle(&mut h);
    let top = h.get_by_label("Save as preset").rect();
    let settings = h.get_by_label("Refresh scanners").rect();
    let tools = h.get_by_label("Rotate 90°").rect();
    let actions = h.get_by_label("Scan").rect();
    assert!(top.top() < settings.top());
    assert!(settings.right() < tools.left());
    assert!(actions.top() < settings.top());
    h.get_by_label("Scan").click();
    settle(&mut h);
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    assert_eq!(h.state().session().unwrap().pages.len(), 1);
    h.get_by_label("Rotate 90°").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages[0].rotation, 90);
    h.get_by_label("Scan").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 2);
    let old = h.state().session().unwrap().pages[0].id.clone();
    h.get_by_label("Move down").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages[1].id, old);
    h.get_by_label("Delete selected page").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 1);
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session().unwrap().pages.len(), 2);
    assert_eq!(h.state().session().unwrap().pages[1].id, old);
    h.get_by_label("Use scanned pages").click();
    h.run();
    let events = h.state_mut().take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e,OpenScanEvent::Accepted(s) if s.pages.len()==2))
    );
    h.get_by_label("Apply corrections").click();
    settle(&mut h);
    assert!(
        h.state()
            .session()
            .unwrap()
            .pages
            .iter()
            .all(|p| !p.excluded)
    );
}
#[test]
fn preset_save_and_reopen_is_available_without_a_host_application() {
    let temp = tempfile::tempdir().unwrap();
    let panel = OpenScanUi::with_backend(
        temp.path().join("storage"),
        temp.path().join("models"),
        Box::new(FixtureScanner),
    );
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180.0, 840.0))
        .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
    settle(&mut h);
    h.get_by_label("Save as preset").click();
    h.run();
    h.get_by_label("Save preset").click();
    settle(&mut h);
    assert!(h.state().error().is_none());
    let presets = scan_module::presets::PresetStore::new(temp.path().join("storage/Presets"))
        .list()
        .unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].device_id.as_deref(), Some("fixture"));
}

#[test]
fn refresh_recovers_stale_selection_preserves_valid_selection_and_handles_disconnect() {
    use std::sync::{Arc, Mutex};
    struct ChangingScanner(Arc<Mutex<Vec<Device>>>, Arc<Mutex<Vec<String>>>);
    impl ScannerBackend for ChangingScanner {
        fn capabilities(&self) -> Capabilities {
            FixtureScanner.capabilities()
        }
        fn devices(&self) -> Result<Vec<Device>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn device_capabilities(&self, id: &str) -> Result<DeviceCapabilities> {
            self.1.lock().unwrap().push(id.into());
            if !self.0.lock().unwrap().iter().any(|device| device.id == id) {
                return Err(ScanError::InvalidRequest(
                    "Stale device reached backend".into(),
                ));
            }
            FixtureScanner.device_capabilities(id)
        }
        fn acquire(
            &self,
            _: &ScanRequest,
            _: &CancellationToken,
            _: &mut dyn FnMut(Progress),
        ) -> Result<ScanBatch> {
            unreachable!("Discovery tests must never start physical scanning")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let devices = Arc::new(Mutex::new(vec![
        Device {
            id: "first:scanner".into(),
            name: "First scanner".into(),
        },
        Device {
            id: "second:scanner".into(),
            name: "Second scanner".into(),
        },
    ]));
    let queries = Arc::new(Mutex::new(Vec::new()));
    let mut panel = OpenScanUi::with_backend(
        temp.path().join("storage"),
        temp.path().join("models"),
        Box::new(ChangingScanner(devices.clone(), queries.clone())),
    );
    panel.profile.device_id = Some("old\0corrupt-device".into());
    let mut h = Harness::builder()
        .with_size(egui::vec2(1180.0, 840.0))
        .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
    settle(&mut h);
    assert_eq!(
        h.state().profile.device_id.as_deref(),
        Some("first:scanner")
    );
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    assert_eq!(*queries.lock().unwrap(), vec!["first:scanner"]);

    h.state_mut().profile.device_id = Some("second:scanner".into());
    h.get_by_label("Refresh scanners").click();
    settle(&mut h);
    assert_eq!(
        h.state().profile.device_id.as_deref(),
        Some("second:scanner")
    );
    assert_eq!(
        queries.lock().unwrap().last().map(String::as_str),
        Some("second:scanner")
    );

    devices.lock().unwrap().clear();
    let previous_queries = queries.lock().unwrap().len();
    h.get_by_label("Refresh scanners").click();
    settle(&mut h);
    assert!(h.state().profile.device_id.is_none());
    assert!(h.state().error().is_none());
    assert_eq!(queries.lock().unwrap().len(), previous_queries);
}
