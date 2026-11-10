# Tested scan defaults and findings — ver0.2.7

These behaviors are implemented in Rust. Downstream applications do not need to reproduce the tuning: use `ScanProfile::default()`, `CleanupOptions::default()` and `CleanOptions::default()`. The optional egui panel uses these defaults and respects saved preferences. A host can override a profile without forking the algorithms.

## Built-in defaults

| Setting | Default / behavior | Boundary |
|---|---|---|
| Resolution | 300 DPI; validated range 75–600 | Retain source image DPI when available; avoid repeated resizing. |
| Color | Color | Select Black & White for predominantly black text; never silently discard colored annotations or photos. |
| Straighten / Crop / Remove dust | ON | Review edge annotations and difficult borders; acquisition originals remain available. |
| Remove background / sharpen | OFF | Broad tonal/edge changes require an explicit choice. |
| Skip blank pages | OFF | Lightly marked pages must not disappear silently. Exclusion is reversible when enabled. |
| Brightness / contrast / rotation | 0 / 0 / 0 | No added shift by default. |
| Searchable PDF | OFF in the acquisition profile | OCR is a separately requested operation; hosts can enable searchable export. |
| File as | PDF in the optional panel | JPEG is an explicit, lossy alternative, including quality 100. |
| OCR Clean: remove specks / reduce shadows | Both ON | Used when OCR Clean is requested; saved scans are not automatically regenerated. |
| OCR Clean: replace recognized text | OFF for every region | Never automatically replace scanned ink with OCR guesses. |
| Erase marks | Manual opt-in | Erase only selected rectangles, with preview and Undo. |

Saved choices take precedence. The panel atomically stores correction preferences in `image-correction.json` under host-selected storage. Saving preferences does not change a named preset or an existing page. `ScanSettings::default()` retains the earlier driver-dialog behavior; use `ScanSettings::from(&ScanProfile::default())` for these programmatic profile defaults.

## Balanced Sauvola and dust removal

`processing::balanced_black_white` uses **k=0.34**, **R=128**, and a **51-pixel window at 300 DPI**, scaled with DPI. Local mean and standard deviation determine the threshold. Clipped edge windows, overlapping stripes and bounded rolling statistics prevent stripe seams and avoid large integral-image buffers; auxiliary statistics occupy at most approximately 66 KiB. Cancellation is checked each row. Exact black and already-bilevel input remain intact during thresholding.

This balanced variant was selected after comparing local and global thresholds and more aggressive settings on real scan copies. It reduced mottled paper while retaining readable thin text. It is a starting point, not a guarantee for every device. Thresholding intentionally changes grayscale pixels. Gray/Color modes retain their separate behavior. Component dust removal runs after Black & White conversion, protects nearby larger ink and dense halftones, and avoids whole-page median blur.

Do not erase everything outside OCR boxes: recognition can miss handwriting, signatures, stamps, small print, barcodes, pictures or entire lines. Do not classify noise by angle alone. Automatically retyping a page or stretching glyphs into OCR boxes can substitute incorrect content and damage appearance.

## Content-aware OCR Clean

Call `documents::clean_copy::render_with_options_at_dpi` with word bounds for the **current working image** and its actual DPI. Never mix original acquisition geometry with bounds from a cropped/rotated preview. The compatibility wrappers `render` and `render_with_options` use 300 DPI.

Cleanup starts with a copy of the working image. Eligible OCR word regions provide context for local paper estimation and connected-ink analysis, including a surrounding guard area. Substantive ink, crossing rules, nearby accents/decimal points/dots and colored content are protected. Only isolated tiny neutral components far from protected ink are candidates for removal: maximum 3-pixel side and 8-pixel area at 300 DPI, scaled with DPI. Density checks protect halftones/stippling. Light neutral gaps can be normalized to surrounding paper tone with an ink halo, avoiding conspicuous white rectangles on gray paper.

**Pixels outside recognized regions are exactly unchanged.** No recognized text means no automatic OCR Clean changes. Implausibly wide text, insufficient ink anchors, dark/dense or colored regions and analysis-limit overflows are retained. Limits are 1,048,576 pixels per contextual patch, 64 million cumulative patch pixels and 20,000 tiny candidates per patch, with cooperative cancellation. Reaching a bound retains content instead of invoking an aggressive fallback.

