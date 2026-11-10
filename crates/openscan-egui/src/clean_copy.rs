use crate::{
    preview::Preview,
    worker::{CleanPreview, Command},
};

pub(crate) struct Review {
    data: CleanPreview,
    original: egui::TextureHandle,
    cleaned: egui::TextureHandle,
    preview: Preview,
    show_original: bool,
    dirty: bool,
    erase_mode: bool,
    erase_start: Option<egui::Pos2>,
}

impl Review {
    pub fn new(ctx: &egui::Context, mut data: CleanPreview) -> Self {
        let texture = |name: &str, image: &image::RgbaImage| {
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            )
        };
        let original = texture("clean-copy-original", &data.original);
        let cleaned = texture("clean-copy-reviewed", &data.cleaned);
        // The textures now own the previews; release the worker's RGBA buffers.
        data.original = image::RgbaImage::new(0, 0);
        data.cleaned = image::RgbaImage::new(0, 0);
        let mut preview = Preview::default();
        preview.set_source(data.draft.size);
        Self {
            data,
            original,
            cleaned,
            preview,
            show_original: false,
            dirty: false,
            erase_mode: false,
            erase_start: None,
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        busy: bool,
        error: Option<&str>,
    ) -> (Option<Command>, bool) {
        let mut command = None;
        let mut close = false;
        let mut open = true;
        egui::Window::new("OCR Clean").default_size(egui::vec2(1000.0,720.0))
            .min_size(egui::vec2(650.0,400.0)).collapsible(false).open(&mut open).enabled(!busy).show(ctx,|ui| {
            ui.horizontal_wrapped(|ui| {
                if crate::history_arrow(ui, !self.show_original, false, "Undo", "Undo cleanup: show the original scan. Redo brings back your edits.").clicked() {
                    self.show_original = true;
                    self.erase_start = None;
                }
                if crate::history_arrow(ui, self.show_original, true, "Redo", "Redo cleanup: show the latest edited preview.").clicked() {
                    self.show_original = false;
                }
                ui.separator();
                if ui.add_enabled(self.dirty,egui::Button::new("Preview changes")).on_hover_text("Refresh the page after changing cleanup options, text or selected marks.").clicked() {
                    command=Some(Command::RenderClean(self.data.draft.clone(),ctx.input(|i|i.max_texture_side).clamp(1,4096) as u32));
                }
                let apply_tip = if self.show_original { "Keep the original scan and close this window." } else { "Save the reviewed changes to this scan page and close. The main Undo arrow can restore the previous scan." };
                if ui.add_enabled(self.show_original || !self.dirty,egui::Button::new("Apply")).on_hover_text(apply_tip).clicked() {
                    if self.show_original {
                        // Undo selects the unchanged page. Never apply an edited
                        // image while the user is looking at the original.
                        close = true;
                    } else {
                        command=Some(Command::ApplyClean(self.data.draft.clone(),self.data.image_path.clone()));
                    }
                }
                ui.label(if self.show_original {"Original scan"} else {"Edited preview"});
                if busy {ui.spinner();ui.label("Processing…");}
            });
            ui.separator();
            ui.add_enabled_ui(!busy, |ui| { ui.horizontal_wrapped(|ui| {
                self.dirty |= ui.checkbox(&mut self.data.draft.options.remove_specks, "Remove isolated specks").changed();
                self.dirty |= ui.checkbox(&mut self.data.draft.options.reduce_shadows, "Reduce wrinkle shadows").changed();
            }); });
            ui.small("Content-aware cleanup keeps unrecognized areas and uncertain marks. Dark creases may remain.");
            ui.add_enabled_ui(!busy, |ui| { ui.horizontal_wrapped(|ui| {
                if ui.checkbox(&mut self.erase_mode,"Erase marks").changed() {
                    self.show_original = false;
                    self.erase_start = None;
                }
                if ui.add_enabled(!self.data.draft.erasures.is_empty(),egui::Button::new("Undo last mark")).clicked() {
                    self.data.draft.erasures.pop(); self.dirty = true;
                }
                if self.erase_mode { ui.label("Drag over a mark, then Preview changes. Everything inside the rectangle will be erased."); }
            }); });
            if let Some(error)=error {ui.colored_label(egui::Color32::DARK_RED,error);}
            if self.dirty && !self.show_original {ui.label("Preview changes to review your edits before Apply.");}
            ui.separator();
            egui::Panel::right("clean-copy-text-review").default_size(310.0).size_range(220.0..=450.0).resizable(true).show(ui,|ui| {
                ui.heading("Optional text replacement");
                ui.label("Leave unchecked to preserve the printed appearance. OCR can misread numbers, barcodes and handwriting.");
                egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui| {
                    ui.add_enabled_ui(!busy,|ui| {
                        for (index,line) in self.data.draft.lines.iter_mut().enumerate() {
                            ui.push_id(index,|ui| {
                                self.dirty |= ui.checkbox(&mut line.rebuild,format!("Replace region {}",index+1)).changed();
                                self.dirty |= ui.add(egui::TextEdit::singleline(&mut line.line.text).desired_width(f32::INFINITY))
                                    .accessible_name(format!("OCR region {} text",index+1)).changed();
                                if !line.rebuild {ui.small("Keep original appearance");}
                                ui.separator();
                            });
                        }
                    });
                });
            });
            egui::CentralPanel::default().show(ui,|ui| {
                egui::Panel::bottom("clean-copy-zoom").show(ui,|ui|self.preview.controls(ui,true));
                let selecting = self.erase_mode && !busy && !self.show_original;
                let response = self.preview.show_selectable(ui,if self.show_original {&self.original}else{&self.cleaned},selecting);
                let size = egui::vec2(self.data.draft.size[0] as f32,self.data.draft.size[1] as f32);
                if selecting {
                    if response.drag_started() {
                        self.erase_start = ui.input(|i|i.pointer.press_origin()).map(|p| source_position(response.rect,size,p));
                    }
                    if let (Some(start),Some(pointer)) = (self.erase_start,response.interact_pointer_pos()) {
                        let end = source_position(response.rect,size,pointer);
                        let selected = egui::Rect::from_two_pos(start,end);
                        paint_mark(ui,&response,selected,size);
                        if response.drag_stopped() {
                            if selected.width() >= 1.0 && selected.height() >= 1.0 && self.data.draft.erasures.len() < 500 {
                                self.data.draft.erasures.push([selected.min.x,selected.min.y,selected.width(),selected.height()]);
                                self.dirty = true;
                            }
                            self.erase_start = None;
                        }
                    }
                } else { self.erase_start = None; }
                if !self.show_original {
                    for &[x,y,w,h] in &self.data.draft.erasures {
                        paint_mark(ui,&response,egui::Rect::from_min_size(egui::pos2(x,y),egui::vec2(w,h)),size);
                    }
                }
            });
        });
        (command, close || !open)
    }
}

