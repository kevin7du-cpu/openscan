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

[Windows, Linux and macOS checks passed](https://github.com/kevin7du-cpu/openscan/actions/runs/42421535463)
for Rust source commit `6b5947858ea71f4a946fa465b7122ae507779b20`: formatting,
tests, strict Clippy and optimized native builds. Linux also acquired and exported
a page from SANE's installed test driver with the requested page size and resolution.
A Windows optimized build passed OCR export followed by independent PDFCraft text
extraction and rendering.

Physical scanning through the new Rust build and native GUI checks remain pending.
See [validation details](rust/docs/VALIDATION.md). Compilation and synthetic drivers
are not evidence that every real scanner works.

The earlier C# implementation remains in this repository for historical reference.
It is not required to build or run the Rust module.