This is geometric ink protection guided by OCR, not a complete text/table/photo classifier, fold reconstruction or ABBYY-equivalent mixed raster content (MRC) encoding. Uncertain marks, especially outside recognized areas, may remain. An already-clean Black & White page may correctly produce no changes.

In the panel, **Undo** shows the original review image and **Redo** restores the edited preview. After editing options, use **Preview changes**, then **Apply** to persist only the reviewed current working page. The corner **X** closes without applying. The main session Undo/Redo restores corrections, clean-copy Apply, rotation, deletion and ordering without rerunning OCR or acquisition. Its 32-edit history lasts for the current open session, not across restart. Acquisition originals are never replaced.

## OCR, boxed forms and notes

OCR is local, with separately supplied, hash-verified ocrs/RTen models. `session_words` caches by image content and model signatures; changed pixels invalidate stale recognition. Thin-line form preprocessing improves recognition input for boxed/ruled text without silently changing the saved image. Latin-alphabet models still require review of names, numbers, punctuation, handwriting and dense tables.

Scan Note lets an embedding host receive the current session for its own note editor. Saving, optional note formatting and catalog integration belong to the host. PaperDesk's catalog and note editor are not dependencies of this module. Keep source wording available and never add inferred facts to extracted notes.

## Compression and exports

PDF compression is lossless **relative to the chosen working image**, without automatic downsampling. Bilevel pages compare generic-region JBIG2, CCITT Group 4 and Flate. JBIG2 is independently decoded and pixel-verified; lossy symbol substitution is not used. Gray/color use level-9 Flate. Searchable text overlays do not visually retype the scan. JPEG export remains lossy.

A fixed size such as 700 KB for 20 pages cannot be promised: noise, dimensions, photos and OCR/font data matter. Never discard source content to meet a byte target. PDF append creates a new copy; document-level signatures, forms and bookmarks are not preserved. Never describe pixel-changing cleanup as lossless restoration.

## Scanner and platform findings

- Refresh discovery and recover stale selections. Re-query the selected scanner's supported sources/DPI instead of retaining another device's settings.
- Parse SANE diagnostics and device rows robustly; use cooperative child cancellation. Check actual Feeder/Duplex support and paper availability.
- Virtual machines need the scanner attached to the guest. Host discovery is not guest access. Fujitsu fi-7030 acquisition was user-confirmed in Ubuntu on an earlier Rust release; new releases still need hardware retesting.
- Windows uses WIA. Linux/macOS use separately installed SANE `scanimage`; `OPENSCAN_SCANIMAGE` supplies an explicit path. Apple ImageCaptureCore is not implemented.
- Use target-native user directories, not build-host drive letters, UNC paths or separators. Validate portable relative session paths and reject traversal.
- Debian archives need parent directories before children and correct Unix modes. Windows-side extraction does not prove native installation works.

## Validation and future tuning

Synthetic regressions cover faint ink, punctuation, crossing rules, color, barcode geometry, halftones, unknown marks, DPI scaling, independent controls, invalid input and cancellation. Native production checks on three 300-DPI grayscale scan copies and a corrected Black & White page preserved every source file and pixel outside recognized regions. Independent Poppler extraction restored every exported image pixel. The Black & White sample was unchanged by OCR Clean. Private scans are not included; synthetic fixtures are.

For further tuning, retain originals, record DPI/mode/settings, compare the same input and geometry, inspect at native resolution, check text and graphics independently, and verify export with a separate decoder. Add a regression fixture before relaxing preservation rules. Smaller files or better OCR text alone do not establish image fidelity.

Primary references: [Sauvola and Pietikainen (2000)](https://doi.org/10.1016/S0031-3203(99)00055-2), [ocrs](https://github.com/robertknight/ocrs), [SANE](https://www.sane-project.org/) and [ABBYY area recognition](https://help.abbyy.com/en-us/finereader/16/user_guide/editareas/). This Rust implementation does not use ABBYY source code.