fn source_position(rect: egui::Rect, size: egui::Vec2, point: egui::Pos2) -> egui::Pos2 {
    let normalized = (point - rect.min) / rect.size();
    egui::pos2(
        normalized.x.clamp(0.0, 1.0) * size.x,
        normalized.y.clamp(0.0, 1.0) * size.y,
    )
}

fn paint_mark(ui: &egui::Ui, response: &egui::Response, area: egui::Rect, size: egui::Vec2) {
    let rect = egui::Rect::from_min_max(
        response.rect.min + area.min.to_vec2() / size * response.rect.size(),
        response.rect.min + area.max.to_vec2() / size * response.rect.size(),
    );
    ui.painter()
        .with_clip_rect(response.rect.intersect(ui.clip_rect()))
        .rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, egui::Color32::RED),
            egui::StrokeKind::Inside,
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use scan_module::documents::{TextLine, clean_copy::CleanLine};

    #[test]
    fn mark_coordinates_follow_zoom_and_scrolling_and_clamp_to_page() {
        let viewport = egui::Rect::from_min_size(egui::pos2(-200., 80.), egui::vec2(1200., 1600.));
        let source = egui::vec2(600., 800.);
        assert_eq!(
            source_position(viewport, source, egui::pos2(100., 480.)),
            egui::pos2(150., 200.)
        );
        assert_eq!(
            source_position(viewport, source, egui::pos2(-500., 3000.)),
            egui::pos2(0., 800.)
        );
    }

    fn fixture() -> CleanPreview {
        CleanPreview {
            draft: crate::worker::CleanDraft {
                page_id: "page-1".into(),
                source: "processed/source.png".into(),
                dpi: 300,
                size: [600, 800],
                options: Default::default(),
                erasures: vec![],
                lines: vec![CleanLine {
                    line: TextLine {
                        text: "TEST-4827".into(),
                        bounds: [50., 50., 300., 40.],
                    },
                    rebuild: true,
                }],
            },
            image_path: "clean-copies/draft.png".into(),
            original: image::RgbaImage::from_pixel(600, 800, image::Rgba([245, 245, 245, 255])),
            cleaned: image::RgbaImage::from_pixel(600, 800, image::Rgba([255, 255, 255, 255])),
        }
    }

    #[derive(Default)]
    struct ReviewState {
        review: Option<Review>,
        command: Option<Command>,
        close: bool,
        busy: bool,
    }

    fn harness() -> Harness<'static, ReviewState> {
        Harness::builder()
            .with_size(egui::vec2(1280.0, 900.0))
            .build_ui_state(
                |ui, state: &mut ReviewState| {
                    let review = state
                        .review
                        .get_or_insert_with(|| Review::new(ui.ctx(), fixture()));
                    let (command, close) = review.show(ui.ctx(), state.busy, None);
                    if command.is_some() {
                        state.command = command;
                    }
                    state.close |= close;
                },
                ReviewState::default(),
            )
    }

    #[test]
    fn changing_ocr_review_requires_a_new_preview_before_apply() {
        let mut h = harness();
        h.run();
        assert!(!h.get_by_label("Apply").accesskit_node().is_disabled());
        h.get_by_label("Replace region 1").click();
        h.run();
        assert!(h.get_by_label("Apply").accesskit_node().is_disabled());
        assert!(
            !h.get_by_label("Preview changes")
                .accesskit_node()
                .is_disabled()
        );
        h.get_by_label("Undo").click();
        h.run();
        assert!(h.state().review.as_ref().unwrap().show_original);
        assert!(!h.get_by_label("Apply").accesskit_node().is_disabled());
        h.get_by_label("Redo").click();
        h.run();
        assert!(h.get_by_label("Apply").accesskit_node().is_disabled());
        assert!(h.query_by_label("Keep original appearance").is_some());
        h.get_by_label("Erase marks").click();
        h.run();
        let canvas = h.get_by_label("Scanned page preview").rect();
        let start = canvas.min + canvas.size() * 0.25;
        let end = canvas.min + canvas.size() * 0.5;
        h.hover_at(start);
        h.run();
        h.drag_at(start);
        h.run();
        h.hover_at(end);
        h.run();
        h.drop_at(end);
        h.run();
        assert!(
            !h.get_by_label("Undo last mark")
                .accesskit_node()
                .is_disabled()
        );
        let review = h.state().review.as_ref().unwrap();
        assert_eq!(review.data.draft.erasures.len(), 1);
        let [x, y, w, height] = review.data.draft.erasures[0];
        assert!(
            (x - 150.).abs() < 1.
                && (y - 200.).abs() < 1.
                && (w - 150.).abs() < 1.
                && (height - 200.).abs() < 1.
        );
    }

    #[test]
    fn undo_redo_preview_apply_and_corner_close_have_distinct_effects() {
        let mut h = harness();
        h.run();
        assert!(h.query_by_label("Clean copy").is_none());
        assert!(h.query_by_label("Close clean copy").is_none());
        assert!(h.query_by_label("Update clean preview").is_none());
        assert!(!h.get_by_label("Undo").accesskit_node().is_disabled());
        assert!(h.get_by_label("Redo").accesskit_node().is_disabled());
        h.get_by_label("Undo").click();
        h.run();
        assert!(h.state().review.as_ref().unwrap().show_original);
        assert!(h.get_by_label("Undo").accesskit_node().is_disabled());
        h.get_by_label("Redo").click();
        h.run();
        assert!(!h.state().review.as_ref().unwrap().show_original);
        h.get_by_label("Apply").click();
        h.run();
        assert!(matches!(h.state().command, Some(Command::ApplyClean(..))));
        assert!(!h.state().close, "Only successful persistence closes Apply");

        let mut h = harness();
        h.run();
        h.get_by_label("Undo").click();
        h.run();
        h.get_by_label("Apply").click();
        h.run();
        assert!(h.state().close);
        assert!(
            h.state().command.is_none(),
            "Applying the original must not save the hidden edited image"
        );

        let mut h = harness();
        h.run();
        h.get_by_label("Remove isolated specks").click();
        h.run();
        let close_rect = h.get_by_label("Close window").rect();
        assert!(close_rect.min.x > h.get_by_label("Apply").rect().max.x);
        assert!(close_rect.max.y < h.get_by_label("Apply").rect().min.y);
        h.get_by_label("Close window").click();
        h.run();
        assert!(h.state().close);
        assert!(
            h.state().command.is_none(),
            "Closing a draft leaves the saved scan unchanged"
        );

        let mut h = harness();
        h.state_mut().busy = true;
        h.run_steps(3);
        assert!(
            h.get_by_label("Close window")
                .accesskit_node()
                .is_disabled()
        );
        assert!(h.get_by_label("Apply").accesskit_node().is_disabled());
    }

    #[test]
    #[ignore = "Requires a graphics adapter; set PAPERDESK_UI_CAPTURE to a new PNG path"]
    fn capture_cleanup_controls_for_visual_review() {
        let path = std::env::var_os("PAPERDESK_UI_CAPTURE").expect("PNG destination is required");
        assert!(
            !std::path::Path::new(&path).exists(),
            "Do not replace an existing capture"
        );
        let mut h = harness();
        h.ctx.set_visuals(egui::Visuals::light());
        // The fixture exercises replacement in interaction tests; the visual
        // example should show the production default of retaining source ink.
        h.state_mut().review.as_mut().unwrap().data.draft.lines[0].rebuild = false;
        h.run();
        h.render()
            .expect("Render the real egui controls")
            .save(path)
            .expect("Save UI capture");
    }
}
