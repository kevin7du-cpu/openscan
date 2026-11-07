# Validation status — 2026-11-07

Windows local checks completed:

* Rust workspace builds and scoped Clippy checks pass.
* Core tests cover deskew, black-and-white output, presets, bounds, cancellation
  recovery, original preservation, multipage TIFF, PDF append and image color channels.
* A local synthetic HTTP scanner exercises feeder transfer and scan-job deletion.
* egui tests exercise Scan/Cancel and both panels at 480/1040-point widths, including
  accessible control names.
* A rasterized synthetic text page was processed with the real OCR models. PDFCraft
  independently reopened and rendered the result and extracted the expected words
  and invoice number.
* Installed Windows drivers enumerate the HP Smart Tank 7600 and fi-7030. The fi-7030
  reports feeder/duplex and 75–600 DPI. Its BMP transfer and completion-callback
  requirements were corrected; an empty-feeder acquisition completes with a clear error.

Cross-platform CI completed at Rust source commit
`6b5947858ea71f4a946fa465b7122ae507779b20`:

| Runner | Tests passed | Format / Clippy / release build |
| --- | ---: | --- |
| Windows x64 | 22 | Passed |
| Linux x64 | 22 | Passed |
| macOS ARM64 | 21 | Passed |

[Run and downloadable binaries](https://github.com/kevin7du-cpu/openscan/actions/runs/42421535463).
Linux's total includes an actual `scanimage` invocation against the installed SANE
`test:0` driver, then processing and PDF export. The acquired A5 dimensions at 75 DPI
are checked within one pixel, and an unsupported 100 DPI request is rejected before
acquisition. The backend queries source/mode-dependent options before scanning;
tests cover resolution steps and paper bounds. Windows has an additional BMP-format
selection test. The Windows CI
release binary also passed the local real-model OCR test using models alongside the
packaged executable, with independent PDFCraft extraction and rendering.

Pending: a new physical test sheet through the Rust build, duplex/HP acquisition,
native UI inspection and real devices on Linux/macOS.
The earlier C# build's successful fi-7030 scan is not presented as a Rust hardware test.

The source remains a development build until these required checks are completed.
Do not describe compilation, mocked drivers or API discovery as a physical scan.
