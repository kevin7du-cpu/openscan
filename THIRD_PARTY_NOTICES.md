# Third-party notices

Original OpenScan design and source: MIT, copyright 2026 OpenScan contributors.
The original LICENSE is retained; both reusable crates include OPENSCAN-LICENSE.
The Rust implementation is MIT OR Apache-2.0; see LICENSE-MIT and LICENSE-APACHE.

Cargo dependencies and exact resolved versions are recorded in Cargo.lock.
DEPENDENCY_LICENSES.md lists the selected graph's declared licenses, including
development and target-specific dependencies. Generate it using:

```sh
cargo metadata --locked --all-features --format-version 1 > target/dependency-metadata.json
node scripts/dependency-licenses.mjs
```

The embedded unmodified Hack font is distributed under its MIT and Bitstream
Vera notices in crates/scan-module/assets/Hack-LICENSE.txt. Keep that file with
the font when reusing the crate.

OCR models by Robert Knight are separate data under CC BY-SA 4.0, with source,
download links and attribution in models/README.md. Models are not committed
and are never downloaded by the application. The setup helper and runtime
verify exact SHA-256 values. ocrs and RTen are MIT OR Apache-2.0.

Windows uses operating-system WIA and the installed device driver. Linux/macOS
optionally invoke a separately installed open-source SANE scanimage. Neither is
bundled as a Cargo dependency.

The pre-existing files in licenses/ are preserved historical notices from the
removed C# release. They do not describe current runtime dependencies. In
particular, .NET, NAPS2, OpenCvSharp, OpenCV, PDFsharp, Tesseract and the old native
runtime are no longer built or bundled by this source tree.

The unmodified Ubuntu Light font is under the Ubuntu Font Licence in crates/scan-module/assets/Ubuntu-Font-Licence.txt. Keep it with the font. The egui-office-widgets crate is included locally (MIT OR Apache-2.0) for shared progress controls; it has no PaperDesk dependency. Lossless bilevel export uses the pinned open-source fax, jbig2enc-rust and hayro-jbig2 dependencies listed in the lockfile.

## OpenScan 0.3 workflow source

The new openscan-core pipeline and current.rs interface derive from the supplied
Scan Module 0.1.3 Rust source, whose files carry SPDX-License-Identifier:
MIT OR Apache-2.0. Its crop, shadow, deskew, despeckle and Otsu algorithms are
retained. The synthetic skewed_page.png test fixture accompanies that source.
Image processing uses image 0.25.10 and imageproc 0.25.1 (MIT); PDF composition
uses lopdf 0.34.0 (MIT) and fax 0.2 (MIT). Tesseract is an optional separately
installed Apache-2.0 program. NAPS2 is not bundled and its external adapter is
disabled by default. No operating-system or proprietary icon/font assets were
added; the dropdown chevron is drawn as vector geometry.
