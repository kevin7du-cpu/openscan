//! Image correction in Rust, using image/imageproc. Originals are never overwritten.
use crate::*;
use image::{DynamicImage, GrayImage, ImageReader, Luma, Rgb, RgbImage, imageops};
use imageproc::geometric_transformations::{Interpolation, rotate_about_center};
use std::path::Path;

pub const MAX_PIXELS: u64 = 60_000_000;
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
pub fn read(path: &Path) -> Result<DynamicImage> {
    if std::fs::metadata(path)?.len() > MAX_BYTES {
        return Err(error("Image exceeds 256 MB."));
    }
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    let image = reader.decode()?;
    if image.width() < 16
        || image.height() < 16
        || u64::from(image.width()) * u64::from(image.height()) > MAX_PIXELS
    {
        return Err(error(
            "Image must be at least 16 × 16 and at most 60 million pixels.",
        ));
    }
    Ok(image)
}

#[derive(Debug)]
pub struct Corrected {
    pub image: DynamicImage,
    pub angle: f64,
    pub ink_percent: f64,
}

/// Projection profiles favor horizontal text rows, with a conservative improvement gate.
pub fn skew(gray: &GrayImage) -> f64 {
    let small = imageops::thumbnail(gray, 1200, 1600);
    let points: Vec<_> = small
        .enumerate_pixels()
        .filter(|(x, y, p)| {
            *x > 4 && *y > 4 && *x + 4 < small.width() && *y + 4 < small.height() && p.0[0] < 140
        })
        .map(|(x, y, _)| (x as f64, y as f64))
        .collect();
    if points.len() < 100
        || points.len() as f64 > 0.4 * f64::from(small.width()) * f64::from(small.height())
    {
        return 0.;
    }
    let stride = (points.len() / 40_000).max(1);
    let points: Vec<_> = points.into_iter().step_by(stride).collect();
    let score = |degrees: f64| {
        let radians = degrees.to_radians();
        let (s, c) = radians.sin_cos();
        let mut rows = vec![0u32; (small.height() + small.width() + 4) as usize];
        for (x, y) in &points {
            let row = (y * c - x * s + f64::from(small.width())).round() as usize;
            if let Some(count) = rows.get_mut(row) {
                *count += 1;
            }
        }
        rows.iter()
            .map(|v| u64::from(*v) * u64::from(*v))
            .sum::<u64>() as f64
    };
    let baseline = score(0.);
    let mut best = (baseline, 0.);
    for step in -24..=24 {
        let angle = f64::from(step) * 0.5;
        let s = score(angle);
        if s > best.0 {
            best = (s, angle);
        }
    }
    let coarse = best.1;
    for step in -10..=10 {
        let angle = coarse + f64::from(step) * 0.05;
        if angle.abs() > 12. {
            continue;
        }
        let s = score(angle);
        if s > best.0 {
            best = (s, angle);
        }
    }
    if best.0 > baseline * 1.025 && best.1.abs() >= 0.15 {
        best.1
    } else {
        0.
    }
}
pub fn process(
    image: DynamicImage,
    profile: &Profile,
    rotation: u16,
    cancel: &Cancellation,
) -> Result<Corrected> {
    profile.validate()?;
    cancel.check()?;
    if ![0, 90, 180, 270].contains(&rotation) {
        return Err(error("Invalid page rotation."));
    }
    let mut rgb = image.into_rgb8();
    let angle = if profile.cleanup.deskew {
        skew(&DynamicImage::ImageRgb8(rgb.clone()).into_luma8())
    } else {
        0.
    };
    if angle != 0. {
        let radians = angle.to_radians();
        let width = (f64::from(rgb.width()) * radians.cos().abs()
            + f64::from(rgb.height()) * radians.sin().abs())
        .ceil() as u32;
        let height = (f64::from(rgb.width()) * radians.sin().abs()
            + f64::from(rgb.height()) * radians.cos().abs())
        .ceil() as u32;
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(error("Deskew would exceed the 60 million pixel limit."));
        }
        let mut expanded = RgbImage::from_pixel(width, height, Rgb([255; 3]));
        imageops::overlay(
            &mut expanded,
            &rgb,
            i64::from((width - rgb.width()) / 2),
            i64::from((height - rgb.height()) / 2),
        );
        rgb = rotate_about_center(
            &expanded,
            -radians as f32,
            Interpolation::Bilinear,
            Rgb([255; 3]),
        );
    }
    cancel.check()?;
    rgb = match (profile.cleanup.rotate_degrees + rotation) % 360 {
        90 => imageops::rotate90(&rgb),
        180 => imageops::rotate180(&rgb),
        270 => imageops::rotate270(&rgb),
        _ => rgb,
    };
    if profile.cleanup.remove_background {
        let small = imageops::thumbnail(&rgb, 96, 96);
        let smooth = imageops::blur(&small, 5.);
        let background = imageops::resize(
            &smooth,
            rgb.width(),
            rgb.height(),
            imageops::FilterType::Triangle,
        );
        for (p, b) in rgb.pixels_mut().zip(background.pixels()) {
            for (channel, bg) in p.0.iter_mut().zip(b.0) {
                *channel = (f32::from(*channel) * 255. / f32::from(bg.max(1))).min(255.) as u8;
            }
        }
    }
    if profile.cleanup.denoise {
        rgb = imageproc::filter::median_filter(&rgb, 1, 1);
    }
    if profile.cleanup.sharpen {
        rgb = imageops::unsharpen(&rgb, 1., 3);
    }
    if profile.cleanup.contrast != 0 {
        rgb = imageops::contrast(&rgb, profile.cleanup.contrast as f32);
    }
    if profile.cleanup.brightness != 0 {
        rgb = imageops::brighten(
            &rgb,
            (profile.cleanup.brightness as f32 * 2.55).round() as i32,
        );
    }
    cancel.check()?;
    if profile.cleanup.auto_crop {
        let mut bounds = None::<(u32, u32, u32, u32)>;
        for (x, y, p) in rgb.enumerate_pixels() {
            if p.0.iter().all(|v| *v < 210) {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                });
            }
        }
        if let Some((left, top, right, bottom)) = bounds {
            let margin = profile.dpi / 16;
            let x = left.saturating_sub(margin);
            let y = top.saturating_sub(margin);
            let w = (right
                .saturating_add(margin)
                .saturating_add(1)
                .min(rgb.width()))
            .saturating_sub(x);
            let h = (bottom
                .saturating_add(margin)
                .saturating_add(1)
                .min(rgb.height()))
            .saturating_sub(y);
            if w >= 16 && h >= 16 {
                rgb = imageops::crop_imm(&rgb, x, y, w, h).to_image();
            }
        }
    }
    let gray = DynamicImage::ImageRgb8(rgb.clone()).into_luma8();
    let margin = (gray.width().min(gray.height()) / 100).max(1);
    let ink = gray
        .enumerate_pixels()
        .filter(|(x, y, p)| {
            *x >= margin
                && *y >= margin
                && *x < gray.width() - margin
                && *y < gray.height() - margin
                && p.0[0] < 210
        })
        .count();
    let ink_percent = 100. * ink as f64
        / (f64::from(gray.width() - 2 * margin) * f64::from(gray.height() - 2 * margin));
    let image = match profile.color {
        Color::Color => DynamicImage::ImageRgb8(rgb),
        Color::Grayscale => DynamicImage::ImageLuma8(gray),
        Color::BlackWhite => {
            let background = imageops::blur(&gray, 8.);
            let mut binary = gray;
            for (pixel, local) in binary.pixels_mut().zip(background.pixels()) {
                *pixel = Luma([if pixel.0[0].saturating_add(15) < local.0[0] {
                    0
                } else {
                    255
                }]);
            }
            DynamicImage::ImageLuma8(binary)
        }
    };
    Ok(Corrected {
        image,
        angle,
        ink_percent,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blank_photos_are_not_rotated_and_blackwhite_is_binary() {
        let white = RgbImage::from_pixel(200, 300, Rgb([255; 3]));
        let p = Profile {
            color: Color::BlackWhite,
            ..Profile::default()
        };
        let corrected = process(
            DynamicImage::ImageRgb8(white),
            &p,
            0,
            &Cancellation::default(),
        )
        .expect("correction");
        assert_eq!(corrected.angle, 0.);
        assert_eq!(corrected.ink_percent, 0.);
        assert!(
            corrected
                .image
                .into_luma8()
                .pixels()
                .all(|p| [0, 255].contains(&p.0[0]))
        );
    }
    #[test]
    fn skewed_rows_are_detected_before_quarter_turn() {
        let mut image = GrayImage::from_pixel(500, 700, Luma([255]));
        for row in (80..620).step_by(35) {
            for x in 50..440 {
                for dy in 0..4 {
                    image.put_pixel(x, row + dy, Luma([0]));
                }
            }
        }
        let tilted = rotate_about_center(
            &image,
            4f32.to_radians(),
            Interpolation::Bilinear,
            Luma([255]),
        );
        assert!((skew(&tilted) - 4.).abs() < 0.3);
        let corrected = process(
            DynamicImage::ImageLuma8(tilted),
            &Profile::default(),
            90,
            &Cancellation::default(),
        )
        .expect("correction");
        assert!((corrected.angle - 4.).abs() < 0.3);
        assert!(corrected.image.width() > corrected.image.height());
    }
}
