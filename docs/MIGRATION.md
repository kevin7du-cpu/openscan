> OpenScan 0.3.0 uses the new accepted interface and `openscan-core` pipeline.
> Start with [the 0.3 integration guide](OPENSCAN_0_3.md). The reference below
> describes compatibility APIs; its former panel is now `LegacyOpenScanUi`.

# Migration from C# to Rust

The C# application, C# library, C# tests, NuGet lockfiles, .NET build properties and
OpenCvSharp native build helper are removed from the current source tree.
Git history, the original MIT license, historical third-party license texts and
the Python process client are preserved.

The replacement includes these reusable crates:

- `scan-module`: acquisition adapters, profiles/presets, image correction,
  recoverable sessions, optional OCR/PDF.
- `openscan-egui`: embeddable scan/review/preset UI, with background workers and
  an optional standalone eframe/wgpu host.
- `egui-office-widgets`: local shared controls and progress popup used by the panel; no PaperDesk dependency.

ver0.2.7 includes built-in correction defaults, balanced Sauvola, conservative OCR Clean, reversible history, lossless compression and platform fixes. Read [scan-quality defaults and findings](../crates/scan-module/docs/SCAN_QUALITY.md) before overriding them. Existing explicit settings remain respected; saved scans are not automatically regenerated.

WIA replaces the NAPS2 WIA wrapper on Windows. Rust image/imageproc replaces
OpenCvSharp; ocrs/RTen replaces Tesseract; lopdf replaces PDFsharp.
No .NET runtime, C# compiler or NuGet package is needed to build or run the module.
SANE is an optional separately installed open-source native tool on Linux/macOS.

## Compatibility

The established scan/preset control layout and workflow are preserved. Profile
version-1 camelCase JSON remains supported. Session import supports the original
array manifests when their files remain within the session directory; relative
Rust manifests make newly saved sessions movable together with their contents.

Image algorithms are not pixel-identical to OpenCV. Review saved presets against
representative scans. The Rust OCR engine uses Latin-alphabet models, not the
former Tesseract training data.

The historical JSON process protocol and Python client are not implemented by
the Rust standalone example. `examples/client.py` remains explicitly labeled
legacy. Use the Rust library/panel API or retain a previous release for an
existing process-protocol host until that host is migrated.

Windows standalone storage now defaults to `%LOCALAPPDATA%/OpenScan/Rust` to avoid
writing into a previous C# installation. Use `--storage` to select another
location. No existing user sessions, presets or exports are deleted automatically.

Old physical scanner validation belongs to the old implementation. It must not be
treated as Rust backend validation. See [VALIDATION.md](VALIDATION.md).
