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

Pending: a new physical test sheet through the Rust build, duplex/HP acquisition,
Linux/macOS runner results, native UI inspection and real devices on those platforms.
The earlier C# build's successful fi-7030 scan is not presented as a Rust hardware test.

The source remains a development build until these required checks are completed.
Do not describe compilation, mocked drivers or API discovery as a physical scan.
