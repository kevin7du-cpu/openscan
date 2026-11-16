// SPDX-License-Identifier: MIT OR Apache-2.0
//! PaperDesk Soft 3D Light, implemented with native egui controls.
//!
//! Configure a local `Style`, then wrap that UI's contents in [`paint`]. Only
//! background shapes emitted by that scope are decorated. Text, images, icons,
//! widget IDs, hit rectangles, callbacks, and accessibility remain egui's own.

use egui::epaint::{RectShape, Shadow};
use egui::{Color32, Context, Id, Rect, Shape, Stroke, Style, TextureHandle, Ui, pos2};

/// Fine blue-gray outline, shared by fields, groups and separators.
pub const EDGE: Color32 = Color32::from_rgb(185, 198, 213);
/// Keyboard-focus and interaction outline.
pub const FOCUS: Color32 = Color32::from_rgb(22, 117, 200);
/// A near-white field fill, recognized by the scoped background decorator.
pub const FIELD_FILL: Color32 = Color32::from_rgb(255, 255, 254);
/// Blue selection fill, recognized by the scoped background decorator.
pub const CHECKED_FILL: Color32 = Color32::from_rgb(0, 103, 191);

const NORMAL: Color32 = Color32::from_rgb(248, 250, 252);
const HOVER: Color32 = Color32::from_rgb(238, 247, 255);
const PRESSED: Color32 = Color32::from_rgb(216, 231, 245);
const SELECTED: Color32 = Color32::from_rgb(239, 247, 255);

#[derive(Clone, Copy)]
enum Surface {
    Normal,
    Hover,
    Pressed,
    Selected,
    Field,
    Checked,
}

impl Surface {
    fn for_fill(fill: Color32) -> Option<Self> {
        match fill {
            NORMAL => Some(Self::Normal),
            HOVER => Some(Self::Hover),
            PRESSED => Some(Self::Pressed),
            SELECTED => Some(Self::Selected),
            FIELD_FILL => Some(Self::Field),
            CHECKED_FILL => Some(Self::Checked),
            _ => None,
        }
    }
    fn stops(self) -> [Color32; 3] {
        use Color32 as C;
        match self {
            Self::Normal => [C::WHITE, NORMAL, C::from_rgb(234, 240, 246)],
            Self::Hover => [C::WHITE, HOVER, C::from_rgb(217, 234, 249)],
            Self::Pressed => [
                PRESSED,
                C::from_rgb(228, 239, 250),
                C::from_rgb(237, 245, 252),
            ],
            Self::Selected => [
                SELECTED,
                C::from_rgb(228, 241, 255),
                C::from_rgb(215, 234, 255),
            ],
            Self::Field => [C::from_rgb(245, 248, 251), C::WHITE, C::WHITE],
            Self::Checked => [
                C::from_rgb(39, 134, 221),
                C::from_rgb(18, 119, 207),
                C::from_rgb(0, 103, 192),
            ],
        }
    }
}

#[derive(Clone)]
struct Surfaces {
    normal: TextureHandle,
    hover: TextureHandle,
    pressed: TextureHandle,
    selected: TextureHandle,
    field: TextureHandle,
    checked: TextureHandle,
}

impl Surfaces {
    fn get(ctx: &Context) -> Self {
        let id = Id::unique("OpenScan/Soft3DLight/1");
        if let Some(surfaces) = ctx.data(|data| data.get_temp::<Self>(id)) {
            return surfaces;
        }
        // Six tiny procedural textures are retained per context. Repainting
        // controls does not allocate or upload new textures every frame.
        let create = |surface: Surface, name: &str| {
            let stops = surface.stops();
            let pixels = (0..64u8)
                .map(|row| {
                    let t = f32::from(row) / 63.0;
                    let (a, b, amount) = if t <= 0.48 {
                        (stops[0], stops[1], t / 0.48)
                    } else {
                        (stops[1], stops[2], (t - 0.48) / 0.52)
                    };
                    a.lerp_to_gamma(b, amount)
                })
                .collect();
            ctx.load_texture(
                name,
                egui::ColorImage::new([1, 64], pixels),
                egui::TextureOptions::LINEAR,
            )
        };
        let surfaces = Self {
            normal: create(Surface::Normal, "Soft3D/normal"),
            hover: create(Surface::Hover, "Soft3D/hover"),
            pressed: create(Surface::Pressed, "Soft3D/pressed"),
            selected: create(Surface::Selected, "Soft3D/selected"),
            field: create(Surface::Field, "Soft3D/field"),
            checked: create(Surface::Checked, "Soft3D/checked"),
        };
        ctx.data_mut(|data| data.insert_temp(id, surfaces.clone()));
        surfaces
    }
    fn texture(&self, surface: Surface) -> egui::TextureId {
        match surface {
            Surface::Normal => self.normal.id(),
            Surface::Hover => self.hover.id(),
            Surface::Pressed => self.pressed.id(),
            Surface::Selected => self.selected.id(),
            Surface::Field => self.field.id(),
            Surface::Checked => self.checked.id(),
        }
    }
}

