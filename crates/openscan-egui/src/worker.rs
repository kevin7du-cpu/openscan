use scan_module::{
    CancellationToken, CleanupOptions, Device, DeviceCapabilities, ScanProfile, ScannerBackend,
    documents::pdf,
    presets::{CorrectionStore, PresetStore},
    session::ScanSession,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

pub(crate) enum Command {
    Devices,
    Capabilities(String),
    Scan(ScanProfile),
    Import(Vec<PathBuf>, ScanProfile),
    Correct(ScanProfile),
    SaveCorrections(CleanupOptions),
    Rotate(usize),
    Move(usize, usize),
    Delete(usize),
    Undo,
    Redo,
    Preview(usize, u32),
    PrepareClean(usize, u32),
    RenderClean(CleanDraft, u32),
    ApplyClean(CleanDraft, PathBuf),
    Open(PathBuf),
    New,
    Export {
        path: PathBuf,
        append: Option<PathBuf>,
        profile: ScanProfile,
    },
    Images(PathBuf),
    SavePreset(ScanProfile, bool),
    DeletePreset(String),
    RenamePreset(String, String),
}
pub(crate) enum Event {
    Session(ScanSession),
    Devices(Vec<Device>),
    Capabilities(DeviceCapabilities),
    Presets(Vec<ScanProfile>),
    Corrections {
        options: CleanupOptions,
        saved: bool,
    },
    Preview(usize, image::RgbaImage, [u32; 2]),
    CleanPreview(Box<CleanPreview>),
    CleanApplied(usize),
    History(Option<String>, Option<String>),
    Focus(usize),
    Progress(String),
    Message(String),
    Error(String),
    Exported(PathBuf),
    Done,
}

#[derive(Clone)]
pub(crate) struct CleanDraft {
    pub page_id: String,
    pub source: PathBuf,
    pub dpi: u32,
    pub size: [u32; 2],
    pub lines: Vec<scan_module::documents::clean_copy::CleanLine>,
    pub options: scan_module::documents::clean_copy::CleanOptions,
    pub erasures: Vec<[f32; 4]>,
}
pub(crate) struct CleanPreview {
    pub draft: CleanDraft,
    pub image_path: PathBuf,
    pub original: image::RgbaImage,
    pub cleaned: image::RgbaImage,
}
pub(crate) struct Worker {
    sender: SyncSender<Command>,
    pub events: Receiver<Event>,
    cancel: CancellationToken,
    flag: Arc<AtomicBool>,
}
impl Worker {
    pub fn start(root: PathBuf, models: PathBuf, backend: Box<dyn ScannerBackend>) -> Self {
        let (sender, commands) = mpsc::sync_channel(4);
        let (events_tx, events) = mpsc::channel();
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::from_shared_flag(flag.clone());
        let token = cancel.clone();
        thread::spawn(move || {
            let presets = PresetStore::new(root.join("Presets"));
            let corrections = CorrectionStore::new(&root);
            let create = || {
                ScanSession::create(root.join("Sessions").join(uuid::Uuid::new_v4().to_string()))
            };
            let mut session = match create() {
                Ok(s) => s,
                Err(e) => {
                    let _ = events_tx.send(Event::Error(e.to_string()));
                    let _ = events_tx.send(Event::Done);
                    return;
                }
            };
            let _ = events_tx.send(Event::Session(session.clone()));
            match corrections.load() {
                Ok(options) => {
                    let _ = events_tx.send(Event::Corrections {
                        options,
                        saved: false,
                    });
                }
                Err(e) => {
                    let _ = events_tx.send(Event::Error(format!(
                        "Could not load image correction settings: {e}"
                    )));
                }
            }
            match presets.list() {
                Ok(list) => {
                    let _ = events_tx.send(Event::Presets(list));
                }
                Err(e) => {
                    let _ = events_tx.send(Event::Error(e.to_string()));
                }
            }
            let _ = events_tx.send(Event::Done);
            let mut history = scan_module::history::ScanHistory::default();
            while let Ok(command) = commands.recv() {
                let before = session.clone();
                let label = match &command {
                    Command::Scan(_) => Some("Scan pages"),
                    Command::Import(..) => Some("Import pages"),
                    Command::Correct(_) => Some("Apply corrections"),
                    Command::Rotate(_) => Some("Rotate page"),
                    Command::Move(..) => Some("Reorder pages"),
                    Command::Delete(_) => Some("Delete page"),
                    Command::ApplyClean(..) => Some("Apply clean copy"),
                    _ => None,
                };
                let reset_history = matches!(&command, Command::Open(_) | Command::New);
                let mut progress = |message| {
                    let _ = events_tx.send(Event::Progress(message));
                };
                let result = (|| -> anyhow::Result<()> {
                    match command {
                        Command::Devices => {
                            events_tx.send(Event::Devices(backend.devices()?))?;
                        }
                        Command::Capabilities(id) => {
                            events_tx
                                .send(Event::Capabilities(backend.device_capabilities(&id)?))?;
                        }
                        Command::Scan(profile) => {
                            let result = session.acquire(
                                backend.as_ref(),
                                &profile,
                                &token,
                                &mut progress,
                            )?;
                            events_tx.send(Event::Message(result.warning.unwrap_or_else(
                                || {
                                    format!(
                                        "Received {} pages. Scan again to add more.",
                                        result.received
                                    )
                                },
                            )))?;
                        }
                        Command::Import(paths, profile) => {
                            session.import_images(&paths, &profile, &token, &mut progress)?;
                            events_tx.send(Event::Message(
                                "Images imported; review pages before export.".into(),
                            ))?;
                        }
                        Command::Correct(profile) => {
                            for index in 0..session.pages.len() {
                                token.check()?;
                                progress(format!("Applying corrections to page {}", index + 1));
                                session.reprocess(index, &profile, None, &token)?;
                            }
                            events_tx.send(Event::Message(
                                "Corrections reapplied from preserved originals.".into(),
                            ))?;
                        }
                        Command::SaveCorrections(options) => {
                            corrections.save(&options)?;
                            events_tx.send(Event::Corrections {
                                options,
                                saved: true,
                            })?;
                            events_tx.send(Event::Message(
                                "Image correction settings saved for future scans and imports."
                                    .into(),
                            ))?;
                        }
                        Command::Rotate(index) => session.rotate_page(index, &token)?,
                        Command::Move(index, next) => session.move_page(index, next)?,
                        Command::Delete(index) => session.delete_page(index)?,
                        Command::Undo | Command::Redo => {
                            let undo = matches!(command, Command::Undo);
                            let label = if undo {
                                history.undo_label()
                            } else {
                                history.redo_label()
                            }
                            .unwrap_or("change")
                            .to_owned();
                            if undo {
                                history.undo(&mut session)?;
                            } else {
                                history.redo(&mut session)?;
                            }
                            if let Some(index) = changed_page(&before, &session) {
                                events_tx.send(Event::Focus(index))?;
                            }
                            events_tx.send(Event::Message(format!(
                                "{}: {label}",
                                if undo { "Undid" } else { "Redid" }
                            )))?;
                        }
                        Command::Preview(index, max_side) => {
                            let page = session
                                .pages
                                .get(index)
                                .ok_or_else(|| anyhow::anyhow!("Page no longer exists"))?;
                            let image =
                                scan_module::processing::load(&session.resolve(&page.image_path)?)?;
                            let (image, original_size) = preview_image(image, max_side);
                            events_tx.send(Event::Preview(index, image, original_size))?;
                        }
                        Command::PrepareClean(index, max_side) => {
                            use scan_module::documents::{clean_copy, ocr};
                            let page = session
                                .pages
                                .get(index)
                                .ok_or_else(|| anyhow::anyhow!("Page no longer exists"))?;
                            progress("Recognizing text for a clean copy".into());
                            let image =
                                scan_module::processing::load(&session.resolve(&page.image_path)?)?;
                            token.check()?;
                            let lines =
                                ocr::session_words(&session, page, &models, &mut None, &token)?;
                            token.check()?;
                            let draft = CleanDraft {
                                page_id: page.id.clone(),
                                source: page.image_path.clone(),
                                dpi: page.dpi,
                                size: [image.width(), image.height()],
                                lines: clean_copy::prepare(&image, lines)?,
                                options: Default::default(),
                                erasures: vec![],
                            };
                            progress(
                                "Cleaning text areas while preserving unrecognized content".into(),
                            );
                            let preview = render_clean(&session, draft, max_side, &token)?;
                            events_tx.send(Event::CleanPreview(Box::new(preview)))?;
                        }
                        Command::RenderClean(draft, max_side) => {
                            let preview = render_clean(&session, draft, max_side, &token)?;
                            events_tx.send(Event::CleanPreview(Box::new(preview)))?;
                        }
                        Command::ApplyClean(draft, image_path) => {
                            let index = apply_clean(&mut session, &draft, &image_path, &token)?;
                            events_tx.send(Event::CleanApplied(index))?;
                            events_tx.send(Event::Message(
                                "Clean copy applied. Undo restores the previous scan.".into(),
                            ))?;
                        }
                        Command::Open(path) => {
                            session = ScanSession::load(&path)?;
                            events_tx.send(Event::Message("Saved session reopened.".into()))?;
                        }
                        Command::New => {
                            session = create()?;
                            events_tx.send(Event::Message(
                                "New session. The previous session remains saved on disk.".into(),
                            ))?;
                        }
                        Command::Export {
                            path,
                            append,
                            profile,
                        } => {
                            let bytes = pdf::export_session(
                                &session,
                                &profile,
                                &models,
                                &token,
                                &mut progress,
                            )?;
                            let bytes = if let Some(source) = append {
                                pdf::append_copy(&source, &bytes, &token)?
                            } else {
                                bytes
                            };
                            token.check()?;
                            pdf::save_new(&path, &bytes)?;
                            events_tx.send(Event::Message(format!("Saved {}", path.display())))?;
                            events_tx.send(Event::Exported(path))?;
                        }
                        Command::Images(path) => {
                            session.export_images(&path, &token)?;
                            events_tx.send(Event::Message(format!(
                                "Saved PNG pages to {}",
                                path.display()
                            )))?;
                        }
                        Command::SavePreset(profile, replace) => {
                            presets.save(&profile, replace)?;
                            events_tx.send(Event::Presets(presets.list()?))?;
                            events_tx.send(Event::Message("Preset saved.".into()))?;
                        }
                        Command::DeletePreset(name) => {
                            presets.delete(&name)?;
                            events_tx.send(Event::Presets(presets.list()?))?;
                        }
                        Command::RenamePreset(old, new) => {
                            presets.rename(&old, &new)?;
                            events_tx.send(Event::Presets(presets.list()?))?;
                        }
                    }
                    Ok(())
                })();
                if reset_history && result.is_ok() {
                    history.clear();
                }
                if let Some(label) = label {
                    history.record(&before, &session, label);
                }
                let _ = events_tx.send(Event::History(
                    history.undo_label().map(str::to_owned),
                    history.redo_label().map(str::to_owned),
                ));
                // Even failed/cancelled batches can contain durable, completed pages.
                let _ = events_tx.send(Event::Session(session.clone()));
                if let Err(error) = result {
                    let _ = events_tx.send(Event::Error(format!("{error:#}")));
                }
                let _ = events_tx.send(Event::Done);
            }
        });
        Self {
            sender,
            events,
            cancel,
            flag,
        }
    }
    pub fn submit(&self, command: Command) -> anyhow::Result<()> {
        self.flag.store(false, Ordering::Relaxed);
        self.sender
            .try_send(command)
            .map_err(|e| anyhow::anyhow!("Scanner worker unavailable: {e}"))
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn changed_page(before: &ScanSession, after: &ScanSession) -> Option<usize> {
    after
        .pages
        .iter()
        .position(|page| !before.pages.iter().any(|p| p.id == page.id))
        .or_else(|| {
            after
                .pages
                .iter()
                .position(|page| before.pages.iter().any(|p| p.id == page.id && p != page))
        })
        .or_else(|| {
            after
                .pages
                .iter()
                .enumerate()
                .position(|(i, page)| before.pages.get(i).is_some_and(|p| p.id != page.id))
        })
}

fn apply_clean(
    session: &mut ScanSession,
    draft: &CleanDraft,
    image_path: &std::path::Path,
    cancel: &CancellationToken,
) -> anyhow::Result<usize> {
    let index = session
        .pages
        .iter()
        .position(|p| p.id == draft.page_id)
        .ok_or_else(|| anyhow::anyhow!("The reviewed page no longer exists"))?;
    anyhow::ensure!(
        session.pages[index].dpi == draft.dpi,
        "The page resolution changed after review"
    );
    let image = scan_module::processing::load(&session.resolve(image_path)?)?;
    anyhow::ensure!(
        [image.width(), image.height()] == draft.size,
        "Clean preview dimensions changed"
    );
    session.apply_image(&draft.page_id, &draft.source, &image, cancel)?;
    Ok(index)
}

fn preview_image(image: image::RgbImage, max_side: u32) -> (image::RgbaImage, [u32; 2]) {
    let original_size = [image.width(), image.height()];
    let max_side = max_side.clamp(1, 4096);
    let image = image::DynamicImage::ImageRgb8(image);
    // A normal 300 DPI page retains every pixel when supported by the renderer.
    // Larger pages respect GPU limits and a 64 MiB RGBA texture budget.
    let image = if original_size.into_iter().any(|side| side > max_side) {
        image.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    (image.to_rgba8(), original_size)
}

fn render_clean(
    session: &ScanSession,
    draft: CleanDraft,
    max_side: u32,
    cancel: &CancellationToken,
) -> anyhow::Result<CleanPreview> {
    let original = scan_module::processing::load(&session.resolve(&draft.source)?)?;
    anyhow::ensure!(
        [original.width(), original.height()] == draft.size,
        "Source page dimensions changed"
    );
    let mut cleaned = scan_module::documents::clean_copy::render_with_options_at_dpi(
        &original,
        &draft.lines,
        draft.options,
        draft.dpi,
        cancel,
    )?;
    scan_module::documents::clean_copy::erase_marks(&mut cleaned, &draft.erasures, cancel)?;
    let directory = session.directory.join("clean-copies");
    std::fs::create_dir_all(&directory)?;
    let id = uuid::Uuid::new_v4();
    let image_path = PathBuf::from(format!("clean-copies/{id}.png"));
    scan_module::processing::save_png_with_dpi(
        &cleaned,
        &session.directory.join(&image_path),
        draft.dpi,
    )?;
    let (original, _) = preview_image(original, max_side);
    let (cleaned, _) = preview_image(cleaned, max_side);
    Ok(CleanPreview {
        draft,
        image_path,
        original,
        cleaned,
    })
}

#[cfg(test)]
mod tests {
    use super::preview_image;

    #[test]
    fn preview_preserves_scan_detail_and_respects_renderer_limit() {
        let source = image::RgbImage::from_fn(2550, 3300, |x, y| {
            image::Rgb([((x + y) % 256) as u8, 127, 255])
        });
        let (full, dimensions) = preview_image(source.clone(), 4096);
        assert_eq!(dimensions, [2550, 3300]);
        assert_eq!(full.dimensions(), (2550, 3300));
        assert_eq!(
            full.get_pixel(2137, 3021)[0],
            source.get_pixel(2137, 3021)[0]
        );
        let (bounded, dimensions) = preview_image(source, 1024);
        assert_eq!(dimensions, [2550, 3300]);
        assert_eq!(bounded.height(), 1024);
        assert!(bounded.width() <= 1024);
        let (small, _) = preview_image(image::RgbImage::new(80, 120), 4096);
        assert_eq!(small.dimensions(), (80, 120));
    }
}
