use egui_kittest::{Harness, kittest::Queryable};
use openscan_egui::OpenScanUi;
use scan_module::{
    CancellationToken, Capabilities, Device, Progress, ScanBatch, ScanError, ScanRequest,
    ScannerBackend,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

struct HotplugScanner {
    connected: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl ScannerBackend for HotplugScanner {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "fixture",
            driver_dialog: false,
            programmatic_settings: true,
            cancellation_note: "fixture",
        }
    }

    fn devices(&self) -> Result<Vec<Device>, ScanError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let connected = self.connected.load(Ordering::SeqCst);
        Ok(if connected == 0 {
            Vec::new()
        } else {
            vec![Device {
                id: format!("fixture-{connected}"),
                name: format!("Fixture scanner {connected}"),
            }]
        })
    }

    fn acquire(
        &self,
        _: &ScanRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(Progress),
    ) -> Result<ScanBatch, ScanError> {
        Err(ScanError::Unsupported(
            "No physical acquisition in discovery tests".into(),
        ))
    }
}

fn settle(harness: &mut Harness<'_, OpenScanUi>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.run_steps(1);
        if !harness.state().is_busy() {
            harness.run_steps(2);
            if !harness.state().is_busy() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "scanner discovery did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn opening_scan_rediscovers_devices_and_preserves_held_pages_and_settings() {
    for initially_connected in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let connected = Arc::new(AtomicUsize::new(initially_connected));
        let calls = Arc::new(AtomicUsize::new(0));
        let panel = OpenScanUi::with_backend(
            dir.path().join("storage"),
            dir.path().join("models"),
            Box::new(HotplugScanner {
                connected: connected.clone(),
                calls: calls.clone(),
            }),
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1400.0, 1000.0))
            .build_ui_state(|ui, panel: &mut OpenScanUi| panel.show(ui), panel);
        settle(&mut harness);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "first open must discover automatically"
        );
        if initially_connected == 1 {
            assert_eq!(harness.state().settings().device, "Native|fixture-1");
        }
        harness.get_by_label("Cancel").click();
        harness.run_steps(3);
        let input = dir.path().join("page.png");
        image::RgbImage::from_pixel(80, 120, image::Rgb([255; 3]))
            .save(&input)
            .unwrap();
        harness.state_mut().settings_mut().resolution = 200;
        harness.state_mut().import_images(vec![input]);
        settle(&mut harness);
        let held_directory = harness.state().session().unwrap().directory.clone();
        let before = harness.state().settings().clone();
        connected.store(2, Ordering::SeqCst);
        harness.state_mut().open_scan_dialog();
        harness.state_mut().open_scan_dialog(); // Coalesce repeated opens before the next frame.
        settle(&mut harness);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "reopening Scan must refresh without a Refresh Scanners click"
        );
        assert_eq!(harness.state().settings().device, "Native|fixture-2");
        let mut after = harness.state().settings().clone();
        after.device = before.device.clone();
        assert_eq!(after, before, "discovery must preserve scan options");
        let session = harness.state().session().unwrap();
        assert_eq!(session.directory, held_directory);
        assert_eq!(session.pages.len(), 1);
        harness.get_by_label("Cancel").click();
        harness.run_steps(3);
        harness.state_mut().settings_mut().device = "files|import".into();
        harness.state_mut().open_scan_dialog();
        settle(&mut harness);
        assert_eq!(
            harness.state().settings().device,
            "files|import",
            "retain an explicit image-import choice when hardware was already available"
        );
        harness.run_steps(5);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "idle frames must not keep discovering"
        );
    }
}

#[test]
fn background_poll_detects_a_scanner_without_drawing_the_holding_window() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut panel = OpenScanUi::with_backend(
        dir.path().join("storage"),
        dir.path().join("models"),
        Box::new(HotplugScanner {
            connected: Arc::new(AtomicUsize::new(1)),
            calls: calls.clone(),
        }),
    );
    let ctx = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        panel.poll(&ctx);
        if !panel.is_busy() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(panel.settings().device, "Native|fixture-1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
