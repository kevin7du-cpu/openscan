//! The same pure-Rust OCR engine and pinned models as PdfCraft (ocrs + RTen).
use crate::*;
use ocrs::TextItem;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Ocr(ocrs::OcrEngine);
#[derive(Debug)]
pub struct Word {
    pub text: String,
    pub rect: [f64; 4],
}
const MODELS: [(&str, &str); 2] = [
    (
        "text-detection.onnx",
        "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe",
    ),
    (
        "text-recognition.onnx",
        "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5",
    ),
];
pub fn model_directory() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("OPENSCAN_MODELS") {
        let p = PathBuf::from(p);
        if !p.is_absolute() {
            return Err(error("OPENSCAN_MODELS must be absolute."));
        }
        return Ok(p);
    }
    let exe = std::env::current_exe()?;
    let parent = exe
        .parent()
        .ok_or_else(|| error("Cannot locate the application."))?;
    Ok(parent.join("models"))
}
pub fn available() -> bool {
    model_directory().is_ok_and(|d| MODELS.iter().all(|(name, _)| d.join(name).is_file()))
}
impl Ocr {
    pub fn load(directory: &Path) -> Result<Self> {
        for (name, hash) in MODELS {
            let path = directory.join(name);
            if !path.is_file() {
                return Err(error(
                    "OCR models are missing. Run the documented model download step, then enable Searchable PDF.",
                ));
            }
            if std::fs::metadata(&path)?.len() > 256 * 1024 * 1024 {
                return Err(error("OCR model exceeds its size limit."));
            }
            if format!("{:x}", Sha256::digest(std::fs::read(path)?)) != hash {
                return Err(error(format!("OCR model failed its SHA-256 check: {name}")));
            }
        }
        let load = |name: &str| {
            rten::Model::load_file(directory.join(name)).map_err(|e| error(e.to_string()))
        };
        let engine=ocrs::OcrEngine::new(ocrs::OcrEngineParams{
            detection_model:Some(load("text-detection.onnx")?),recognition_model:Some(load("text-recognition.onnx")?),
            alphabet:Some(" 0123456789!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~€ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".into()),..Default::default()
        }).map_err(|e|error(e.to_string()))?;
        Ok(Self(engine))
    }
    pub fn recognize(
        &self,
        image: &image::DynamicImage,
        cancel: &Cancellation,
    ) -> Result<Vec<Word>> {
        cancel.check()?;
        let rgb = image.to_rgb8();
        let input = ocrs::ImageSource::from_bytes(rgb.as_raw(), rgb.dimensions())
            .map_err(|e| error(e.to_string()))?;
        let input = self
            .0
            .prepare_input(input)
            .map_err(|e| error(e.to_string()))?;
        let boxes = self
            .0
            .detect_words(&input)
            .map_err(|e| error(e.to_string()))?;
        if boxes.len() > 50_000 {
            return Err(error("OCR page exceeds 50,000 text regions."));
        }
        cancel.check()?;
        let lines = self.0.find_text_lines(&input, &boxes);
        let lines = self
            .0
            .recognize_text(&input, &lines)
            .map_err(|e| error(e.to_string()))?;
        let mut words = Vec::new();
        for line in lines.into_iter().flatten() {
            for word in line.words() {
                let r = word.bounding_rect();
                words.push(Word {
                    text: word.to_string(),
                    rect: [
                        f64::from(r.left()),
                        f64::from(r.top()),
                        f64::from(r.right()),
                        f64::from(r.bottom()),
                    ],
                });
            }
        }
        cancel.check()?;
        Ok(words)
    }
}
