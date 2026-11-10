# OpenScan changes

## ver0.2.7 — Rust and egui/eframe

This release replaces the old C# implementation with reusable Rust crates. Original MIT licensing and repository history are retained. All scanner improvements developed through PaperDesk ver0.2.7 are included without a PaperDesk runtime dependency.

- `scan-module`: WIA/SANE acquisition, validated profiles, native paths, durable sessions, immutable acquisition originals and independent history API.
- `openscan-egui`: embeddable presets/settings/preview, saved correction preferences, zoom, progress/cancellation, deletion/context menu and exact Undo/Redo; standalone eframe/wgpu example.
- `egui-office-widgets`: shared local progress/control crate, independent of PaperDesk.
- Balanced Sauvola: k=0.34, R=128, DPI-scaled 51-pixel window at 300 DPI, bounded memory, cancellation and exact black retention.
- Content-aware OCR Clean protects connected ink, punctuation, graphics and uncertain regions. Pixels outside recognized text stay unchanged. No automatic retyping or blanket erasure.
- Curved labeled review Undo/Redo, Preview changes, current-page Apply and corner X; original acquisitions and reversible working edits retained.
- Local ocrs/RTen, boxed/ruled-form recognition preparation, content/model-addressed caching and optional Scan Note event for host note editors.
- Lossless searchable PDF with independently verified generic-region JBIG2, CCITT Group 4 or Flate; no downsampling or lossy symbol matching. Explicit image/JPEG export and append-as-new-copy.
- Robust scanner discovery, stale device/source recovery, target-native data/model paths, relative session references and traversal rejection.
- Straighten, Crop and Remove dust default ON; saved choices remain respected. Atomic correction settings do not modify named presets or old pages.
- Synthetic tests, examples, licenses, migration instructions and the [scan-quality guide](crates/scan-module/docs/SCAN_QUALITY.md) record defaults, tuning evidence and limits.

The core has no GUI/database/.NET dependency. The panel uses egui and local widgets; eframe is optional for embedded use. Models require explicit setup. SANE is separately installed on Linux/macOS. The historical C# JSON protocol is not implemented; `examples/client.py` is a legacy reference. Native macOS validation and ver0.2.7 hardware retesting remain pending. Full semantic layout recognition, MRC and fold reconstruction are not implemented. Conservative cleanup can leave marks.
