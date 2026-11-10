# Reusable Rust scan module

**ver0.2.7** includes built-in balanced Sauvola, conservative content-aware OCR Clean, retained originals and session Undo/Redo. See [tested defaults, findings and limits](docs/SCAN_QUALITY.md); consumers do not need to reproduce the tuning manually.

Reusable Rust acquisition, OpenScan-compatible profiles/presets, image correction, and durable sessions. The optional `documents` feature adds local OCR and PDF export. No egui, eframe, database, PaperDesk, C#, or .NET dependency. The caller owns all acquired files.

```toml
[dependencies]
scan-module = { path = "path/to/crates/scan-module" }
```

```rust,no_run
use scan_module::{native_backend, spawn_scan, ScanRequest, ScanSettings, ScanEvent};

let backend = native_backend();
for device in backend.devices()? {
    println!("{}: {}", device.id, device.name);
}
let job = spawn_scan(backend, ScanRequest {
    device_id: None,
    output_dir: std::env::temp_dir().join("my-new-scan-batch"),
    settings: ScanSettings::default(),
});
// Integrate these events with any GUI framework, CLI, or application event loop.
for event in &job.events {
    match event {
        ScanEvent::Progress(progress) => println!("{progress:?}"),
        ScanEvent::Finished(result) => {
            let batch = result?;
            println!("Pages: {:?}; cancelled: {}", batch.pages, batch.cancelled);
            break;
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use a new or empty output directory. Existing files cause validation to fail; acquisition does not overwrite them. `job.cancellation.cancel()` requests cancellation. A host can also call the synchronous trait from its own worker and use `CancellationToken::from_shared_flag` to share its `Arc<AtomicBool>` cancellation flag. Implement this trait to add another driver or a mock without rewriting host applications.

Enable `features = ["documents"]` for OCR/PDF. Versions are explicit, so the crate can be reused outside this workspace. Preserve its license and font files when copying it.

## Sessions, profiles, and export

Use `ScanSettings::from(&ScanProfile::default())` for programmatic acquisition. `ScanSettings::default()` retains the earlier driver-dialog behavior. Inspect `device_capabilities(id)` before selecting a source and DPI.

`ScanSession::create(directory)` creates a durable session. Call `import_images`, `acquire`, `reprocess`, `move_page`, `toggle`, and `export_images` on a worker thread with a `CancellationToken`. Recover with `ScanSession::load(directory)`. `ScanProfile` holds acquisition/cleanup/PDF settings; `PresetStore` manages atomic compatible JSON and archived deletion. `processing::correct` returns pixels, deskew angle, ink coverage, and blank detection. Corrections start from unchanged originals.

In ver0.2.6, `ScanColor::BlackWhite` uses balanced Sauvola (k=0.34, R=128, a 51-pixel window at 300 DPI, scaled with DPI) before optional component dust removal. Existing presets use the same default without rewriting their saved settings. `processing::balanced_black_white(&gray, dpi, &cancellation)` exposes the operation separately for other hosts. Rolling statistics with overlapping stripes bound auxiliary memory to about 66 KiB; cancellation is checked each row. Already-bilevel pixels and exact black fills remain intact during thresholding. The filter changes grayscale pixels intentionally; it does not retype OCR, reconstruct creases, or alter the PDF compression contract. Gray/Color modes keep their existing rendering. Session reopen never silently regenerates saved images.

With `documents`, use `documents::ocr::LocalOcr` and `documents::pdf::{export_session, export, save_new, append_copy}`. `ExportDocument`, `ExportPage`, and `TextLine` allow host-supplied reviewed text and metadata. `save_new` refuses overwrite. OCR models are separate data; exact sources/hashes are in the workspace `models/README.md` and OCR adapter. No models are downloaded implicitly.

Sessions allow 250 pages, 256 MB/image, and 60 million pixels/page. Import reads available PNG/BMP/JPEG JFIF/TIFF DPI, otherwise uses the profile DPI. TIFF imports every supported 8/16-bit gray/RGB/RGBA frame; unsupported formats fail explicitly. PDF append creates a new page-tree copy without document-level forms, signatures, or bookmarks. Algorithms are not pixel-identical to OpenCV. Cancellation is cooperative between processing operations.

## Platform adapters

| Platform | Implementation | Validation |
|---|---|---|
| Windows | Rust WIA 2 COM; device/source/DPI discovery; explicit properties and file-stream callbacks; optional driver dialog; partial-batch recovery | Compiled/tested; HP Smart Tank 7600 capabilities verified; physical acquisition pending |
| Linux | Rust adapter invoking open-source SANE `scanimage`; device source mapping, feeder batches, resolution/color/paper, child cancellation | Native Ubuntu 26.10 tests/export and app installation passed; Fujitsu fi-7030 acquisition user-confirmed on an earlier Rust release; ver0.2.7 hardware retest pending |
| macOS | SANE scanimage adapter; Homebrew/MacPorts executable locations, optional explicit OPENSCAN_SCANIMAGE path | Native build/runtime/hardware pending; Apple ImageCaptureCore adapter remains unimplemented |

Windows supports explicit settings and an optional driver dialog. Native streaming observes cancellation in callbacks; cancel a vendor dialog with its own Cancel button. Partial batches remain recoverable. Unsupported device capabilities are reported rather than invented. SANE must be installed separately on Linux/macOS; source and duplex availability depend on its driver. Use `sane-utils` on Debian/Ubuntu or `brew install sane-backends` on macOS. `OPENSCAN_SCANIMAGE` can select an explicit executable. Image Capture plus image import remains an alternative on macOS.

The only unsafe code is in `src/wia.rs`, around documented COM FFI. COM initialization is balanced on the same thread, BSTR values and arrays use the documented allocators, and interface pointers stay within that thread. The application and widget crates forbid unsafe code.

Sources: [WIA device manager](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-creating-a-wia-device-manager), [WIA acquisition and returned-array ownership](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wia/-wia-iwiadevmgr2-getimagedlg), [SANE scanimage](https://sane-project.gitlab.io/website/man/scanimage.1.html).

Run `cargo test -p scan-module --all-features` for contracts and processing/session/PDF fixtures; `cargo run -p scan-module --example devices` performs read-only hardware discovery and capability queries. Mock tests do not certify hardware. The adjacent `openscan-egui` crate supplies the reusable panel. Full port details are in the workspace `OPENSCAN_PORT.md`.

License: MIT OR Apache-2.0. Original OpenScan design/port notice: `OPENSCAN-LICENSE`; embedded Hack font notices: `assets/Hack-LICENSE.txt`. `windows` bindings are MIT OR Apache-2.0. SANE is an optional separately installed open-source system tool.
