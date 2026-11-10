# Contributing to OpenScan

Keep the core reusable without PaperDesk, egui, eframe or a database. Keep platform adapters separate and retain the optional egui/eframe/wgpu panel. Preserve MIT/Apache licensing and original OpenScan notices. Use pinned stable dependencies and commit Cargo.lock.

Run the format, core-only check, Clippy, tests, doctests and standalone build commands in README.md. Use synthetic fixtures, not private documents. Test cancellation, original-file preservation, exact Undo/Redo, native paths and decoded PDF pixels when relevant. Follow crates/scan-module/docs/SCAN_QUALITY.md before changing cleanup defaults.

Report version, OS, scanner/backend, source, DPI/mode, settings and a minimal synthetic reproduction. Never post patient/client documents, credentials or identifying logs. A cross-build does not establish native installation or scanner success; record each separately in docs/VALIDATION.md.
