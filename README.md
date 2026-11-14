# OpenScan 0.3.0 — Rust, egui, eframe and wgpu

OpenScan is a reusable native scanner for Windows, Linux and macOS. This release
adopts the Scan Module 0.1.3 interface and image-processing pipeline, including
the accepted scan and temporary holding workspace. No C# or .NET runtime is
required. The core does not depend on a GUI or on PaperDesk/PDFCraft.

## Accepted defaults and controls

- Black & White, 300 DPI.
- Deskew, despeckle and crop black borders: on.
- Remove wrinkle and crease shadows: Low.
- Controls: 110%. Standalone holding window: 65% of the monitor's width and
  height, never initially maximized. Embedded controls leave host zoom unchanged.
- **Scan more Pages** immediately precedes **Done**. Additional batches append
  to the held document. A failed batch retains the previously completed pages.
- Both scan dialog layouts, presets, scanner/source settings, image corrections,
  compression, OCR options, thumbnails, preview and activity log are retained.

The new processing order is crop, shadow correction, deskew, descreen (optional),
despeckle, sharpening (optional), blank-page detection (optional), and global
Otsu black/white conversion. SANE requests the device's line-art mode for B&W.
The older Sauvola pipeline remains available through the compatibility API; it
is not applied again to pages produced by the new interface.

## Reusable packages

| Package | Purpose |
|---|---|
| `openscan-core` | Acquisition, exact accepted cleanup, OCR interface, PDF export, durable session bridge. No egui dependency. |
| `openscan-egui` | `OpenScanUi::new(storage, models)` and `show(ui)`, background discovery/processing, host completion events. |
| `scan-module` | Existing WIA/SANE adapters and v1 session/history/document APIs for compatibility. |
| `egui-office-widgets` | Existing reusable office widgets. |

The former panel is explicitly available as `LegacyOpenScanUi`. New applications
use `OpenScanUi`. See [0.3 integration](docs/OPENSCAN_0_3.md),
[API reference](docs/API.md), [migration](docs/MIGRATION.md), and
[release validation](docs/VALIDATION_0_3.md).

## Build and run

Use a compatible stable Rust toolchain (MSRV 1.95), and commit `Cargo.lock` in
applications. The GUI uses pinned egui/eframe 0.37.0 with wgpu rendering.

```sh
cargo build --release --locked -p openscan-egui --features standalone --bin openscan
cargo run --locked -p openscan-egui --features standalone --bin openscan -- --storage ./scans
# Headless conversion, with OCR when Tesseract is installed:
openscan --convert output.pdf page1.png page2.png
# Minimal example of embedding the reusable panel:
cargo run --locked -p openscan-egui --features standalone --example standalone
```

The binary runs without a source checkout. `--storage DIRECTORY` selects the
session/preset location. Default storage is `%LOCALAPPDATA%/OpenScan/Rust` on
Windows, `~/Library/Application Support/OpenScan` on macOS, and
`$XDG_DATA_HOME/openscan` (or `~/.local/share/openscan`) on Linux.

## Platform adapters and OCR

Windows uses direct Rust WIA 2 and the installed scanner driver. Linux uses the
open-source SANE `scanimage` program (`sane-utils` on Ubuntu). macOS uses SANE,
including Homebrew and MacPorts locations; alternatively import images captured
by Image Capture. `OPENSCAN_SCANIMAGE` selects an explicit SANE executable.
Native scanner dialogs depend on backend support; SANE does not implement a
vendor settings dialog. WIA's gray acquisition is converted to B&W by the common
cleanup pipeline. Device support is determined by the installed driver.

The new interface prefers separately installed open-source **Tesseract** for
searchable PDF export and falls back to the host's hash-verified **ocrs/RTen**
models when Tesseract is absent. Image-only PDF requires neither. Hosts may
supply an `OcrEngine` implementation to the core. The optional `rust-ocr` core
feature enables the model adapter and is enabled by the GUI package. The optional `naps2` feature is an
explicit external-adapter opt-in and is disabled by default. It is unnecessary
for the normal Windows, Linux or macOS build. Existing `scan-module` OCR-model
APIs and old sessions remain available for compatibility.

## Data preservation

Completed batches are committed to caller-owned session folders before delivery
to the host. Exact imported source bytes and lossless corrected PNGs are kept
separately. Reopening a v1 session never runs cleanup again. `openscan-v3.json`
records the new output settings alongside the compatible `session.json`.
**From Scanner → Open saved pages** opens an unfinished session. Explicit saved
presets win over defaults; the old standalone presets are read only as a fallback
and subsequent changes go to the host's own `presets-v3.json`.

New PDFs and split output never overwrite existing files. Append replaces the
destination only after the merged PDF is complete. OCR errors retain the held
pages. Applications own filing permissions, retention and access controls.

## Licensing

Rust code and the supplied Scan Module source are MIT OR Apache-2.0. Original
OpenScan notices remain in [LICENSE](LICENSE), [LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE), and [third-party notices](THIRD_PARTY_NOTICES.md).
Only synthetic images are included in tests. No patient documents or local
scanner/preset data are included in the repository.
