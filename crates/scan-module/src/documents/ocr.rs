use super::TextLine;
use anyhow::{Context, Result, ensure};
use image::RgbImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams, TextItem};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedWords {
    version: u32,
    source: String,
    models: Vec<String>,
    words: Vec<TextLine>,
}

/// Share recognition between scan cleanup, scan notes and searchable export.
/// Content-addressed entries automatically invalidate on any image correction.
pub fn session_words(
    session: &crate::session::ScanSession,
    page: &crate::session::PageRecord,
    models: &Path,
    engine: &mut Option<LocalOcr>,
    cancel: &crate::CancellationToken,
) -> Result<Vec<TextLine>> {
    cancel.check()?;
    let path = session.resolve(&page.image_path)?;
    ensure!(
        fs::metadata(&path)?.len() <= crate::processing::MAX_FILE_BYTES,
        "Scan image exceeds size limit"
    );
    let bytes = fs::read(&path)?;
    let source = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let model_ids = MODELS
        .iter()
        .map(|(_, hash)| hash.to_string())
        .collect::<Vec<_>>();
    let directory = session.directory.join("ocr-cache");
    fs::create_dir_all(&directory)?;
    ensure!(
        directory
            .canonicalize()?
            .starts_with(session.directory.canonicalize()?),
        "OCR cache escapes session"
    );
    let cache_path = directory.join(format!("{source}.json"));
    if let Ok(meta) = fs::symlink_metadata(&cache_path)
        && meta.is_file()
        && meta.len() <= 8 * 1024 * 1024
        && let Ok(data) = fs::read(&cache_path)
        && let Ok(cache) = serde_json::from_slice::<CachedWords>(&data)
        && cache.version == 1
        && cache.source == source
        && cache.models == model_ids
        && cache.words.len() <= 5000
        && cache
            .words
            .iter()
            .all(|w| w.text.len() <= 8000 && w.bounds.iter().all(|b| b.is_finite()))
    {
        cancel.check()?;
        return Ok(cache.words);
    }
    if engine.is_none() {
        *engine = Some(LocalOcr::load(models)?);
    }
    let image = crate::processing::decode(&bytes)?;
    let words = engine.as_ref().unwrap().recognize_words(&image)?;
    cancel.check()?;
    ensure!(words.len() <= 5000, "Too many OCR regions");
    crate::presets::atomic_json(
        &cache_path,
        &CachedWords {
            version: 1,
            source,
            models: model_ids,
            words: words.clone(),
        },
        true,
    )?;
    Ok(words)
}

pub const MODELS: [(&str, &str); 2] = [
    (
        "text-detection.rten",
        "f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca",
    ),
    (
        "text-recognition.rten",
        "e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e",
    ),
];
pub struct LocalOcr {
    engine: OcrEngine,
}
impl LocalOcr {
    pub fn load(directory: &Path) -> Result<Self> {
        let mut models = Vec::new();
        for (name, hash) in MODELS {
            let bytes=fs::read(directory.join(name)).with_context(|| format!("Missing OCR model {name}. Run scripts/setup-ocr-models.mjs or select the installed model directory."))?;
            ensure!(
                Sha256::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
                    == hash,
                "OCR model checksum mismatch: {name}"
            );
            models.push(rten::Model::load(bytes)?);
        }
        let recognition_model = models.pop();
        let detection_model = models.pop();
        Ok(Self {
            engine: OcrEngine::new(OcrEngineParams {
                detection_model,
                recognition_model,
                ..Default::default()
            })?,
        })
    }
    pub fn recognize(&self, image: &RgbImage) -> Result<Vec<TextLine>> {
        self.recognize_regions(image, false)
    }
    /// Word boxes retain the placement of columns and widely spaced labels when
    /// reconstructing a page; ordinary searchable export continues using lines.
    pub fn recognize_words(&self, image: &RgbImage) -> Result<Vec<TextLine>> {
        self.recognize_regions(image, true)
    }
    fn recognize_regions(&self, image: &RgbImage, by_word: bool) -> Result<Vec<TextLine>> {
        let input = self
            .engine
            .prepare_input(ImageSource::from_bytes(image.as_raw(), image.dimensions())?)?;
        let words = self.engine.detect_words(&input)?;
        let lines = self.engine.find_text_lines(&input, &words);
        let recognized = self.engine.recognize_text(&input, &lines)?;
        Ok(recognized
            .into_iter()
            .flatten()
            .flat_map(|line| {
                let make = |text, item: &dyn TextItem| {
                    let rect = item.bounding_rect();
                    TextLine {
                        text,
                        bounds: [
                            rect.left().max(0) as f32,
                            rect.top().max(0) as f32,
                            rect.width().max(1) as f32,
                            rect.height().max(1) as f32,
                        ],
                    }
                };
                if by_word {
                    line.words()
                        .map(|word| make(word.to_string(), &word))
                        .collect::<Vec<_>>()
                } else {
                    vec![make(line.to_string(), &line)]
                }
            })
            .collect())
    }
}
