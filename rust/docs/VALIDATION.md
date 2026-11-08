# Validation status

The Rust scanner source at `8677344d5780305f9cb17fe91eb5d5f3c0000b2a` passed
[Windows, Linux and macOS native validation](https://github.com/kevin7du-cpu/openscan/actions/runs/42430141576).

| Runner | Tests passed | Format / Clippy / release build / startup |
| --- | ---: | --- |
| Windows Server 2025 x64 (10.0.26100) | 23 | Passed |
| Ubuntu 24.04.5 x64 | 23 | Passed |
| macOS 26.7.2 ARM64 | 22 | Passed |

The Linux total includes an explicit acquisition using the installed SANE
`test:0` driver. It acquired an A5 page at 75 DPI, checked both dimensions within
one pixel, exported a PDF and refused unsupported 100 DPI before acquisition.
That test is normally ignored in the general suite, then explicitly run by CI.
Windows includes an additional BMP-format selection test. These are synthetic
driver checks, not proof of physical device compatibility.

Core tests cover deskew, black-and-white output, presets, input bounds, cancellation
recovery, original preservation, multipage TIFF, PDF append and image color channels.
A local synthetic HTTP scanner exercises feeder transfer and scan-job deletion.
egui tests exercise Scan/Cancel, the scan and preset panels at 480/1040-point
widths, accessible control names and the Close action. A regression reproduced a
clipped Close button at the minimum width; the corrected layout passed at three
window heights, and the actual widgets were rendered and inspected.

An earlier Linux artifact built on ubuntu-latest required GLIBC 2.43 and could not
start on PDFCraft's Ubuntu 24.04 validation host. The final build uses Ubuntu 24.04;
its binary references no GLIBC version newer than 2.39 and its release startup
passed on that host. The listed native runners are the tested OS/CPU combinations,
not a claim that every older OS, distribution or architecture is supported.

## Native PDFCraft interoperability

[The final integration run](https://github.com/kevin7du-cpu/openscan/actions/runs/42431338625) pairs the exact scanner binaries
with the modified PDFCraft application on Windows, Linux and macOS. Each platform
passed seven checks: preset create/list, refusal of implicit overwrite, explicit
update, deletion, root-path enforcement, invalid-setting errors, and real-model
OCR/PDF export followed by independent PDFCraft text extraction and rendering.
Both OCR models were SHA-256 verified and loaded from beside the scanner binary,
without a model-path environment override. The models support English/ASCII and
the euro symbol; this is not an Arabic or general multilingual OCR test.

The final Windows executable also passed the same seven checks locally with
compiler directories removed from PATH. Its OCR result was visually inspected.

## Hardware and native desktop work still pending

Installed Windows drivers enumerate the HP Smart Tank 7600 and fi-7030. The
fi-7030 reports feeder/duplex and 75-600 DPI. Its BMP transfer and completion-callback
requirements were corrected; an empty-feeder acquisition completes with a clear
error. A new physical test sheet through this Rust build, duplex/HP acquisition,
native desktop interaction and real Linux/macOS devices still need testing.

The earlier C# build's successful fi-7030 scan is not a Rust hardware test.
The source remains a development build until the required manual checks finish.
Compilation, mocked drivers, protocol tests and API discovery must not be described
as successful physical scans.
