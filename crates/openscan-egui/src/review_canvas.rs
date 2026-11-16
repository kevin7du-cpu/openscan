//! Full-resolution review pixels are prepared off the GUI thread.
use egui::{ColorImage, TextureHandle, Ui};
use openscan_core::cleanup::Page;
use std::sync::{Arc, mpsc};

#[derive(Default)]
pub(crate) struct ReviewCanvas {
    view: crate::preview::Preview,
    source: Option<Arc<image::DynamicImage>>,
    texture: Option<TextureHandle>,
    pending: Option<(Arc<image::DynamicImage>, mpsc::Receiver<ColorImage>)>,
    warning: Option<String>,
}

impl ReviewCanvas {
    pub fn show(&mut self, ui: &mut Ui, page: &Page, thumbnail: &TextureHandle) {
        if !self
            .source
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, &page.image))
        {
            self.source = Some(page.image.clone());
            self.texture = None;
            self.warning = None;
            self.view
                .set_source([page.image.width(), page.image.height()]);
        }
        if let Some((source, receiver)) = &self.pending {
            match receiver.try_recv() {
                Ok(image) => {
                    if Arc::ptr_eq(source, &page.image) {
                        self.texture = Some(ui.ctx().load_texture(
                            "scan-quality-review",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.warning =
                        Some("Full-resolution preview unavailable; showing thumbnail.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && self.texture.is_none() && self.warning.is_none() {
            let max_side = ui.input(|i| i.max_texture_side).clamp(1, 8192) as u32;
            if page.image.width().max(page.image.height()) > max_side {
                self.warning = Some(
                    "Review image reduced to fit graphics limits; exported pixels are unchanged."
                        .into(),
                );
            }
            let source = page.image.clone();
            let image = source.clone();
            let ctx = ui.ctx().clone();
            let (sender, receiver) = mpsc::channel();
            match std::thread::Builder::new()
                .name("openscan-quality-preview".into())
                .spawn(move || {
                    let pixels = if image.width().max(image.height()) > max_side {
                        image.thumbnail(max_side, max_side).to_rgb8()
                    } else {
                        image.to_rgb8()
                    };
                    let preview = ColorImage::from_rgb(
                        [pixels.width() as usize, pixels.height() as usize],
                        pixels.as_raw(),
                    );
                    let _ = sender.send(preview);
                    ctx.request_repaint();
                }) {
                Ok(_) => self.pending = Some((source, receiver)),
                Err(error) => {
                    self.warning = Some(format!("Could not prepare detailed preview: {error}"))
                }
            }
        }
        self.view.controls(ui, true);
        if let Some(warning) = &self.warning {
            ui.label(warning);
        } else if self.pending.is_some() {
            ui.label("Loading full-resolution preview…");
        }
        self.view
            .show(ui, self.texture.as_ref().unwrap_or(thumbnail));
        if self.pending.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_preview_keeps_source_resolution_instead_of_enlarging_a_thumbnail() {
        let ctx = egui::Context::default();
        let page = Page::from_image(
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                1800,
                2200,
                image::Rgb([250; 3]),
            )),
            300,
        );
        assert!(page.preview.width() < page.image.width());
        let thumb = ctx.load_texture(
            "test-thumb",
            ColorImage::from_rgb(
                [
                    page.preview.width() as usize,
                    page.preview.height() as usize,
                ],
                page.preview.as_raw(),
            ),
            Default::default(),
        );
        let mut review = ReviewCanvas::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| review.show(ui, &page, &thumb));
            output.textures_delta.clear();
            if let Some(texture) = &review.texture {
                assert_eq!(texture.size(), [1800, 2200]);
                break;
            }
            assert!(review.warning.is_none(), "{:?}", review.warning);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
