> OpenScan 0.3.0 uses the new accepted interface and `openscan-core` pipeline.
> Start with [the 0.3 integration guide](OPENSCAN_0_3.md). The reference below
> describes compatibility APIs; its former panel is now `LegacyOpenScanUi`.

# Rust API

The reusable libraries replace the old C# process interface. They do not require
a subprocess, local server, PaperDesk, C# or .NET.

## ver0.2.7 additions

- `processing::balanced_black_white` exposes DPI-aware balanced Sauvola independently of acquisition.
- `documents::clean_copy::prepare` retains every region as scanned ink by default. `render_with_options_at_dpi` takes current working pixels, reviewed word bounds, options, actual DPI and a cancellation token; unknown content is retained. Existing wrappers use 300 DPI.
- `history::ScanHistory` records committed session changes for exact Undo/Redo. Originals are retained; history is bounded and lasts for the open session.
- `documents::ocr::session_words` shares image/model-addressed OCR with cleanup, searchable export and hosts requesting notes.
- `OpenScanUi::enable_filing_formats` exposes PDF/JPEG filing; the host reads `filing_format` on acceptance. `enable_scan_notes` exposes the Scan Note event; the host supplies the editor.
- The panel owns worker progress/cancellation, saved correction preferences, zoom, current-page review and compact labeled Undo/Redo. New defaults do not silently alter saved scans.

See [quality/defaults](../crates/scan-module/docs/SCAN_QUALITY.md) and the crate READMEs for contracts and examples. The shared local `egui-office-widgets` dependency is included in this repository and has no PaperDesk dependency.

## Core

`scan-module` exposes:

| API | Responsibility |
|---|---|
| `ScannerBackend` | Driver discovery, capabilities and blocking acquisition |
| `native_backend()` | WIA on Windows; SANE process adapter on Linux/macOS |
| `spawn_scan(backend, request)` | Background acquisition with progress, result and cancellation |
| `ScanProfile`, `ScanSettings` | Validated acquisition/correction/export settings |
| `PresetStore` | Atomic JSON save/load, explicit replace, rename and archived deletion |
| `ScanSession` | Import/acquire, immutable originals, corrections, order/inclusion, recovery and image export |
| `processing::correct` | Shared Rust image corrections |
| `documents::ocr::LocalOcr` | Verified local model loading and OCR |
| `documents::pdf` | Image/searchable PDF, no-overwrite save, append as a new copy |

Enable the `documents` feature for the last two APIs. The
[core README](../crates/scan-module/README.md) includes a compiling acquisition example.
The [workflow example](../crates/scan-module/examples/verify_workflow.rs) demonstrates
import, correction, durable recovery, real OCR, PDF and PNG export independently of
any GUI or catalog.

Run synchronous operations on a worker thread. Reuse the same
`CancellationToken` across processing stages and observe progress.
`ScanSettings::from(&profile)` enables programmatic settings;
`ScanSettings::default()` retains driver-dialog compatibility on Windows.
Use a new/empty acquisition output directory. Check the returned
`ScanBatch.cancelled` and `warning` as well as its completed pages.

`ScanProfile` uses the original camelCase version-1 JSON schema. Unknown fields,
unknown enum values and invalid settings fail explicitly. Host applications own
their allowed filesystem locations; these APIs are local trusted-host APIs.

`ExportDocument`, `ExportPage` and `TextLine` accept host-reviewed text and metadata.
Coordinates are image pixels; the PDF adapter maps them using page DPI.
Standalone export performs OCR directly, so a host requiring mandatory review
must implement that gate before export.

## egui panel

`OpenScanUi::new(storage_directory, models_directory)` creates the panel;
`with_backend` accepts a custom or mock `ScannerBackend`.
Keep one panel for the host lifetime and call `show(ui)` every frame.

- `set_accept_label("Use these pages")` enables a host handoff action.
- `take_events()` drains `Accepted(ScanSession)` and `Exported(PathBuf)`.
- `import_images(paths)` and `open_session(directory)` accept host-supplied input.
- Submit work only when `!is_busy()`; keep calling `show` to drain worker events.
- Before closing a busy host, request `cancel()` and wait for `!is_busy()`.

Sessions and output files stay caller-owned; accepted/exported events do not
delete them. Copy the adjacent crates together (including licenses and font
assets) or point Cargo to the repository. No host-global font resolver is needed.

See the [complete eframe/wgpu host](../crates/openscan-egui/examples/standalone.rs).
The standalone executable accepts `--storage` and `--models`; it does not implement
the historical `OpenScan.exe COMMAND` JSON protocol.

## Migration

Original profile JSON is supported. Legacy session manifests can be read when
their referenced files remain inside the same session directory; new manifests
use relative paths. Back up existing sessions before migrating.
See [MIGRATION.md](MIGRATION.md) for retained historical examples and API boundaries.
