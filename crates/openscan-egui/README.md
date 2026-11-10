# Reusable OpenScan egui panel

An embeddable Rust scanning interface preserving OpenScan's control arrangement. Uses egui 0.37 and the adjacent `scan-module`; no PaperDesk, database, C#, or .NET dependency. Acquisition, import, correction, OCR, PDF, presets, and preview decoding run on a background worker.

```toml
[dependencies]
egui = "=0.37.0"
openscan-egui = { path = "path/to/crates/openscan-egui" }
```

Create one `OpenScanUi::new(storage_directory, models_directory)` for the host lifetime. Model files are needed for searchable export and OCR clean copies. Call `scanner.show(ui)` each frame. Optionally use `set_accept_label("Use these pages")` and consume `take_events()`: `Accepted(ScanSession)` hands included pages to the host; `Exported(PathBuf)` reports completed output. Sessions remain caller-owned and are not deleted by these events.

For a complete compiling host, see [examples/standalone.rs](examples/standalone.rs):

```powershell
cargo run --locked -p openscan-egui --features standalone --example standalone
```

The example uses eframe and **wgpu**. These are optional host dependencies; the panel itself needs only egui. To reuse outside the workspace, copy both adjacent crates with their license/font notices and declare dependencies in the host lockfile.

`with_backend` accepts any `Box<dyn ScannerBackend>`, including a mock. `import_images` and `open_session` let a host provide input; submit only when `!is_busy()`. Keep calling `show` to drain progress/events. Before closing a busy host, call `cancel()` and wait for `!is_busy()`. Completed pages remain recoverable.

Menus, presets and wrapping Scan/File as/import/export/Cancel actions sit above the workspace. Bottom-right zoom offers 1–400%, Fit, scrolling and drag-to-pan without editing scan pixels. Worker previews retain up to 4096 pixels per side within the renderer limit. Preset management retains its established controls.

Hosts implementing PDF/JPEG filing call `enable_filing_formats()` and read `filing_format()` when consuming Accepted; PDF is the default. Existing `set_accept_label` hosts keep their original acceptance API. The host owns its catalog and creates the chosen file format.

Hosts with a note editor call `enable_scan_notes()` and handle `OpenScanEvent::ScanNote(ScanSession)`. The Scan Note button appears immediately after OCR Clean; the event contains the current session snapshot. PaperDesk performs local OCR and optional note formatting on its worker, protects dirty notes, and files the original scan only when the note is saved. `scan_module::documents::ocr::session_words` shares content-addressed OCR with cleanup and searchable export, invalidating it when image bytes or model signatures change.

OCR Clean opens a comparison/review window for one selected page. In ver0.2.7 it uses conservative content-aware cleanup: pixels outside recognized regions stay unchanged. Curved labeled Undo/Redo switch between the original and edited review; the corner X closes without applying. See [built-in defaults and scan-quality findings](../scan-module/docs/SCAN_QUALITY.md). By default it preserves scanned text and graphics, removes small isolated specks and reduces light neutral paper shadows. OCR requires the bundled models; its guesses never automatically replace the image. Optional Replace region checkboxes permit reviewed retyping at normal glyph proportions. Erase marks allows source-coordinate rectangular selection at any zoom, with Undo last mark. After changing any setting, text or mark, Preview changes is required before Apply. Apply changes only the working page and closes the window; normal Undo/Redo restore exact saved revisions. Dark creases can remain, particularly where they cross text. Direction is not a reliable content classifier. Use normal File as or Save PDF for export. Originals stay unchanged. PDF compression remains lossless relative to the chosen working image: generic-region JBIG2 (independently decoded and pixel-verified), Group 4 or Flate for true black/white; level-9 Flate for gray/color. Image cleanup itself intentionally changes pixels and is not described as lossless restoration.

Windows WIA and Linux/macOS SANE are exposed through the core; SANE requires a separate installation and a supported device. macOS Image Capture plus image import remains available. Native Ubuntu 26.10 tests, export and PaperDesk installation are verified; Fujitsu fi-7030 acquisition was user-confirmed on an earlier Rust release. ver0.2.7 physical acquisition and native macOS validation remain pending. See the core README and workspace `OPENSCAN_PORT.md` for format/processing limits, PDF append restrictions, and hardware validation status.

Project code: MIT OR Apache-2.0. Original OpenScan design: MIT, retained in `OPENSCAN-LICENSE`. OCR model/font notices are described in the core README and workspace `THIRD_PARTY_NOTICES.md`.

Apply corrections is in the bottom bar. Left/right arrows Undo/Redo up to 32 edits in the currently open session, including corrections, clean-copy Apply, rotation, reordering, deletion, and added pages. Opening/creating a session or restarting the app resets this in-memory history; the saved current page state and retained originals/revisions remain on disk. Undo/Redo atomically save the restored session manifest and do not rerun OCR, processing, or acquisition. New edits clear Redo.

Del and right-click Delete page remove the chosen page from the current session and exports. Image files remain recoverable; Undo restores the page and its order. The standalone Rust `scan_module::history::ScanHistory` API lets other hosts record committed `ScanSession` changes and undo/redo independently of egui.
