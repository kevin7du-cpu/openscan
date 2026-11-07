# Validation — 2026-11-07

Windows x64; .NET SDK 10.0.403. OpenCV 4.13.0 and the pinned OpenCvSharp wrapper
were compiled using GCC 16.2 with core, imgproc and imgcodecs only. IPP, nonfree
algorithms, OpenCL and video/FFmpeg are disabled.

`build.ps1 -Test` passes 33 checks: transfer streams positioned at EOF; empty
transfer rejection; measured 4.5-degree deskew; preserved originals; blank-page
detection; cropping; binary black-and-white cleanup; settings validation;
preset create/read/edit/delete and overwrite protection; session recovery;
non-destructive rotation and correction; PDF page size; append and overwrite
safety; cancellation; pages retained after a simulated paper jam; searchable
OCR; minimal native runtime; and use of the bundled OFL font in PDF output.

Physical validation:

- A fi-7030 produced a color Letter feeder scan at 300 DPI. The PDF rendered
  successfully. The retained original also passed background cleanup and OCR.
- HP Smart Tank 7600 discovery and capability reads succeeded. Its reported
  flatbed/feeder capabilities were shown in the selection UI.
- The Scan window imported a synthetic skewed image, displayed its correction,
  saved a PDF and returned it to a normal PdfCraft document tab.
- Preset CRUD through PdfCraft's JSON adapter passed without operating hardware.

HP acquisition, physical duplex acquisition, other scanner models and operating
systems have not been validated. No physical test pages, personal documents,
device identifiers or recovery sessions are included in the repository.
