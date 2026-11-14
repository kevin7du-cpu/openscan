# OpenScan 0.3.0 validation

Validated on Windows and Ubuntu with synthetic documents, 2026-11-14.
No patient records or real filing data were used in these tests.

## Automated checks

- Stable Rust 1.99.1; pinned egui/eframe 0.37.0 and wgpu 30.0.1.
- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo test --workspace --all-features --lib`: 61 passed, 2 optional graphics
  captures ignored in that run. The new interface capture was run separately
  with wgpu and inspected at the accepted 110% control scale.
- Core coverage includes cleanup, source discovery parsing, presets, PDF output,
  append/split output, OCR, exact imported originals, two retained batches,
  reopening sessions and failed append preserving the previous manifest.
- UI coverage includes 110% controls, initial 65% holding-window size, both Scan
  layouts, long device names, failed Scan more Pages preserving held pages, and
  Done delivering the combined durable session to a host.
- The Rust OCR adapter was exercised on Windows with the separately provisioned,
  hash-verified ocrs/RTen models. Searchable text was present without embedding a
  duplicate page image. Tesseract was not installed in that Windows environment.

## Scan-quality comparison on Ubuntu

The accepted Scan Module 0.1.3 and OpenScan 0.3.0 converted the same synthetic
skewed invoice with their B&W/300 DPI, crop, deskew, despeckle and Low shadow
correction defaults. The extracted page images were byte-for-byte identical:

`c35c004ca2c531cb557d8b7b4ef4492679c3e4ee12b49e0da112f9256f08f4db`

The Ubuntu Tesseract output contained searchable invoice/customer/total text.
`pdfinfo`, `pdfimages` and `pdftotext` independently checked the resulting PDF.
Windows and Linux standalone release builds completed successfully.

## Host integration checks

PaperDesk passed both scanner filing workflow tests, including the new pipeline's
bitonal PDF, unchanged original bytes and retained corrected images.

PDFCraft passed ten scanner-specific engine, UI and automation tests. Its Done
flow opens a new unsaved PDF and retains the previously open document. Imports
respect the existing session-folder and automation-root protections. Its
WebAssembly check also passed; native scanner dependencies remain native-only.

Broader host tests and application packaging are recorded by the host projects;
this file does not claim that every existing host feature was retested.

## Practical limits

Synthetic imports exercise the common processing and export pipeline. They do
not replace a physical-device test of WIA, SANE feeder/duplex behavior, vendor
dialogs or cancellation. macOS runtime and attached-device tests were not run.
Native drivers can block until a batch completes; hosts must keep draining the
module while it is busy instead of terminating its worker.

Version-specific documentation consulted:
[egui 0.37.0](https://docs.rs/egui/0.37.0/egui/),
[eframe 0.37.0](https://docs.rs/eframe/0.37.0/eframe/), and
[wgpu 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/).
