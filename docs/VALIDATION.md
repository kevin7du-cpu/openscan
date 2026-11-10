# OpenScan ver0.2.7 validation

The independent workspace is tested without PaperDesk as a dependency. Rust 1.99.1 was used on Windows. Its external dependency versions/checksums are a subset of the tested host lockfile; no new external versions were introduced.

| Check | Observed result |
|---|---|
| Core without documents | Release-profile compilation passed |
| Independent Windows suite | 90 tests passed; optional GPU capture excluded from normal suite |
| Doctests | Two passed |
| Strict Clippy / formatting | All workspace targets/features passed |
| Standalone eframe/wgpu example | Release build and --help passed |
| Compact arrow UI | Separately rendered with wgpu and visually checked; labels align with neighboring buttons |
| Shared scanner code on Ubuntu | The host's 131-test native suite passed, including core, processing, history and panel tests; the independent standalone binary has not been run on Ubuntu |
| Real cleanup/export on Ubuntu | Three grayscale inputs and one corrected Black & White input; originals and all pixels outside OCR regions unchanged; independent Poppler PDF extraction pixel-identical |
| Acquisition | Fujitsu fi-7030 user-confirmed in Ubuntu on an earlier Rust release; ver0.2.7 physical acquisition still pending |
| macOS | Earlier cross-build/package checks only; native launch and acquisition remain pending |

The host Windows suite passed 132 tests before the final compact-arrow visual refinement. Linux host package installation/launch passed with existing saved files preserved. These host checks do not imply every independent distribution/platform has been installed. The GitHub workflow is manual and has not been run for this publication.

Tests use synthetic text, punctuation, thin rules, diagonal ink, barcodes, color and halftone fixtures. The real scans are private and are not published. OCR Clean changed 8,283 / 206 / 65,220 / 0 pixels on the four native samples; the conservative no-op is intentional. This does not prove complete wrinkle removal or semantic layout classification.

Run the reproducible commands in README.md and inspect crates/scan-module/tests, crates/openscan-egui/tests and the examples. See crates/scan-module/docs/SCAN_QUALITY.md for defaults, tuning evidence and known limitations.
