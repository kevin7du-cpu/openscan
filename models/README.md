# Local OCR models

Run `node scripts/setup-ocr-models.mjs` once from the project. The application never uploads documents and does not download models itself. The setup script verifies SHA-256 hashes before installation.

These unmodified text detection and recognition models are by Robert Knight, trained on HierText and other open data. They are distributed under **CC BY-SA 4.0**; the engine and RTen runtime are MIT OR Apache-2.0. No model changes have been made.

- Source: https://github.com/robertknight/ocrs-models
- Maintainer's model license clarification: https://github.com/robertknight/ocrs-models/issues/34
- License: https://creativecommons.org/licenses/by-sa/4.0/
- Downloads: https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten and https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten
- Byte-identical fallback mirror (same required hashes): https://github.com/deeplabua/deepocr/releases/tag/models-v1

The Latin alphabet models are an early-preview OCR engine; users must review recognition results. Other writing systems are not supported by this adapter.