/// Apply the palette to a scanner-local style, preserving fonts and spacing.
/// The host's context style must not be changed when embedding the scanner.
pub fn configure(style: &mut Style) {
    let visuals = &mut style.visuals;
    visuals.panel_fill = Color32::WHITE;
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = FIELD_FILL;
    visuals.faint_bg_color = Color32::from_rgb(251, 252, 254);
    visuals.window_stroke = Stroke::new(1.0, EDGE);
    visuals.window_corner_radius = 8.into();
    visuals.window_shadow = popup_shadow();
    visuals.popup_shadow = popup_shadow();
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0, FOCUS);
    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, NORMAL),
        (&mut visuals.widgets.inactive, NORMAL),
        (&mut visuals.widgets.hovered, HOVER),
        (&mut visuals.widgets.active, PRESSED),
        (&mut visuals.widgets.open, SELECTED),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0, EDGE);
        widget.corner_radius = 5.into();
    }
    visuals.widgets.hovered.bg_stroke.color = FOCUS;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, FOCUS);
    visuals.widgets.open.bg_stroke.color = FOCUS;
}

fn popup_shadow() -> Shadow {
    Shadow {
        offset: [0, 5],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(40),
    }
}

/// White dialog frame with a thin edge and soft depth, without extra padding.
pub fn popup_frame() -> egui::Frame {
    egui::Frame::popup(&Style::default())
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, EDGE))
        .corner_radius(8)
        .shadow(popup_shadow())
}

/// Decorate only the native shapes generated by this invocation.
/// Call after setting the scanner's local style, including its approved font.
pub fn paint<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    let layer = ui.layer_id();
    let surfaces = Surfaces::get(&ctx);
    let first = ctx.graphics_mut(|graphics| graphics.entry(layer).next_idx());
    let result = contents(ui);
    ctx.graphics_mut(|graphics| {
        if let Some(list) = graphics.get_mut(layer) {
            let end = list.next_idx();
            for index in first.0..end.0 {
                list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                    decorate(&mut clipped.shape, &surfaces, 0);
                });
            }
        }
    });
    result
}

fn surface_shape(mut rectangle: RectShape, surface: Surface, textures: &Surfaces) -> Shape {
    let rect = rectangle.rect;
    let radius = rectangle.corner_radius;
    let mut shapes = Vec::with_capacity(4);
    if !matches!(surface, Surface::Field | Surface::Pressed) && rect.height() > 8.0 {
        let shadow = Shadow {
            offset: [0, 1],
            blur: 2,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(37, 60, 88, 31),
        };
        shapes.push(shadow.as_shape(rect, radius).into());
    }
    // egui 0.37 cannot tessellate an untextured stroke into a textured fill
    // mesh. Keep the native outline in its own primitive (and font-atlas mesh).
    let mut outline = rectangle.clone();
    outline.fill = Color32::TRANSPARENT;
    rectangle.stroke = Stroke::NONE;
    rectangle.fill = Color32::WHITE;
    rectangle = rectangle.with_texture(
        textures.texture(surface),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
    );
    shapes.push(rectangle.into());
    if !outline.stroke.is_empty() {
        shapes.push(outline.into());
    }
    // Highlights stay at the edges. Drawing a translucent filled rectangle
    // over an editor would wash out its text and native selection/caret.
    if rect.width() > 12.0 && rect.height() > 12.0 {
        let inset = matches!(surface, Surface::Field | Surface::Pressed);
        let color = if inset {
            Color32::from_rgba_unmultiplied(24, 52, 79, 24)
        } else {
            Color32::from_white_alpha(190)
        };
        shapes.push(Shape::line_segment(
            [
                pos2(rect.left() + 5.0, rect.top() + 1.5),
                pos2(rect.right() - 5.0, rect.top() + 1.5),
            ],
            Stroke::new(1.0, color),
        ));
    }
    Shape::Vec(shapes)
}

