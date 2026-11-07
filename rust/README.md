# OpenScan — Rust scanner module

Reusable scanning, image correction, presets and searchable PDF export, with an
egui interface. The native application and processing pipeline are Rust. It needs
no paid SDK, subscription, account, server, .NET runtime or document upload.

This is the 0.2 development implementation. The repository's older C# implementation
is retained separately for historical builds; it is not a dependency of this build.

## Platforms and drivers

| Platform | Acquisition | Interface |
| --- | --- | --- |
| Windows | Microsoft's Windows scanner API over installed WIA drivers | egui/eframe; native Windows file dialogs |
| Linux | Installed SANE `scanimage`; eSCL/AirScan network devices | egui/eframe on Wayland or X11; desktop portal dialogs |
| macOS | Installed SANE `scanimage`; eSCL/AirScan network devices | egui/eframe; native macOS file dialogs |

The OS or manufacturer still supplies the physical scanner driver. Linux users
usually install `sane-utils`; macOS users can install `sane-backends` through their
package manager. Native ICA and TWAIN acquisition are not implemented. A device
that works in the OS's scanner app is not guaranteed to have a SANE driver.

Refresh lists installed drivers. **Search network scanners** explicitly enables
mDNS discovery and may cause an OS firewall prompt. **Add network scanner by
address** accepts an eSCL URL without opening a discovery listener. HTTPS certificate
validation stays enabled. The module never changes firewall rules.

## Scan and manage presets

`openscan dialog` opens Scan; `openscan presets-dialog` opens Manage presets.
Choose the device, source/sides, color, paper, resolution and maximum page count.
Scan or import images, review the preview, reorder/rotate pages and exclude unwanted
pages. Apply corrections always starts from the retained original.

Corrections include deskew, content crop, background whitening, despeckle, sharpen,
brightness/contrast, grayscale/adaptive black-and-white, blank-page exclusion and
duplex-back rotation. Imported PNG/JPEG/BMP/PNM and common multipage TIFFs are supported.
Review automatic crop/blank detection for faint text, photographs and unusual layouts.

Save PDF creates a new file. Append creates a new PDF copy, retaining the source's
catalog, page tree and forms; encrypted or digitally signed inputs must be handled
in the host application. Save images creates a new folder of PNG pages.
OCR uses the same free `ocrs`/RTen engine and checksum-pinned models as PDFCraft.
The current recognition alphabet is English/ASCII plus the euro sign. No cloud OCR,
automatic page-orientation recognition, perspective correction or book dewarping is claimed.

Profiles and recovery sessions use the platform application-data directory
(`OpenScan` under local application data on Windows, `org.OpenScan.OpenScan` on
macOS, and `openscan` on Linux). `openscan dialog --session <absolute-directory>`
restores a saved session. Originals and previous correction outputs are retained;
sessions are never silently deleted. Limits: 75–600 DPI, 250 pages, 60 megapixels
per image and 256 MB per imported/acquired image.

## Reuse in another application

* **Without rebuilding:** ship the platform's existing OpenScan binary, models and
  notices. Spawn it and exchange JSON on stdin/stdout using [protocol v1](docs/API.md).
  `dialog --return-json` returns the saved PDF path when the user finishes.
* **Rust host:** depend on `openscan-core` for `Backend`, `Profile`, `Session`,
  correction, OCR and PDF export. It has no GUI dependency.
* **egui host:** embed `openscan-ui::ScanPanel`. It returns typed actions; the host
  owns workers, cancellation, native dialogs and window lifecycle.
* **Additional driver:** implement the `Backend` trait; processing, profiles,
  recovery and output do not need to be rebuilt from scratch.

The egui, eframe, winit, craft-ui and craft-dialogs revisions match PDFCraft's
reviewed GUI stack. Native dialogs are queued through craft-dialogs to avoid
blocking/reentering the macOS event loop. Controls use accessible labels and
PDFCraft's outlined 30-point dialog convention. Most controls are egui-drawn;
they are not OS-native widgets. [GUI references](docs/GUI.md).

## Build and check

Install the free stable Rust toolchain (1.95 or newer) and the platform's native
linker/build prerequisites. On Linux install `pkg-config`, `libxkbcommon-dev` and
`libwayland-dev` for the window stack. From this directory:

```sh
cargo test --locked --workspace -- --test-threads=1
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo build --locked --release -p openscan
cargo run --locked -p openscan-core --example download_models -- target/release/models
```

The explicit download verifies SHA-256 before installing either OCR model. Distribute
`target/release/openscan[.exe]`, its adjacent `models/` folder, LICENSE, NOTICE,
THIRD-PARTY-NOTICES.html and `licenses/`. The model licence is CC-BY-SA-4.0; attribution
and unchanged model hashes are in [NOTICE](NOTICE). Keep all notices with the build.

For PDFCraft, put this bundle in `scanner/` beside its executable, or point
`PDFCRAFT_SCANNER` at the absolute binary path. On macOS this is beside the executable
inside the app's `Contents/MacOS` folder. Stock PDFCraft must first include the
provided adapter/menu change.

Only Windows has been exercised locally so far. Linux/macOS builds and SANE's
synthetic scanner are checked by the accompanying CI workflow. A passing build is
not a physical scanner test. See [validation notes](docs/VALIDATION.md) for the
actual results and remaining checks.
