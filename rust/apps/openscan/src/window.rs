use craft_dialogs::{Answer, Dialogs, FileDialog};
use openscan_core::*;
use openscan_ui::{Action, ScanPanel};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

enum Update {
    Devices(Vec<Device>, Vec<String>),
    NetworkDevices(Vec<Device>),
    Caps(Capabilities),
    Presets(Vec<Profile>),
    Session(Session, Option<String>),
    Preview(PathBuf, egui::ColorImage),
    Saved(PathBuf),
    Images,
}
#[derive(Clone)]
enum Then {
    Import,
    Save,
    Images,
    AppendSource,
    AppendDestination(PathBuf),
}
struct App {
    panel: ScanPanel,
    receiver: Option<mpsc::Receiver<Result<Update>>>,
    cancel: Cancellation,
    dialogs: Dialogs<Then>,
    preview_path: Option<PathBuf>,
    result: Arc<Mutex<Option<PathBuf>>>,
    return_json: bool,
}
pub fn run(
    presets: bool,
    return_json: bool,
    session: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let initial = match session {
        Some(path) => Some(Session::load(&path)?),
        None => None,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040., 820.])
            .with_min_inner_size([480., 450.]),
        ..Default::default()
    };
    eframe::run_native(
        "OpenScan",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let mut app = App {
                panel: ScanPanel::new(presets, initial),
                receiver: None,
                cancel: Cancellation::default(),
                dialogs: Dialogs::new(Arc::new(move || ctx.request_repaint())),
                preview_path: None,
                result: output,
                return_json,
            };
            match PresetStore::new(None).and_then(|p| p.list()) {
                Ok(p) => app.panel.presets = p,
                Err(e) => app.panel.error = Some(e.to_string()),
            }
            app.start(&cc.egui_ctx, "Discovering scanners…", |c| {
                let (d, w) = backend::devices(c);
                Ok(Update::Devices(d, w))
            });
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| error(e.to_string()))?;
    let path = result
        .lock()
        .map_err(|_| error("Scanner result could not be read."))?
        .clone();
    Ok(serde_json::json!({"ok":true,"protocolVersion":1,"cancelled":path.is_none(),"path":path}))
}
impl App {
    fn start(
        &mut self,
        ctx: &egui::Context,
        status: &str,
        task: impl FnOnce(&Cancellation) -> Result<Update> + Send + 'static,
    ) {
        if self.receiver.is_some() {
            return;
        }
        self.cancel = Cancellation::default();
        let cancel = self.cancel.clone();
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new().name("openscan-worker".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||task(&cancel))).unwrap_or_else(|_|Err(error("The operation stopped unexpectedly. Original images remain in the scan session.")));
            let _=send.send(result);ctx.request_repaint();
        }){Ok(_)=>{self.receiver=Some(receive);self.panel.busy=true;self.panel.status=status.into();self.panel.error=None;},Err(e)=>self.panel.error=Some(e.to_string())}
    }
    fn session_task(
        &mut self,
        ctx: &egui::Context,
        status: &str,
        task: impl FnOnce(&mut Session, &Profile, &Cancellation) -> Result<()> + Send + 'static,
    ) {
        let Some(mut session) = self.panel.session.clone() else {
            return;
        };
        let profile = self.panel.profile.clone();
        self.start(ctx, status, move |cancel| {
            let result = task(&mut session, &profile, cancel);
            Ok(Update::Session(
                session,
                result.err().map(|e| e.to_string()),
            ))
        });
    }
    fn action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Refresh => self.start(ctx, "Discovering scanners…", |c| {
                let (d, w) = backend::devices(c);
                Ok(Update::Devices(d, w))
            }),
            Action::SearchNetwork => self.start(ctx, "Searching network scanners…", |c| {
                Ok(Update::NetworkDevices(backend::network_devices(c)?))
            }),
            Action::SelectDevice(id) => {
                self.panel.profile.device_id = Some(id.clone());
                self.start(ctx, "Reading scanner capabilities…", move |_| {
                    Ok(Update::Caps(backend::for_device(&id)?.capabilities(&id)?))
                });
            }
            Action::Scan => {
                let session = self.panel.session.clone();
                let profile = self.panel.profile.clone();
                self.start(ctx, "Scanning…", move |c| {
                    let session = match session {
                        Some(s) => s,
                        None => Session::new(None)?,
                    };
                    let (s, w) = crate::acquire(session, &profile, c);
                    Ok(Update::Session(s, w))
                });
            }
            Action::Import => self.dialogs.ask(
                FileDialog::open_many()
                    .title("Import scanned images")
                    .filter(
                        "Images",
                        &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "pnm"],
                    ),
                Then::Import,
            ),
            Action::SavePdf => self.dialogs.ask(
                FileDialog::save()
                    .title("Save scanned PDF")
                    .filter("PDF", &["pdf"])
                    .file_name("Scanned document.pdf"),
                Then::Save,
            ),
            Action::AppendPdf => self.dialogs.ask(
                FileDialog::open()
                    .title("Choose PDF to append to")
                    .filter("PDF", &["pdf"]),
                Then::AppendSource,
            ),
            Action::SaveImages => self.dialogs.ask(
                FileDialog::folder().title("Choose a parent folder for scanned images"),
                Then::Images,
            ),
            Action::LoadPreset(index) => {
                if let Some(p) = self.panel.presets.get(index) {
                    self.panel.profile = p.clone();
                    self.panel.selected_preset = Some(index);
                    self.panel.capabilities = None;
                }
            }
            Action::SavePreset(overwrite) => {
                let profile = self.panel.profile.clone();
                self.start(ctx, "Saving preset…", move |_| {
                    let store = PresetStore::new(None)?;
                    store.save(&profile, overwrite)?;
                    Ok(Update::Presets(store.list()?))
                });
            }
            Action::DeletePreset => {
                let name = self
                    .panel
                    .selected_preset
                    .and_then(|i| self.panel.presets.get(i))
                    .map(|p| p.name.clone());
                if let Some(name) = name {
                    self.start(ctx, "Deleting preset…", move |_| {
                        let store = PresetStore::new(None)?;
                        store.delete(&name)?;
                        Ok(Update::Presets(store.list()?))
                    });
                }
            }
            Action::Rotate(degrees) => {
                let i = self.panel.selected_page;
                self.session_task(ctx, "Rotating page…", move |s, p, c| {
                    let old = s
                        .pages
                        .get(i)
                        .ok_or_else(|| error("Page no longer exists."))?
                        .rotation;
                    let rotation = (i32::from(old) + i32::from(degrees)).rem_euclid(360) as u16;
                    s.reprocess(i, p, rotation, c)
                });
            }
            Action::Reprocess => {
                let i = self.panel.selected_page;
                self.session_task(ctx, "Applying corrections…", move |s, p, c| {
                    let rotation = s
                        .pages
                        .get(i)
                        .ok_or_else(|| error("Page no longer exists."))?
                        .rotation;
                    s.reprocess(i, p, rotation, c)
                });
            }
            Action::Move(direction) => {
                if let Some(s) = &mut self.panel.session {
                    let i = self.panel.selected_page;
                    let target = (i as i64 + i64::from(direction))
                        .clamp(0, s.pages.len().saturating_sub(1) as i64)
                        as usize;
                    if i < s.pages.len() {
                        s.pages.swap(i, target);
                        self.panel.selected_page = target;
                        if let Err(e) = s.save() {
                            self.panel.error = Some(e.to_string());
                        }
                    }
                }
            }
            Action::TogglePage => {
                if let Some(s) = &mut self.panel.session {
                    if let Some(p) = s.pages.get_mut(self.panel.selected_page) {
                        p.excluded = !p.excluded;
                    }
                    if let Err(e) = s.save() {
                        self.panel.error = Some(e.to_string());
                    }
                }
            }
            Action::Cancel => {
                self.cancel.cancel();
                self.panel.status = "Cancelling; keeping received pages…".into();
            }
            Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
    fn answer(&mut self, ctx: &egui::Context, then: Then, paths: Vec<PathBuf>) {
        match then {
            Then::Import => {
                let session = self.panel.session.clone();
                let profile = self.panel.profile.clone();
                self.start(ctx, "Importing images…", move |c| {
                    let mut s = match session {
                        Some(s) => s,
                        None => Session::new(None)?,
                    };
                    let result = (|| {
                        if paths.len() + s.pages.len() > 250 {
                            return Err(error("A session can hold 250 pages."));
                        }
                        for path in paths {
                            s.import(&path, &profile, c)?;
                        }
                        Ok(())
                    })();
                    Ok(Update::Session(
                        s,
                        result.err().map(|e: Error| e.to_string()),
                    ))
                });
            }
            Then::Save => {
                let Some(path) = paths.first().cloned() else {
                    return;
                };
                let Some(session) = self.panel.session.clone() else {
                    return;
                };
                let profile = self.panel.profile.clone();
                self.start(ctx, "Saving PDF…", move |c| {
                    pdf::export(&session.pages, &path, &profile, false, c)?;
                    Ok(Update::Saved(path))
                });
            }
            Then::Images => {
                let Some(parent) = paths.first().cloned() else {
                    return;
                };
                let Some(session) = self.panel.session.clone() else {
                    return;
                };
                self.start(ctx, "Saving page images…", move |c| {
                    let directory = parent.join(
                        session
                            .directory
                            .file_name()
                            .ok_or_else(|| error("Invalid session name."))?,
                    );
                    std::fs::create_dir(&directory)?;
                    for (i, page) in session.pages.iter().filter(|p| !p.excluded).enumerate() {
                        c.check()?;
                        let path = directory.join(format!("page-{:04}.png", i + 1));
                        let image = correction::read(&page.image_path)?;
                        let mut bytes = std::io::Cursor::new(Vec::new());
                        image.write_to(&mut bytes, image::ImageFormat::Png)?;
                        storage::atomic_write(&path, &bytes.into_inner(), false)?;
                    }
                    Ok(Update::Images)
                });
            }
            Then::AppendSource => {
                if let Some(path) = paths.first() {
                    self.dialogs.ask(
                        FileDialog::save()
                            .title("Save appended PDF as a new file")
                            .filter("PDF", &["pdf"])
                            .file_name("Appended scan.pdf"),
                        Then::AppendDestination(path.clone()),
                    );
                }
            }
            Then::AppendDestination(source) => {
                let Some(path) = paths.first().cloned() else {
                    return;
                };
                let Some(session) = self.panel.session.clone() else {
                    return;
                };
                let profile = self.panel.profile.clone();
                self.start(ctx, "Appending scanned pages…", move |c| {
                    pdf::append(&source, &session.pages, &path, &profile, c)?;
                    Ok(Update::Saved(path))
                });
            }
        }
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.panel.busy && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.cancel.cancel();
            self.panel.status = "Cancelling before closing. Received pages will be kept.".into();
        }
        let update = self.receiver.as_ref().and_then(|r| match r.try_recv() {
            Ok(u) => Some(u),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(error("Scanner worker disconnected.")))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(update) = update {
            self.receiver = None;
            self.panel.busy = false;
            self.panel.status = "Ready".into();
            match update {
                Err(e) => self.panel.error = Some(e.to_string()),
                Ok(Update::Devices(d, w)) => {
                    self.panel
                        .devices
                        .retain(|device| device.id.starts_with("escl:"));
                    self.panel.devices.extend(d);
                    self.panel.error = if w.is_empty() {
                        None
                    } else {
                        Some(w.join("\n"))
                    };
                    if self.panel.devices.is_empty() {
                        self.panel.status =
                            "No scanners found. Connect a scanner or add its network address."
                                .into();
                    }
                }
                Ok(Update::NetworkDevices(devices)) => {
                    for device in devices {
                        if !self
                            .panel
                            .devices
                            .iter()
                            .any(|existing| existing.id == device.id)
                        {
                            self.panel.devices.push(device);
                        }
                    }
                    self.panel.status = "Network search finished.".into();
                }
                Ok(Update::Caps(c)) => {
                    if !self.panel.devices.iter().any(|d| d.id == c.device_id) {
                        self.panel.devices.push(Device {
                            id: c.device_id.clone(),
                            name: "Network scanner".into(),
                            driver: "AirScan".into(),
                        });
                    }
                    if !c
                        .sources
                        .iter()
                        .any(|s| s.source == self.panel.profile.source)
                        && let Some(s) = c.sources.first()
                    {
                        self.panel.profile.source = s.source;
                    }
                    self.panel.capabilities = Some(c);
                }
                Ok(Update::Presets(p)) => {
                    self.panel.presets = p;
                    self.panel.selected_preset = self
                        .panel
                        .presets
                        .iter()
                        .position(|p| p.name == self.panel.profile.name);
                }
                Ok(Update::Session(s, w)) => {
                    self.panel.session = Some(s);
                    self.panel.error = w;
                    self.preview_path = None;
                }
                Ok(Update::Preview(path, image)) => {
                    self.preview_path = Some(path);
                    self.panel.preview =
                        Some(ctx.load_texture("scan-preview", image, egui::TextureOptions::LINEAR));
                }
                Ok(Update::Saved(path)) => {
                    self.panel.status = format!("Saved {}", path.display());
                    if let Ok(mut result) = self.result.lock() {
                        *result = Some(path);
                    }
                    if self.return_json {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Ok(Update::Images) => self.panel.status = "Page images saved.".into(),
            }
        }
        if !self.panel.busy {
            let path = self
                .panel
                .session
                .as_ref()
                .and_then(|s| s.pages.get(self.panel.selected_page))
                .map(|p| p.image_path.clone());
            if let Some(path) = path
                && self.preview_path.as_ref() != Some(&path)
            {
                // A failed decode must not relaunch every frame and make the UI unusable.
                self.preview_path = Some(path.clone());
                self.panel.preview = None;
                let previous_error = self.panel.error.take();
                self.start(ctx, "Loading preview…", move |_| {
                    let image = correction::read(&path)?.thumbnail(1200, 1600).into_rgba8();
                    let size = [image.width() as usize, image.height() as usize];
                    Ok(Update::Preview(
                        path,
                        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                    ))
                });
                // Loading a thumbnail must not hide a jam/partial-acquisition warning.
                self.panel.error = self.panel.error.take().or(previous_error);
            }
        }
        for (then, answer) in self.dialogs.take_answers() {
            match answer {
                Answer::Paths(paths) => self.answer(ctx, then, paths),
                Answer::Unavailable(e) => self.panel.error = Some(e),
                _ => {}
            }
        }
        self.dialogs.show_pending(Some(frame));
        if self.panel.busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            openscan_ui::dialog_style(ui);
            let actions = self.panel.show(ui);
            for action in actions {
                self.action(&ctx, action);
            }
        });
    }
}
