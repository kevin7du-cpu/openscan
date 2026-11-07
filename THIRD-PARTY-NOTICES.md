# Third-party components

OpenScan's original code is MIT-licensed. It uses the following free, open-source
components; no paid SDK or license key is required. Preserve their notices when
redistributing the compiled bundle. `packages.lock.json` records resolved package
versions and package content hashes.

| Component | Pinned version/source | License | Purpose |
|---|---|---|---|
| NAPS2.Wia | NuGet 2.0.3; https://github.com/cyanfish/naps2-wia | MIT | Windows WIA 2.0 acquisition |
| OpenCvSharp | 4.13.0.20260627; commit `b161e7e012f5101f6d5dc68a835c59db6cc88b18` | Apache-2.0 | Managed imaging interface |
| OpenCV | 4.13.0, minimal source build | Apache-2.0; bundled codecs retain their licenses | Deskew, crop, image cleanup |
| libjpeg-turbo / libpng / zlib | sources bundled in OpenCV 4.13.0 | IJG / BSD-3-Clause / Zlib / libpng-2.0 | Image codecs; notices retained separately |
| GCC runtime / winpthreads | GCC 16.2, MinGW-w64 | GPL-3.0 with GCC-exception-3.1 / MIT and BSD-style notices | Compiler runtime; exception permits distribution with this module |
| Inter Regular | Rasmus Andersson / Inter authors; bytes reused from PdfCraft commit `5644d56c64af1529b0f9012874886fcad855c257`, `assets/fonts/Inter-Regular.ttf` | OFL-1.1 | Searchable PDF font, embedded in OpenScan.Core |
| PDFsharp-GDI | **6.2.0**; https://github.com/empira/PDFsharp/tree/v6.2.0 | MIT | PDF creation and page import |
| Tesseract .NET | NuGet 5.2.0; https://github.com/charlesw/tesseract | Apache-2.0 | OCR wrapper |
| Tesseract | 5.2.0 native library | Apache-2.0 | English text recognition |
| Leptonica | 1.82.0, supplied with Tesseract package | BSD-2-Clause | OCR image support |
| tessdata_fast English | commit `87416418657359cb625c412a48b6e1d6d41c29bd` | Apache-2.0 | OCR language model |
| .NET / Windows Forms | self-contained .NET 10 runtime | MIT and bundled third-party notices | Runtime and native desktop controls |
| Microsoft.Extensions / System.Reflection.Emit | versions in NuGet lockfiles | MIT | Runtime support |

The model's SHA-256 is
`7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2`.
License texts are retained in `licenses/`. The source-built OpenCV runtime includes
only core, imgproc and imgcodecs. IPP, nonfree algorithms, OpenCL and video/FFmpeg
are disabled by `native/build-native.ps1`. This software is based in part on the
work of the Independent JPEG Group. This project uses the MIT NAPS2.Wia package, not the differently
licensed NAPS2 application/SDK. PDFsharp is pinned exactly to its MIT release;
later differently licensed versions must not be substituted automatically.

Windows scanner drivers are supplied by the scanner manufacturer and are not
redistributed in this module. Windows draws native controls using installed UI
fonts; those fonts are not redistributed. The OCR text layer uses only the bundled
OFL font (SHA-256 `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82`).
No reference application's artwork is included.
