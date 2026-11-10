# OpenScan â€” reusable Rust scanner module

OpenScan is a reusable Rust module for Windows, Linux, and macOS, with an optional
native **egui / eframe / wgpu** scanning interface. The framework is named eframe (sometimes called eframework). Applications can embed the core
or the complete panel without depending on PaperDesk or rewriting scanner logic.
The C# implementation and its .NET/NuGet build have been replaced.

The established scan window arrangement is retained: presets and menus above,
settings on the left, page tools and preview in the middle, and scan/import/export
actions below. OCR and image processing run on background workers.

## Built-in tested defaults

Straighten, Crop and Remove dust default ON. Black & White mode uses balanced Sauvola. OCR Clean defaults to conservative content-aware cleanup and preserves unrecognized pixels. Saved choices remain respected. The [scan-quality guide](crates/scan-module/docs/SCAN_QUALITY.md) includes implemented parameters, preservation rules, experimental findings, regression coverage, compression and native-path lessons. This behavior is in the module, not just documentation.

See [changes](CHANGELOG.md), [integration API](docs/API.md), [migration](docs/MIGRATION.md), [validation](docs/VALIDATION.md), and [contributing](CONTRIBUTING.md).

## Platform support

| Platform | Acquisition | Validation |
|---|---|---|
| Windows | Rust WIA 2, installed scanner driver; optional vendor dialog | Windows build and automated tests pass; HP Smart Tank 7600 discovery/capabilities verified; Rust physical acquisition pending |
| Linux | Rust adapter invoking the separately installed open-source SANE `scanimage` | Native Ubuntu 26.10 tests and export pass; Fujitsu fi-7030 user-confirmed on an earlier Rust release; ver0.2.7 hardware retest pending |
| macOS | Same SANE adapter; Homebrew/MacPorts locations supported | Previous cross-build checks only; native macOS validation pending; no Apple ImageCaptureCore adapter |

Image import, correction, session recovery, OCR/PDF, presets, and the egui panel
use shared Rust code on all three systems. A manually triggered three-platform CI workflow is included; a CI
configuration is not evidence of a successful native run.

Install `sane-utils` on Debian/Ubuntu or
[`sane-backends` on macOS](https://formulae.brew.sh/formula/sane-backends).
Scanner support depends on the installed SANE backend and device.
`OPENSCAN_SCANIMAGE` can select an explicit executable. Scans made by another
application, including macOS Image Capture, can also be imported.

## Build and run

Use stable Rust **1.99.1** with Cargo. Dependencies are pinned in the manifests
and the checked-in `Cargo.lock`. Windows requires a Rust-compatible native linker;
macOS requires the Command Line Tools. Ubuntu desktop builds require
`libxkbcommon-dev libwayland-dev libxcb-shape0-dev libxcb-xfixes0-dev`.

```sh
cargo build --locked -p openscan-egui --features standalone --example standalone
cargo run --locked -p openscan-egui --features standalone --example standalone -- --models ./models
```

The built `target/debug/examples/standalone` executable (`.exe` on Windows) can run
without a source checkout. Use `--storage DIRECTORY` to choose the session/preset
location, and `--models DIRECTORY` for OCR data. Defaults:

- Windows: `%LOCALAPPDATA%/OpenScan/Rust`
- macOS: `~/Library/Application Support/OpenScan`
- Linux: `$XDG_DATA_HOME/openscan`, or `~/.local/share/openscan`

Models default to `STORAGE/models`; `OPENSCAN_MODELS` or `--models` overrides it.
The Windows Rust directory is separate from the old C# store. Existing files are
not migrated or deleted automatically.

Searchable export needs two separately installed, hash-verified OCR models.
Node.js 24 is only a setup helper, not an application runtime dependency:

```sh
node scripts/setup-ocr-models.mjs
```

The app never downloads models or uploads documents. Image-only PDF export and
the rest of the workflow do not require OCR models.

## Embed the module

Core only, for any Rust host:

```toml
[dependencies]
scan-module = { path = "path/to/openscan/crates/scan-module", features = ["documents"] }
```

Omit `documents` if OCR/PDF are not needed. The core has no GUI/database dependency.
Implement `ScannerBackend` to add a driver or inject a mock.

Complete panel in an egui host:

```toml
[dependencies]
egui = "=0.37.0"
openscan-egui = { path = "path/to/openscan/crates/openscan-egui" }
```

Create one `OpenScanUi::new(storage, models)`, call `show(ui)` every frame, and
consume `take_events()`. The host controls file locations and receives exported
paths or accepted sessions. The optional `standalone` feature supplies an
eframe/wgpu example; eframe is not required by an embedding host.

See [Rust API](docs/API.md), [core](crates/scan-module/README.md),
[panel](crates/openscan-egui/README.md), and [migration](docs/MIGRATION.md).

## Features and verification

Discovery and source/DPI settings; flatbed/feeder/duplex where supported; presets;
image/TIFF import; preview, rotation, reorder and page inclusion; deskew, crop,
background cleanup, denoise, sharpen, brightness/contrast, gray/BW conversion;
reversible blank exclusion; saved correction preferences; zoom; Undo/Redo; page deletion; conservative OCR Clean; recoverable sessions; PNG/JPEG and searchable/image-only
PDF export; append as a new PDF copy. See [feature catalog](docs/FEATURE_CATALOG.md).

```sh
cargo fmt --all -- --check
cargo check --locked -p scan-module --no-default-features
cargo clippy --locked --workspace --all-features --all-targets -- -D warnings
cargo test --locked --workspace --all-features --all-targets
cargo test --locked --workspace --all-features --doc
cargo run --locked -p scan-module --features documents --example verify_workflow -- test-output/verification models
```

The final command creates synthetic pages and requires installed models and a
new output directory. It tests real OCR, session recovery, blank exclusion, PDF
text/page count, overwrite protection and PNG export without scanner hardware.
`cargo run --locked -p scan-module --example devices` performs read-only device
discovery. Validation status and remaining hardware checks: [docs/VALIDATION.md](docs/VALIDATION.md).

## Boundaries and licensing

Sessions support 250 pages, 256 MB/image and 60 million pixels/page. Import accepts
PNG/JPEG/BMP/WebP and supported 8/16-bit gray/RGB/RGBA TIFF frames. Review faint
originals after crop/blank detection and review OCR before relying on its text.
Cancellation is cooperative; an active OCR inference or vendor dialog may need
to finish. PDF append copies page trees; forms, signatures and document-level
bookmarks are not preserved. No PDF/A, encryption, book dewarping or TWAIN adapter.

This is a Rust library API, not a drop-in binary replacement for the former
C# JSON process protocol. The old Python example remains as historical reference.

Original OpenScan: MIT, preserved in [LICENSE](LICENSE). Rust implementation:
MIT OR Apache-2.0, with the original OpenScan notices retained in both crates.
All selected dependencies are open source. The platform's scanner service/driver
and optional SANE installation are system prerequisites.
See [third-party notices](THIRD-PARTY-NOTICES.md),
[resolved licenses](DEPENDENCY_LICENSES.md), and [OCR model attribution](models/README.md).
