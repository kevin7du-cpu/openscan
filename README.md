# OpenScan

A local, reusable Windows document-scanning component with separate **Scan** and
**Manage presets** windows. Original implementation: MIT. No subscription, paid
scanner SDK, cloud service, API key or document upload is required.

## Use with PdfCraft

The modified PdfCraft source adds **From Scanner → Scan / Manage presets** to its
toolbar and File menu. The companion returns the saved PDF to PdfCraft, which opens
it in a normal document tab. Existing open documents are left intact.

For deployment, copy **the entire** `dist/win-x64` directory to `scanner` beside
`pdfcraft.exe`. Keep all its DLLs, OCR data and notices together. The companion is
self-contained: a separately installed .NET runtime is not required.
OCR's native Tesseract library needs the free Microsoft Visual C++ v14 x64 runtime
already present on many Windows computers. If it is missing, use
[Microsoft's official installer](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
The runtime and manufacturer scanner drivers are system prerequisites, not bundled components.

For development, set `PDFCRAFT_SCANNER` to the absolute path of `OpenScan.exe` before
launching PdfCraft. The installed stock PdfCraft executable does not gain menus
from copying files; use a build of the modified source.

## Scanning

1. Open **From Scanner → Scan** and choose your scanner.
2. Choose flatbed, feeder or duplex (only sources reported by the driver appear),
   color, paper size and resolution. Use the scanner's interface for vendor options.
3. Select corrections and press **Scan / add pages**. Press it again for another
   flatbed sheet or batch. Cancel stops acquisition when the driver next responds.
4. Review pages. Rotate, reorder, or include/exclude pages. **Apply corrections**
   regenerates pages from originals using the current settings.
5. Save a PDF, save PNG pages, or append to an existing PDF **as a new copy**.
   The PdfCraft integration opens the saved PDF automatically.

Supported corrections: deskew (up to ±12° when enough straight-line evidence is
present), content crop with a small margin, background normalization, median
despeckling, sharpening, brightness/contrast, grayscale/black-and-white conversion,
90° rotations, duplex-back rotation and reversible blank-page exclusion. English
Tesseract OCR produces searchable PDFs. JPEG quality controls PDF image compression.
Auto-crop and blank detection should be reviewed on faint originals or photographs.

## Presets and recovery

**Save as preset** captures the current scanner and settings. **Manage presets**
provides New, Edit, Duplicate, Rename and Delete. Duplicate names require explicit
replacement. Settings are stored in `%LOCALAPPDATA%\OpenScan\Presets`.

Originals, processed pages and `session.json` are retained in
`%LOCALAPPDATA%\OpenScan\Sessions`. **Session → Open saved session** restores a batch.
**Session → Show session folder** reveals the retained files. Nothing is uploaded.
Sessions are not automatically deleted; remove old session folders after you no
longer need recovery copies.

## Reuse without rebuilding the scanner

- **Any language:** ship the existing `dist/win-x64` bundle and use its versioned
  JSON process interface. See `docs/API.md` and `examples/client.py`.
- **.NET host:** reference `OpenScan.Core`; reuse `IScannerBackend`,
  `WiaScannerBackend`, `ScanService`, `ScanSession`, `ImageProcessor`, `PresetStore`
  and `PdfWriter`. The core has no dependency on PdfCraft or on the OpenScan windows.
- **Another driver:** implement `IScannerBackend`. Processing, presets, sessions
  and output remain unchanged. WIA objects stay on dedicated STA threads.

## Build and test

Requires the free .NET 10 SDK on Windows x64. Building the native imaging runtime
once also requires CMake and a MinGW-w64 C++ toolchain (GCC 16.2 was tested):

```powershell
.\native\build-native.ps1 -ToolchainBin 'C:\path\to\mingw64\bin'
.\build.ps1 -Test
```

Package versions and NuGet lockfiles are checked in. English OCR data is pinned by
commit and SHA-256. `PDFsharp-GDI` deliberately uses MIT-licensed **6.2.0**; do not
automatically upgrade to differently licensed versions. `native/build-native.ps1`
builds only OpenCV core, imgproc and imgcodecs from checksum-pinned source, with IPP,
nonfree algorithms and video/FFmpeg disabled. The prebuilt module includes that
runtime; applications using the JSON interface need no build tools. License texts
are in `licenses/`. `-OutputDirectory` can select a fresh publish directory.

The OCR text layer embeds the bundled OFL Inter font. A .NET host that also uses
PDFsharp should call `OpenScanFonts.Register()` at startup, before creating fonts;
it preserves the host resolver as a fallback. The JSON interface isolates these
process-global PDFsharp settings automatically.

## Current boundaries

- Windows x64, WIA 2.0 drivers. TWAIN-only scanners, eSCL, macOS and Linux backends
  are not implemented. Install your manufacturer's WIA driver if needed.
- Scan resolution: 75–600 DPI, up to 250 pages per session and 60 million pixels per
  page. Feeder/duplex behavior depends on the physical device and driver.
- Native scanner dialogs control their own acquisition settings; cancellation
  of a blocking vendor dialog must be done inside that dialog.
- English OCR only. No page-orientation OCR, book dewarping, barcode routing,
  document-edge perspective correction or PDF/A certification.
- PDF append copies pages, not interactive document-level forms, signatures or
  bookmarks. Keep the original PDF for those features.
- Automated tests cover synthetic acquisition/failure, correction, recovery,
  presets, PDF dimensions, append safety, runtime contents and OCR font provenance
  (33 checks). A physical
  fi-7030 feeder scan passed at 300 DPI in color, including PDF rendering and a
  subsequent background-cleanup/OCR export from the retained original. HP Smart
  Tank 7600 discovery and capabilities passed; HP acquisition and duplex paper
  acquisition have not been physically tested.
