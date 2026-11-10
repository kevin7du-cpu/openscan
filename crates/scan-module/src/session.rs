//! Durable, caller-owned sessions. Completed pages survive device failures and cancellation.
use crate::{
    CancellationToken, Result, ScanError, ScanProfile, ScanRequest, ScanSource, ScannerBackend,
    presets::{atomic_json, json_error},
    processing,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRecord {
    pub id: String,
    pub original_path: PathBuf,
    pub image_path: PathBuf,
    pub dpi: u32,
    pub deskew_angle: f32,
    pub ink_percent: f64,
    pub excluded: bool,
    #[serde(default)]
    pub rotation: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanSession {
    #[serde(skip)]
    pub directory: PathBuf,
    pub schema_version: u32,
    pub pages: Vec<PageRecord>,
}
#[derive(Debug)]
pub struct AcquisitionResult {
    pub received: usize,
    pub warning: Option<String>,
}
impl ScanSession {
    pub fn create(directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        fs::create_dir_all(directory.join("originals"))?;
        fs::create_dir_all(directory.join("processed"))?;
        if directory.join("session.json").exists() {
            return Err(ScanError::InvalidRequest(
                "Session already exists; open it instead".into(),
            ));
        }
        let session = Self {
            directory: directory.canonicalize()?,
            schema_version: 1,
            pages: Vec::new(),
        };
        session.save()?;
        Ok(session)
    }
    pub fn resolve(&self, path: &Path) -> Result<PathBuf> {
        if path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(ScanError::InvalidRequest(
                "Session path must be relative without traversal".into(),
            ));
        }
        let full = self.directory.join(path).canonicalize()?;
        if !full.starts_with(self.directory.canonicalize()?) {
            return Err(ScanError::InvalidRequest(
                "Session file escapes its directory".into(),
            ));
        }
        Ok(full)
    }
    pub fn load(directory: &Path) -> Result<Self> {
        let root = directory.canonicalize()?;
        let path = root.join("session.json");
        if fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
            return Err(ScanError::InvalidRequest(
                "Session manifest exceeds 4 MB".into(),
            ));
        }
        let bytes = fs::read(path)?;
        // C# OpenScan v1 manifests are arrays with absolute paths. Normalize them once.
        let mut session = if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') {
            let mut pages: Vec<PageRecord> = serde_json::from_slice(&bytes).map_err(json_error)?;
            for page in &mut pages {
                for path in [&mut page.original_path, &mut page.image_path] {
                    let absolute = path.canonicalize()?;
                    *path = absolute
                        .strip_prefix(&root)
                        .map_err(|_| {
                            ScanError::InvalidRequest(
                                "Legacy session file escapes its directory".into(),
                            )
                        })?
                        .into();
                }
            }
            Self {
                directory: root.clone(),
                schema_version: 1,
                pages,
            }
        } else {
            serde_json::from_slice::<Self>(&bytes).map_err(json_error)?
        };
        session.directory = root;
        if session.schema_version != 1 || session.pages.len() > 250 {
            return Err(ScanError::InvalidRequest(
                "Unsupported or oversized session".into(),
            ));
        }
        for page in &session.pages {
            if !(75..=600).contains(&page.dpi)
                || ![0, 90, 180, 270].contains(&page.rotation)
                || !page.deskew_angle.is_finite()
                || !page.ink_percent.is_finite()
            {
                return Err(ScanError::InvalidRequest(
                    "Invalid saved page settings".into(),
                ));
            }
            session.resolve(&page.original_path)?;
            session.resolve(&page.image_path)?;
        }
        Ok(session)
    }
    pub fn save(&self) -> Result<()> {
        atomic_json(&self.directory.join("session.json"), self, true)
    }
    /// Replace only the working image, keeping the acquisition original immutable.
    /// The expected path prevents an older review from overwriting a newer edit.
    pub fn apply_image(
        &mut self,
        page_id: &str,
        expected_image: &Path,
        image: &image::RgbImage,
        cancel: &CancellationToken,
    ) -> Result<()> {
        cancel.check()?;
        let index = self
            .pages
            .iter()
            .position(|p| p.id == page_id)
            .ok_or_else(|| ScanError::InvalidRequest("Page no longer exists".into()))?;
        if self.pages[index].image_path != expected_image {
            return Err(ScanError::InvalidRequest(
                "The page changed after this review was opened".into(),
            ));
        }
        let old = self.pages[index].clone();
        let output = PathBuf::from(format!("processed/{}.png", Uuid::new_v4()));
        processing::save_png_with_dpi(image, &self.directory.join(&output), old.dpi)?;
        cancel.check()?;
        self.pages[index].image_path = output;
        self.pages[index].ink_percent = processing::ink_coverage(image);
        if let Err(error) = self.save() {
            self.pages[index] = old;
            return Err(error);
        }
        Ok(())
    }
    /// Rotate the current working image, including any applied OCR clean copy.
    pub fn rotate_page(&mut self, index: usize, cancel: &CancellationToken) -> Result<()> {
        let old = self
            .pages
            .get(index)
            .cloned()
            .ok_or_else(|| ScanError::InvalidRequest("Page no longer exists".into()))?;
        cancel.check()?;
        let image = processing::load(&self.resolve(&old.image_path)?)?;
        let rotated = image::imageops::rotate90(&image);
        let output = PathBuf::from(format!("processed/{}.png", Uuid::new_v4()));
        processing::save_png_with_dpi(&rotated, &self.directory.join(&output), old.dpi)?;
        cancel.check()?;
        self.pages[index].image_path = output;
        self.pages[index].rotation = (old.rotation + 90) % 360;
        if let Err(error) = self.save() {
            self.pages[index] = old;
            return Err(error);
        }
        Ok(())
    }
    /// Remove a page from this session; retained files allow exact undo/recovery.
    pub fn delete_page(&mut self, index: usize) -> Result<()> {
        if index >= self.pages.len() {
            return Err(ScanError::InvalidRequest("Page no longer exists".into()));
        }
        let old = self.pages.remove(index);
        if let Err(error) = self.save() {
            self.pages.insert(index, old);
            return Err(error);
        }
        Ok(())
    }
    pub(crate) fn restore_pages(&mut self, pages: &[PageRecord]) -> Result<()> {
        for page in pages {
            self.resolve(&page.original_path)?;
            self.resolve(&page.image_path)?;
        }
        let previous = std::mem::replace(&mut self.pages, pages.to_vec());
        if let Err(error) = self.save() {
            self.pages = previous;
            return Err(error);
        }
        Ok(())
    }
    pub fn add(
        &mut self,
        source: &Path,
        profile: &ScanProfile,
        rotation: u16,
        cancel: &CancellationToken,
    ) -> Result<()> {
        profile.validate()?;
        cancel.check()?;
        if self.pages.len() >= 250 {
            return Err(ScanError::InvalidRequest(
                "Session has reached 250 pages".into(),
            ));
        }
        let original = processing::load(source)?;
        let result = processing::correct(&original, profile, rotation, cancel)?;
        let id = Uuid::new_v4().to_string();
        let raw = PathBuf::from(format!("originals/{id}.original"));
        let processed = PathBuf::from(format!("processed/{id}.png"));
        fs::copy(source, self.directory.join(&raw))?;
        processing::save_png_with_dpi(
            &result.image,
            &self.directory.join(&processed),
            profile.dpi,
        )?;
        self.pages.push(PageRecord {
            id,
            original_path: raw,
            image_path: processed,
            dpi: profile.dpi,
            deskew_angle: result.angle,
            ink_percent: result.ink_percent,
            excluded: profile.cleanup.skip_blank_pages && result.blank,
            rotation,
        });
        if let Err(error) = self.save() {
            self.pages.pop();
            return Err(error);
        }
        Ok(())
    }
    pub fn reprocess(
        &mut self,
        index: usize,
        profile: &ScanProfile,
        rotation: Option<u16>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let old = self
            .pages
            .get(index)
            .ok_or_else(|| ScanError::InvalidRequest("Page no longer exists".into()))?
            .clone();
        let mut profile = profile.clone();
        profile.dpi = old.dpi;
        let rotation = rotation.unwrap_or(old.rotation);
        let image = processing::load(&self.resolve(&old.original_path)?)?;
        let result = processing::correct(&image, &profile, rotation, cancel)?;
        let output = PathBuf::from(format!("processed/{}.png", Uuid::new_v4()));
        processing::save_png_with_dpi(&result.image, &self.directory.join(&output), profile.dpi)?;
        self.pages[index] = PageRecord {
            image_path: output,
            deskew_angle: result.angle,
            ink_percent: result.ink_percent,
            excluded: profile.cleanup.skip_blank_pages && result.blank,
            rotation,
            ..old.clone()
        };
        if let Err(error) = self.save() {
            self.pages[index] = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn move_page(&mut self, index: usize, next: usize) -> Result<()> {
        if index >= self.pages.len() || next >= self.pages.len() {
            return Err(ScanError::InvalidRequest("Page no longer exists".into()));
        }
        self.pages.swap(index, next);
        if let Err(error) = self.save() {
            self.pages.swap(index, next);
            return Err(error);
        }
        Ok(())
    }
    pub fn toggle(&mut self, index: usize) -> Result<()> {
        let page = self
            .pages
            .get_mut(index)
            .ok_or_else(|| ScanError::InvalidRequest("Page no longer exists".into()))?;
        page.excluded = !page.excluded;
        if let Err(error) = self.save() {
            self.pages[index].excluded = !self.pages[index].excluded;
            return Err(error);
        }
        Ok(())
    }
    pub fn acquire(
        &mut self,
        backend: &dyn ScannerBackend,
        profile: &ScanProfile,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(String),
    ) -> Result<AcquisitionResult> {
        profile.validate()?;
        cancel.check()?;
        if self.pages.len() >= 250 {
            return Err(ScanError::InvalidRequest(
                "Session has reached 250 pages".into(),
            ));
        }
        let mut profile = profile.clone();
        profile.max_pages = profile.max_pages.min(250 - self.pages.len() as u32);
        let incoming = self
            .directory
            .join("incoming")
            .join(Uuid::new_v4().to_string());
        let mut received = Vec::new();
        let result = backend.acquire(
            &ScanRequest {
                device_id: profile.device_id.clone(),
                output_dir: incoming.clone(),
                settings: (&profile).into(),
            },
            cancel,
            &mut |event| match event {
                crate::Progress::PageReady(path) => {
                    received.push(path);
                    progress(format!("Received page {}", received.len()));
                }
                crate::Progress::Connecting => progress("Connecting to scanner".into()),
                crate::Progress::Acquiring => {
                    progress("Scanning; cancellation waits for the driver".into())
                }
            },
        );
        let warning = match result {
            Ok(batch) => {
                if let Some(dpi) = batch.dpi {
                    profile.dpi = dpi;
                }
                for path in batch.pages {
                    if !received.contains(&path) {
                        received.push(path);
                    }
                }
                batch.warning.or_else(|| {
                    batch
                        .cancelled
                        .then(|| "Scanning stopped; completed pages were retained".into())
                })
            }
            Err(error) => Some(error.to_string()),
        };
        // Record receipt before corrections: a processing error must not hide acquired originals.
        atomic_json(
            &self.directory.join("last-acquisition.json"),
            &serde_json::json!({"profile":profile,"files":received,"warning":warning}),
            true,
        )?;
        let mut added = 0;
        // Finish preserving complete pages even if acquisition was cancelled. Raw files also remain.
        let preserve = CancellationToken::default();
        for (index, path) in received.iter().enumerate() {
            if !path.canonicalize()?.starts_with(incoming.canonicalize()?) {
                return Err(ScanError::InvalidRequest(
                    "Scanner returned a file outside its batch directory".into(),
                ));
            }
            progress(format!(
                "Preserving and correcting page {} of {}",
                index + 1,
                received.len()
            ));
            let rotation = if profile.source == ScanSource::Duplex
                && profile.flip_duplex_backs
                && index % 2 == 1
            {
                180
            } else {
                0
            };
            let mut page_profile = profile.clone();
            if profile.native_interface {
                page_profile.dpi = processing::resolution(path).unwrap_or(profile.dpi);
            }
            self.add(path, &page_profile, rotation, &preserve)?;
            added += 1;
        }
        if added == 0 {
            return Err(ScanError::Backend(
                warning.unwrap_or_else(|| "No pages received".into()),
            ));
        }
        Ok(AcquisitionResult {
            received: added,
            warning,
        })
    }
    pub fn export_images(&self, directory: &Path, cancel: &CancellationToken) -> Result<()> {
        if self.pages.iter().all(|p| p.excluded) {
            return Err(ScanError::InvalidRequest(
                "Include at least one page".into(),
            ));
        }
        // create_dir deliberately fails for existing output folders.
        fs::create_dir(directory)?;
        for (index, page) in self.pages.iter().filter(|p| !p.excluded).enumerate() {
            cancel.check()?;
            let image = processing::load(&self.resolve(&page.image_path)?)?;
            processing::save_png_with_dpi(
                &image,
                &directory.join(format!("page-{:04}.png", index + 1)),
                page.dpi,
            )?;
        }
        Ok(())
    }
    /// Expand each TIFF frame. Other supported formats preserve the exact input file.
    pub fn import_images(
        &mut self,
        paths: &[PathBuf],
        profile: &ScanProfile,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(String),
    ) -> Result<()> {
        profile.validate()?;
        for path in paths {
            cancel.check()?;
            if fs::metadata(path)?.len() > processing::MAX_FILE_BYTES {
                return Err(ScanError::InvalidRequest("Image exceeds 256 MB".into()));
            }
            let format = image::ImageReader::open(path)?
                .with_guessed_format()?
                .format();
            if format == Some(image::ImageFormat::Tiff) {
                let incoming = self
                    .directory
                    .join("incoming")
                    .join(Uuid::new_v4().to_string());
                fs::create_dir_all(&incoming)?;
                fs::copy(path, incoming.join("source.tiff"))?;
                let mut decoder =
                    tiff::decoder::Decoder::new(std::io::BufReader::new(fs::File::open(path)?))
                        .map_err(tiff_error)?;
                loop {
                    cancel.check()?;
                    if self.pages.len() >= 250 {
                        return Err(ScanError::InvalidRequest(
                            "Session has reached 250 pages".into(),
                        ));
                    }
                    let (width, height) = decoder.dimensions().map_err(tiff_error)?;
                    if width < 16
                        || height < 16
                        || u64::from(width) * u64::from(height) > processing::MAX_PIXELS
                    {
                        return Err(ScanError::InvalidRequest(
                            "TIFF frame exceeds page limits".into(),
                        ));
                    }
                    let color = decoder.colortype().map_err(tiff_error)?;
                    let samples = match decoder.read_image().map_err(tiff_error)? {
                        tiff::decoder::DecodingResult::U8(v) => v,
                        tiff::decoder::DecodingResult::U16(v) => {
                            v.into_iter().map(|v| (v >> 8) as u8).collect()
                        }
                        _ => {
                            return Err(ScanError::Unsupported(
                                "TIFF sample type is not supported; export PNG pages first".into(),
                            ));
                        }
                    };
                    let rgb=match color {
                        tiff::ColorType::Gray(8|16)=>image::GrayImage::from_raw(width,height,samples).map(|i|image::DynamicImage::ImageLuma8(i).to_rgb8()),
                        tiff::ColorType::RGB(8|16)=>image::RgbImage::from_raw(width,height,samples),
                        tiff::ColorType::RGBA(8|16)=>image::RgbaImage::from_raw(width,height,samples).map(|i|image::RgbImage::from_fn(width,height,|x,y|{let p=i.get_pixel(x,y);let a=u16::from(p[3]);image::Rgb(std::array::from_fn(|c|((u16::from(p[c])*a+255*(255-a))/255) as u8))})),
                        _=>return Err(ScanError::Unsupported("TIFF color format is unsupported; use 8/16-bit gray, RGB, RGBA or export PNG pages".into())),
                    }.ok_or_else(||ScanError::InvalidRequest("Invalid TIFF image buffer".into()))?;
                    let frame = incoming.join(format!("{}.png", Uuid::new_v4()));
                    processing::save_png(&rgb, &frame)?;
                    let mut frame_profile = profile.clone();
                    let unit = decoder
                        .get_tag_u32(tiff::tags::Tag::ResolutionUnit)
                        .unwrap_or(2);
                    if let Ok(resolution) = decoder.get_tag_f64(tiff::tags::Tag::XResolution) {
                        let dpi = resolution * if unit == 3 { 2.54 } else { 1.0 };
                        if unit != 1 && dpi.is_finite() && (75.0..=600.0).contains(&dpi.round()) {
                            frame_profile.dpi = dpi.round() as u32;
                        }
                    }
                    self.add(&frame, &frame_profile, 0, cancel)?;
                    progress(format!("Imported page {}", self.pages.len()));
                    if !decoder.more_images() {
                        break;
                    }
                    decoder.next_image().map_err(tiff_error)?;
                }
            } else {
                let mut page_profile = profile.clone();
                page_profile.dpi = processing::resolution(path).unwrap_or(profile.dpi);
                self.add(path, &page_profile, 0, cancel)?;
                progress(format!("Imported page {}", self.pages.len()));
            }
        }
        Ok(())
    }
}
fn tiff_error(error: tiff::TiffError) -> ScanError {
    ScanError::Backend(error.to_string())
}
