# OpenScan feature catalog — ver0.2.7

| Area | Implemented behavior |
|---|---|
| Reuse | GUI-independent core, optional egui panel, standalone eframe/wgpu example, injectable ScannerBackend |
| Acquisition | WIA/SANE discovery and capabilities; flatbed/feeder/duplex when supported; cancellation and partial recovery |
| Settings | Validated JSON presets, saved correction preferences, stale scanner/source recovery, retained explicit choices |
| Images | Multi-image/TIFF import, DPI retention, zoom, rotation, reorder, Del/context-menu delete and reversible exclusion |
| Corrections | Straighten/Crop/Remove dust ON; balanced Sauvola in Black & White; optional tonal/background/sharpen controls |
| Sessions | Originals, durable portable manifests, recovery, exact Undo/Redo and atomic current-page updates |
| OCR | Local verified ocrs/RTen models, word bounds, form-line preparation and image/model-addressed cache |
| OCR Clean | Connected-ink protection, isolated-speck removal, paper normalization, unchanged unknown regions, opt-in text replacement and manual erasure |
| Review | Curved labeled Undo/Redo, Preview changes, Apply, corner X |
| Notes | Optional Scan Note event; host supplies editor, saving and formatting |
| Output | Lossless searchable PDF, verified JBIG2/Group 4/Flate, JPEG/images and append-to-new-copy |
| UX | Worker progress/cancellation; established preset/settings/preview arrangement; File as host integration |
| Preservation | No implicit uploads/downloads, bounded analysis, originals and validated native/relative paths |

Not implemented: ImageCaptureCore, legacy C# process protocol, complete semantic layout classification, MRC, automatic fold reconstruction, non-Latin models or guaranteed fixed-size output. Read [scan-quality findings](../crates/scan-module/docs/SCAN_QUALITY.md) and [validation](VALIDATION.md).
