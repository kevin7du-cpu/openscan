# openscan-core 0.3.0

GUI-independent Rust scanning, accepted Scan Module cleanup, Tesseract OCR and
PDF export. See the repository README and docs/OPENSCAN_0_3.md. All blocking
operations belong on a worker. The caller owns storage and document retention.

MIT OR Apache-2.0. No required .NET runtime. The optional naps2 feature is off by
default. Default hardware backends: direct Rust WIA on Windows; SANE on Unix.
