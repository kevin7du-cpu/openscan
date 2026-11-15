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

## Local scanner typography update (2026-11-14)

Inter SemiBold from PDFCraft is embedded, unmodified, in the scanner's own font
family. Its SIL OFL 1.1 notice accompanies the crate and binary package. Enabled
control text is near black (#181c22); secondary text is darker (#414852).
The 110% control sizes, 65% standalone holding-window sizing, widget layout and
scan pipeline/defaults are unchanged. Existing font families and host UI styling
are retained. The OpenScan and PaperDesk scanner source/asset copies match.

Windows validation: Rust formatting passed; strict Clippy passed; all 12 regular
openscan-egui tests passed. The optional current-interface wgpu capture passed,
and both its Scan and holding renders were visually inspected. The layout tests
cover small/full-HD displays and long scanner names using the actual local style.
The initial include-ignored test command also invoked an unrelated legacy capture
without PAPERDESK_UI_CAPTURE; that optional capture reported its missing path.
The normal suite subsequently passed, with both optional captures excluded.
PaperDesk's optimized Windows build completed with --locked --offline.
No dependencies or lockfile changed; no new physical scanner test was performed.

The local Windows package is `PaperDesk/dist/PaperDesk-0.3.0-inter-windows-x64`, separate
from all retained release builds. Published OpenScan 0.3.0 assets remain unchanged.
Linux/macOS binaries and PDFCraft have not been rebuilt for this local font update.

## Lighter scanner text (2026-11-14)

Following user review, the scanner now embeds PDFCraft's licensed Inter Medium
(weight 500), replacing SemiBold (600). Dark text contrast, 110% controls, the
65% standalone holding-window setting and all processing defaults are unchanged.
Both OpenScan and PaperDesk use matching font assets and scanner source.

All six current-interface tests passed, including layout and wgpu rendering.
The lighter Scan dialog rendering was visually inspected. PaperDesk's locked,
offline Windows release build passed. Its separate local package is
`PaperDesk-0.3.0-inter-medium-windows-x64`; all 1,423 package files were verified
against SHA-256 hashes. The desktop shortcut now selects this build. Previous
local packages and published releases are retained. No Linux/macOS or PDFCraft
binary was rebuilt for this font adjustment.

## Single scan dialog verification (2026-11-14)

The scanner menu no longer includes Open saved pages, the duplicate Manage
Presets entry or layout choices. Layout 2's UI implementation is removed.
The existing Layout 1 is the single scan dialog, with Manage Presets inside it.
PaperDesk calls the reusable `open_scan_dialog()` entry point on each Scan Page
click; Cancel does not cause an automatic reopen on the next frame. From Scanner
retains Scan and named preset shortcuts. Opening the dialog preserves settings
and held pages, and starts no acquisition. Old layout fields remain deserializable.

Windows validation: all 13 regular scanner UI tests passed (two optional image
captures excluded), including repeated cancel/reopen, the simplified menu,
Manage Presets access, old Compact preference handling, and settings retention.
Six SANE discovery checks and the saved-session/path validation regression passed.
Strict Clippy passed for openscan-egui and PaperDesk; the locked, offline optimized
Windows build succeeded. The separate local package contains 1,423 checksum-
verified files. Earlier local packages and GitHub releases are retained.

The updated PaperDesk was reopened on Windows. Native UI checks confirmed:
Scan Page opens the dialog; Cancel returns to the holding area; From Scanner has
only Scan when no named presets exist; Scan reopens the dialog; a later Scan Page
click reopens it again. Manage Presets is present inside the dialog. PaperDesk
was left open with that dialog visible. No physical acquisition was performed.
The desktop shortcut targets `PaperDesk-0.3.0-scan-dialog-windows-x64`.
Linux/macOS and PDFCraft were not rebuilt for this local interface update.

## Direct Scan button and scanner dropdown spacing (2026-11-14)

The holding-area toolbar now has one Scan button that immediately opens the
existing scan dialog. The From Scanner menu, its chevron and preset shortcuts
are removed; Manage Presets remains inside the dialog. Opening the dialog starts
no acquisition and retains settings and held pages.

The top Scanner selector is bounded to 200 logical pixels and truncates long
names, keeping it clear of Advanced Options. A hover tooltip exposes the full
name. Setting ComboBox::width alone only established a minimum, so the fix also
uses a bounded child layout and truncation. The other control placements, Inter
Medium font, 110% controls and 65% standalone holding-window size are retained.
OpenScan and PaperDesk have identical scanner UI source.

Validation: all 13 regular openscan-egui tests passed, including cancel/reopen
through the direct button and rectangle-based overlap checks with the installed
HP scanner name and an extra-long synthetic name. Six SANE discovery checks and
the saved-session/path validation regression also passed (20 checks total).
Strict Clippy passed for openscan-egui and PaperDesk, and the locked, offline
optimized Windows build succeeded. The separate local package contains 1,423
SHA-256-verified files, including resources and license notices.

Native Windows verification confirmed that Scan Page opens the dialog, the
truncated scanner name and arrow have clear space before Advanced Options,
Cancel returns to the holding area, and its single Scan button reopens the
dialog directly. PaperDesk was left open with the corrected dialog visible.
No physical scan was performed.

The desktop shortcut now targets
`PaperDesk/dist/PaperDesk-0.3.0-scan-button-fit-windows-x64/paperdesk.exe`.
Previous packages and published GitHub releases are retained. PDFCraft and
Linux/macOS binaries were not rebuilt for these local interface changes.

## Aligned, outlined scanner dropdowns (2026-11-14)

The Scanner field now shares the 210-point width and left/right alignment of
Sides, Source, Color Mode, Paper Size and Resolution. Its inset follows the
group-frame margin below it. Long scanner names still truncate and expose their
full name on hover, with clear space before Advanced Options.

All dropdowns in the reusable scanner dialogs use white fields, thin borders
and the standard dropdown arrows. Hover/focus uses the outline rather than a
dark fill. This styling is scoped to dropdowns. The direct Scan button and
automatic Scan Page dialog opening are retained.

Validation: all 13 regular UI tests passed, including actual left/right edge
alignment with every lower field and no overlap for both tested long names.
All 13 SANE unit checks and the saved-session/path traversal regression passed
(27 tests total). Strict Clippy and the locked, offline Windows release build
passed. Both source copies match. All 1,423 packaged files passed SHA-256 checks.

Native Windows inspection confirmed aligned white fields, visible border lines
and arrows, and a gap before Advanced Options. The new PaperDesk build is open
with the scan dialog visible. No physical scan was performed. The desktop
shortcut selects `PaperDesk/dist/PaperDesk-0.3.0-scan-aligned-windows-x64/paperdesk.exe`.
Earlier builds and GitHub releases remain available; no PDFCraft, Linux or macOS
binary was changed for these local refinements.

## White and blue selection controls (2026-11-14)

Shared selection helpers now style every checkbox and radio control in the
scanner crate, including current Scan/preset/options dialogs and the retained
legacy correction/review interfaces. Unchecked boxes and unselected radio
circles are white with a thin outline. Checked boxes are blue (#0067c0) with
white ticks; selected radio circles have blue outlines and dots. Labels retain
their existing text color. The helpers use native egui widgets, preserving
keyboard/accessibility behavior, enabled states and change responses.

Windows validation: 13 regular scanner UI tests, 13 SANE unit checks and the
saved-session/path traversal regression passed (27 total). Strict Clippy,
formatting, diff checks and the locked/offline optimized Windows build passed.
Both source copies match. All 1,423 package files passed SHA-256 verification.
No dependencies, scan defaults or processing behavior changed.

Native visual checks confirmed both selected and unselected controls in the
Scan dialog, including the output radios, plus blue checks and white empty
boxes in Compression and Image Corrections. No setting was toggled and no
physical scan was started. PaperDesk is open with the updated Scan dialog.
The desktop shortcut selects
`PaperDesk/dist/PaperDesk-0.3.0-scan-selection-windows-x64/paperdesk.exe`.
Older builds and published releases are retained. PDFCraft, Linux and macOS
binaries were not rebuilt for this local appearance update.

## Approved regular-weight scanner text (2026-11-15)

The scanner embeds licensed Inter Regular (400), replacing the Medium weight
from the earlier local revisions. Its private font family leaves host fonts and
zoom unchanged. Numbered page-thumbnail labels expose the selection to assistive
technology. The single dialog, white controls, 110% sizing and processing settings
are retained. The legacy panel tests now explicitly exercise LegacyOpenScanUi;
the current interface continues to have its own interaction and layout tests.

Windows validation: formatting, strict Clippy for the workspace, all 33 core
unit tests, 13 current/legacy UI unit tests, five legacy panel integration tests,
standalone argument tests and workspace doctests passed. Two optional image
captures remain opt-in. PDFCraft 0.21.1 was built and its actual lighter dialog
was visually inspected; Windows scanner discovery returned the installed WIA
scanner. PaperDesk's optimized Windows application was built and packaged locally.
No physical page was acquired during this font verification.

The broader compatibility scan-module suite exposes an existing failure in
removes_clustered_specks_but_preserves_punctuation_lines_and_colored_marks:
a small colored mark is erased by the unchanged legacy despeckler. This revision
does not change that processor, and the complete workspace suite is not claimed
to pass. The current openscan-core cleanup tests pass independently.