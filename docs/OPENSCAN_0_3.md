# OpenScan 0.3.0 integration

## Native egui host

Add `openscan-egui` from this repository to Cargo.toml, pin the selected release
commit with `rev`, and keep Cargo.lock. All egui types must resolve to version
0.37.0 (a host workspace patch of that version is supported).

```rust,ignore
let mut scanner = openscan_egui::OpenScanUi::new(storage, models);
// Each frame, inside a panel or an egui Window:
scanner.show(ui);
for event in scanner.take_events() {
    match event {
        openscan_egui::OpenScanEvent::Accepted(session) => {
            // Done: enqueue host filing/export. The session is already durable.
        }
        openscan_egui::OpenScanEvent::Exported(path) => { /* explicit external export */ }
        openscan_egui::OpenScanEvent::ScanNote(session) => { /* optional host notes */ }
    }
}
```

`standalone()` switches Done to a save dialog and enables the initial 65% native
window size. An embedded scanner never resizes the host window or changes its
global theme/zoom. Call `is_busy()` while closing and keep draining frames until
the current blocking backend operation returns. `cancel()` records the host's
stop request; native driver calls may finish the current batch before returning.
Do not forcibly terminate a worker that owns newly acquired pages.

`settings()` / `settings_mut()` expose the new controls. `profile` remains the
compatibility export profile. `with_backend` accepts an existing Rust scanner
backend. For lower-level implementations use the GUI-independent
`openscan_core::backend::ScannerBackend` and `pipeline::scan_with_backend`.

The visible host completion action is always **Done**. `set_accept_label` remains
source-compatible and enables host delivery; it does not rename that button.
`enable_filing_formats` selects PDF host delivery. Hosts needing the former
PDF/JPEG selector can use `LegacyOpenScanUi`. The supplied interface's Append
and Save Multiple Files modes perform explicit file exports.

## Core-only integration

`openscan-core` has no egui or eframe dependency. Run blocking calls on a worker:

1. Discover sources with `backend::discover()` or supply your own backend.
2. Call `pipeline::scan_and_clean` with `settings::ScanSettings`.
3. Retain a completed batch with `session::append`; pass an existing session to
   append additional pages. Write its settings with `session::write_settings`.
4. Export with `pipeline::save` and an optional `ocr::OcrEngine`, or use
   `session::pdf_bytes` for the standard Tesseract pipeline. Hosts with existing
   Rust OCR models use `session::pdf_bytes_with_models`. The model fallback keeps
   searchable export available on Windows without Tesseract or .NET.

`Page` uses shared immutable original, processed and preview images. Preview
resizing takes place on the processing worker. Sessions preserve source bytes
for image import; acquired images retain their unprocessed pixels as PNG.

## Host integrations

PaperDesk's Scan Page files Accepted sessions into the selected workspace/folder
and preserves originals and corrected images in its catalog. New sessions export
through the 0.3 pipeline. Existing sessions retain the compatibility export path.

PDFCraft's Scan page opens a completed scan as a new unsaved PDF tab and keeps
other tabs intact. Its engine and automation import path use the same new cleanup
and PDF pipeline. Existing v1 sessions without the new settings sidecar still
use the compatibility exporter. The scanner window initially occupies 65% of
the host's available content area, with a 400×300 minimum.

## Limits

Physical Windows and macOS acquisition still needs testing with an attached
supported scanner. SANE on macOS requires a separately installed backend; no
ImageCaptureCore adapter is included. The native transfer-mode control is kept
for workflow compatibility, but buffered-transfer semantics depend on the
selected backend. Multiple simultaneous scanners should use distinct storage
directories. Retention, encryption, multi-user authorization and cloud sync are
host application responsibilities.