fn decorate(shape: &mut Shape, surfaces: &Surfaces, depth: u8) {
    // Native widget shape groups are shallow; bound recursion even when an
    // embedding application supplies a deeply nested custom shape.
    if depth >= 16 {
        return;
    }
    match shape {
        Shape::Vec(children) => {
            for child in children {
                decorate(child, surfaces, depth + 1);
            }
        }
        Shape::Rect(rectangle) if rectangle.brush.is_none() && rectangle.rect.is_finite() => {
            if let Some(surface) = Surface::for_fill(rectangle.fill) {
                *shape = surface_shape(rectangle.clone(), surface, surfaces);
            }
        }
        Shape::Circle(circle)
            if circle.radius.is_finite() && circle.radius > 0.0 && circle.radius <= 128.0 =>
        {
            if let Some(surface) = Surface::for_fill(circle.fill) {
                let rect =
                    Rect::from_center_size(circle.center, egui::Vec2::splat(circle.radius * 2.0));
                // The bounded native indicator radius converts safely to the
                // rounded-rectangle primitive used for textured surfaces.
                let rectangle = RectShape::new(
                    rect,
                    circle.radius.round() as u8,
                    circle.fill,
                    circle.stroke,
                    egui::StrokeKind::Middle,
                );
                *shape = surface_shape(rectangle, surface, surfaces);
            }
        }
        Shape::LineSegment { points, stroke } if stroke.color == EDGE => {
            let original = Shape::line_segment(*points, *stroke);
            let horizontal = (points[1].x - points[0].x).abs() >= (points[1].y - points[0].y).abs();
            let offset = if horizontal {
                egui::vec2(0.0, 1.0)
            } else {
                egui::vec2(1.0, 0.0)
            };
            *shape = Shape::Vec(vec![
                original,
                Shape::line_segment(
                    [points[0] + offset, points[1] + offset],
                    Stroke::new(1.0, Color32::WHITE),
                ),
            ]);
        }
        _ => {} // Preserve text, icon paths, user images, and custom drawing.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_theme_preserves_geometry_text_and_host_style() {
        let ctx = Context::default();
        let original = ctx.style_of(egui::Theme::Light);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        };
        let mut rectangles = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            for themed in [false, true] {
                ui.scope(|ui| {
                    let mut style = Style::default();
                    if themed {
                        configure(&mut style);
                    }
                    ui.set_style(style);
                    let mut contents = |ui: &mut Ui| {
                        let response = ui.button("Same native action");
                        rectangles.push(response.rect.size());
                    };
                    if themed {
                        paint(ui, contents);
                    } else {
                        contents(ui);
                    }
                });
            }
        });
        assert_eq!(rectangles[0], rectangles[1]);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.panel_fill,
            original.visuals.panel_fill
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).spacing.interact_size,
            original.spacing.interact_size
        );
        fn textured(shape: &Shape) -> bool {
            match shape {
                Shape::Rect(rect) => rect.brush.is_some(),
                Shape::Vec(items) => items.iter().any(textured),
                _ => false,
            }
        }
        assert!(output.shapes.iter().any(|s| textured(&s.shape)));
        assert_eq!(
            output
                .textures_delta
                .set
                .iter()
                .filter(|(id, _)| **id != egui::TextureId::default())
                .count(),
            6
        );
        output.textures_delta.clear();
        // Exercise the renderer's tessellation path, not only layout shapes.
        // A textured fill with an attached native stroke panics in egui 0.37.
        assert!(
            !ctx.tessellate(output.shapes.clone(), output.pixels_per_point)
                .is_empty()
        );
    }

    #[test]
    fn backgrounds_do_not_replace_icons_or_user_images() {
        let ctx = Context::default();
        let surfaces = Surfaces::get(&ctx);
        let mut icon = Shape::line_segment(
            [pos2(1.0, 4.0), pos2(4.0, 1.0)],
            Stroke::new(1.5, Color32::BLACK),
        );
        let original = icon.clone();
        decorate(&mut icon, &surfaces, 0);
        assert_eq!(icon, original);
        let mut image = Shape::Rect(
            RectShape::filled(
                Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(20.0, 20.0)),
                0,
                NORMAL,
            )
            .with_texture(
                egui::TextureId::User(42),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            ),
        );
        let original = image.clone();
        decorate(&mut image, &surfaces, 0);
        assert_eq!(image, original);
    }
}
