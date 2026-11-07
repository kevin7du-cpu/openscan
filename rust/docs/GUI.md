# PDFCraft GUI references

The module follows these source guides, reviewed at PDFCraft commit
5644d56c64af1529b0f9012874886fcad855c257 and craft-libs commit
787a7a0f972fd1d7a7618fdbeca3a5267a0407ff:

* [Architecture and layering](https://github.com/storytold/pdfcraft/blob/5644d56c64af1529b0f9012874886fcad855c257/docs/architecture.md)
* [Development](https://github.com/storytold/pdfcraft/blob/5644d56c64af1529b0f9012874886fcad855c257/docs/development.md)
* [UI parity](https://github.com/storytold/pdfcraft/blob/5644d56c64af1529b0f9012874886fcad855c257/docs/ui-parity.md)
* [Dialog widgets](https://github.com/storytold/pdfcraft/blob/5644d56c64af1529b0f9012874886fcad855c257/crates/ui-egui/src/widgets.rs)
* [Shared UI](https://github.com/storytold/craft-libs/blob/787a7a0f972fd1d7a7618fdbeca3a5267a0407ff/crates/ui/README.md)
* [Theme infrastructure](https://github.com/storytold/craft-libs/blob/787a7a0f972fd1d7a7618fdbeca3a5267a0407ff/docs/theme-infrastructure.md)
* [Native dialog queue](https://github.com/storytold/craft-libs/blob/787a7a0f972fd1d7a7618fdbeca3a5267a0407ff/crates/dialogs/README.md)

Core processing has no egui dependency. ScanPanel has no windowing/driver lifecycle.
The application handles actions outside painting and runs acquisition, correction,
preview decoding, OCR and export in cancellable workers. Driver capabilities and
files never block the UI thread. Native dialogs are requested and answered through
craft-dialogs at frame boundaries. Inputs have accessible labels; tests exercise
both windows at narrow and wide sizes.

This follows the same framework and component conventions; it does not claim every
widget is native on each OS, or that a screenshot of another product was copied.
