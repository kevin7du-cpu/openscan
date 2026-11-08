# OpenScan

The active scanner module is the **Rust implementation in [`rust/`](rust/)**.
It uses egui/eframe and PDFCraft's pinned craft-ui/craft-dialogs stack. Acquisition,
image correction, OCR and PDF export run locally; there is no paid SDK or cloud service.

## Build and reuse

```sh
cd rust
cargo build --locked --release -p openscan
cargo test --locked --workspace
```

Use `openscan-core` for headless scanning and processing, `openscan-ui` for an egui
scan/preset panel, or the versioned JSON process protocol from any host application.
See [the Rust README](rust/README.md), [API](rust/docs/API.md) and
[PDFCraft GUI references](rust/docs/GUI.md).

Windows uses its installed scanner drivers through WinRT. Linux and macOS use
installed SANE drivers or eSCL/AirScan network scanners. Driver support depends on
the scanner model; macOS Image Capture and TWAIN are not implemented.

## Verification

[Windows, Linux and macOS scanner checks passed](https://github.com/kevin7du-cpu/openscan/actions/runs/42430141576) for Rust source
commit `8677344d5780305f9cb17fe91eb5d5f3c0000b2a`: formatting, tests, strict Clippy, optimized native builds
and release startup. Linux uses Ubuntu 24.04 and an installed SANE test driver.

[Native PDFCraft interoperability passed](https://github.com/kevin7du-cpu/openscan/actions/runs/42431338625) on all three platforms:
seven preset, validation and real-model OCR/PDF round-trip checks per platform.
PDFCraft independently extracted the expected text and rendered the generated PDF.

Physical scanning through the new Rust build and native GUI checks remain pending.
See [validation details](rust/docs/VALIDATION.md) for exact runners and coverage.
Compilation and synthetic drivers are not evidence that every real scanner works.

The earlier C# implementation remains in this repository for historical reference.
It is not required to build or run the Rust module.
