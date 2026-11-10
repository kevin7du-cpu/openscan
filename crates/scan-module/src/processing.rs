//! Rust image corrections, derived from OpenScan's pipeline (MIT).
//! The input is never modified. Algorithms need not be pixel-identical to OpenCV.
mod despeckle;
mod sauvola;
use crate::{CancellationToken, Result, ScanColor, ScanError, ScanProfile};
use image::{DynamicImage, Rgb, RgbImage};
use imageproc::geometric_transformations::{Border, Interpolation, rotate_about_center_no_crop};
pub use sauvola::balanced_black_white;
use std::{io::Cursor, path::Path};

pub const MAX_PIXELS: u64 = 60_000_000;
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub struct CorrectedPage {
    pub image: RgbImage,
    pub angle: f32,
    pub ink_percent: f64,
    pub blank: bool,
}
fn image_error(e: image::ImageError) -> ScanError {
    ScanError::Backend(e.to_string())
}
pub fn load(path: &Path) -> Result<RgbImage> {
    if std::fs::metadata(path)?.len() > MAX_FILE_BYTES {
        return Err(ScanError::InvalidRequest("Image exceeds 256 MB".into()));
    }
    decode(&std::fs::read(path)?)
}
pub fn decode(bytes: &[u8]) -> Result<RgbImage> {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ScanError::InvalidRequest("Image exceeds 256 MB".into()));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().map_err(image_error)?.to_rgba8();
    if rgba.width() < 16
        || rgba.height() < 16
        || u64::from(rgba.width()) * u64::from(rgba.height()) > MAX_PIXELS
    {
        return Err(ScanError::InvalidRequest(
            "Page must be at least 16 × 16 and at most 60 megapixels".into(),
        ));
    }
    Ok(RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let a = u16::from(p[3]);
        Rgb(std::array::from_fn(|i| {
            ((u16::from(p[i]) * a + 255 * (255 - a)) / 255) as u8
        }))
    }))
}
pub fn save_png(image: &RgbImage, path: &Path) -> Result<()> {
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(image_error)
}
pub fn save_png_with_dpi(image: &RgbImage, path: &Path, dpi: u32) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder =
        png::Encoder::new(std::io::BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let per_meter = (f64::from(dpi) / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: per_meter,
        yppu: per_meter,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder
        .write_header()
        .map_err(|e| ScanError::Backend(e.to_string()))?;
    writer
        .write_image_data(image.as_raw())
        .map_err(|e| ScanError::Backend(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| ScanError::Backend(e.to_string()))?;
    Ok(())
}
/// Read common scanner resolution metadata; absent/nonphysical density uses the host's fallback.
pub fn resolution(path: &Path) -> Option<u32> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(256 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let valid = |dpi: f64| {
        if dpi.is_finite() && (75.0..=600.0).contains(&dpi.round()) {
            Some(dpi.round() as u32)
        } else {
            None
        }
    };
    if bytes.starts_with(b"BM") && bytes.len() >= 54 {
        let ppm = i32::from_le_bytes(bytes[38..42].try_into().ok()?);
        return valid(f64::from(ppm) * 0.0254);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut offset = 8_usize;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            let end = offset.checked_add(length)?.checked_add(12)?;
            if end > bytes.len() {
                break;
            }
            if &bytes[offset + 4..offset + 8] == b"pHYs" && length == 9 && bytes[offset + 16] == 1 {
                return valid(
                    f64::from(u32::from_be_bytes(
                        bytes[offset + 8..offset + 12].try_into().ok()?,
                    )) * 0.0254,
                );
            }
            offset = end;
        }
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut offset = 2_usize;
        while offset + 4 <= bytes.len() && bytes[offset] == 0xff {
            let marker = bytes[offset + 1];
            if marker == 0xda || marker == 0xd9 {
                break;
            }
            let length = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
            if length < 2 || offset + 2 + length > bytes.len() {
                break;
            }
            let data = &bytes[offset + 4..offset + 2 + length];
            if marker == 0xe0 && data.len() >= 12 && data.starts_with(b"JFIF\0") {
                let density = f64::from(u16::from_be_bytes([data[8], data[9]]));
                return match data[7] {
                    1 => valid(density),
                    2 => valid(density * 2.54),
                    _ => None,
                };
            }
            offset += 2 + length;
        }
    }
    None
}
/// Estimate a correction by horizontal projection, bounded to +/-12 degrees.
pub fn deskew_angle(image: &RgbImage, cancel: &CancellationToken) -> Result<f32> {
    let small = DynamicImage::ImageRgb8(image.clone())
        .resize(900, 1200, image::imageops::FilterType::Triangle)
        .to_luma8();
    let points: Vec<_> = small
        .enumerate_pixels()
        .filter(|(x, _, p)| x % 2 == 0 && p[0] < 140)
        .map(|(x, y, _)| {
            (
                x as f32 - small.width() as f32 / 2.0,
                y as f32 - small.height() as f32 / 2.0,
            )
        })
        .collect();
    if points.len() < 50 {
        return Ok(0.0);
    }
    let mut best = (0.0, 0_u64);
    let mut zero = 0;
    for step in -48..=48 {
        cancel.check()?;
        let angle = step as f32 * 0.25;
        let (sin, cos) = angle.to_radians().sin_cos();
        let mut rows = vec![0_u64; (small.height() + small.width() + 8) as usize];
        let offset = rows.len() as f32 / 2.0;
        for &(x, y) in &points {
            if let Some(n) = rows.get_mut((sin * x + cos * y + offset) as usize) {
                *n += 1;
            }
        }
        let score = rows.iter().map(|n| n * n).sum();
        if step == 0 {
            zero = score;
        }
        if score > best.1 {
            best = (angle, score);
        }
    }
    Ok(if best.1 as f64 > zero as f64 * 1.03 {
        best.0
    } else {
        0.0
    })
}
pub fn ink_coverage(image: &RgbImage) -> f64 {
    let gray = DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let margin = (gray.width().min(gray.height()) / 100).max(1);
    let mut dark = 0_u64;
    let mut count = 0_u64;
    for y in margin..gray.height().saturating_sub(margin) {
        for x in margin..gray.width().saturating_sub(margin) {
            dark += u64::from(gray.get_pixel(x, y)[0] < 210);
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        100.0 * dark as f64 / count as f64
    }
}
fn crop_content(image: &RgbImage, dpi: u32) -> RgbImage {
    let gray = DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let mut mask = image::GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
        image::Luma([if gray.get_pixel(x, y)[0] < 210 {
            255
        } else {
            0
        }])
    });
    // Scanner bars can join around a corner, forming a single contour as large
    // as the page. Find a clear separator inside the outer 2.5% on each side.
    // Short marks (including text near the edge) do not qualify as scanner bars.
    let (w, h) = mask.dimensions();
    let mut columns = vec![0_u32; w as usize];
    let mut rows = vec![0_u32; h as usize];
    for (x, y, p) in mask.enumerate_pixels() {
        if p[0] != 0 {
            columns[x as usize] += 1;
            rows[y as usize] += 1;
        }
    }
    fn separator(counts: impl Iterator<Item = u32>, band: usize, span: u32) -> u32 {
        let mut saw_bar = false;
        let mut clear = 0;
        for (i, count) in counts.take(band).enumerate() {
            saw_bar |= count > (span / 12).max(band as u32 * 4).max(4);
            clear = if count <= (span / 100).max(1) {
                clear + 1
            } else {
                0
            };
            if saw_bar && clear >= 2 {
                return (i + 1) as u32;
            }
        }
        0
    }
    let l_edge = separator(columns.iter().copied(), (w / 40) as usize, h);
    let r_edge = w - separator(columns.iter().copied().rev(), (w / 40) as usize, h);
    let t_edge = separator(rows.iter().copied(), (h / 40) as usize, w);
    let b_edge = h - separator(rows.iter().copied().rev(), (h / 40) as usize, w);
    for (x, y, p) in mask.enumerate_pixels_mut() {
        if x < l_edge || x >= r_edge || y < t_edge || y >= b_edge {
            p[0] = 0;
        }
    }
    let contours = imageproc::contours::find_contours::<i32>(&mask);
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for contour in contours {
        let Some(first) = contour.points.first() else {
            continue;
        };
        let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x, first.y);
        for point in contour.points {
            l = l.min(point.x);
            t = t.min(point.y);
            r = r.max(point.x);
            b = b.max(point.y);
        }
        if (r - l + 1) * (b - t + 1) < 12 {
            continue;
        }
        // Isolated thin marks close to an edge can be too faint/short to form
        // a projection bar. Keep compact marks and all interior components.
        let cw = r - l + 1;
        let ch = b - t + 1;
        let vertical_edge =
            (r < w as i32 / 40 || l >= w as i32 * 39 / 40) && ch > cw * 8 && ch > h as i32 / 30;
        let horizontal_edge =
            (b < h as i32 / 40 || t >= h as i32 * 39 / 40) && cw > ch * 8 && cw > w as i32 / 30;
        if vertical_edge || horizontal_edge {
            continue;
        }
        bounds = Some(match bounds {
            Some((a, c, d, e)) => (a.min(l), c.min(t), d.max(r), e.max(b)),
            None => (l, t, r, b),
        });
    }
    let Some((l, t, r, b)) = bounds else {
        return image::imageops::crop_imm(image, l_edge, t_edge, r_edge - l_edge, b_edge - t_edge)
            .to_image();
    };
    let margin = (dpi as f32 * 0.12).round().max(8.0) as i32;
    let l = (l - margin).max(l_edge as i32) as u32;
    let t = (t - margin).max(t_edge as i32) as u32;
    let r = (r + margin + 1).min(r_edge as i32) as u32;
    let b = (b + margin + 1).min(b_edge as i32) as u32;
    image::imageops::crop_imm(image, l, t, r - l, b - t).to_image()
}
pub fn correct(
    original: &RgbImage,
    profile: &ScanProfile,
    extra_rotation: u16,
    cancel: &CancellationToken,
) -> Result<CorrectedPage> {
    profile.validate()?;
    cancel.check()?;
    if original.width() < 16
        || original.height() < 16
        || u64::from(original.width()) * u64::from(original.height()) > MAX_PIXELS
    {
        return Err(ScanError::InvalidRequest(
            "Page must be at least 16 × 16 and at most 60 megapixels".into(),
        ));
    }
    if ![0, 90, 180, 270].contains(&extra_rotation) {
        return Err(ScanError::InvalidRequest("Invalid page rotation".into()));
    }
    let options = &profile.cleanup;
    let cropped;
    let original = if options.auto_crop {
        cropped = crop_content(original, profile.dpi);
        &cropped
    } else {
        original
    };
    let angle = if options.deskew {
        deskew_angle(original, cancel)?
    } else {
        0.0
    };
    let mut image = if angle.abs() >= 0.15 {
        rotate_about_center_no_crop(
            original,
            angle.to_radians(),
            Interpolation::Bilinear,
            Border::Constant(Rgb([255, 255, 255])),
        )
    } else {
        original.clone()
    };
    if u64::from(image.width()) * u64::from(image.height()) > MAX_PIXELS {
        return Err(ScanError::InvalidRequest(
            "Corrected canvas exceeds 60 megapixels".into(),
        ));
    }
    image = match (options.rotate_degrees + extra_rotation) % 360 {
        90 => image::imageops::rotate90(&image),
        180 => image::imageops::rotate180(&image),
        270 => image::imageops::rotate270(&image),
        _ => image,
    };
    cancel.check()?;
    if options.remove_background {
        // A small illumination map keeps normalization practical on high-resolution scans.
        let small = DynamicImage::ImageRgb8(image.clone())
            .resize(160, 160, image::imageops::FilterType::Triangle)
            .to_rgb8();
        let background = image::imageops::resize(
            &image::imageops::blur(&small, 5.0),
            image.width(),
            image.height(),
            image::imageops::FilterType::Triangle,
        );
        for (pixel, bg) in image.pixels_mut().zip(background.pixels()) {
            for c in 0..3 {
                pixel[c] = (u32::from(pixel[c]) * 255 / u32::from(bg[c]).max(1)).min(255) as u8;
            }
        }
    }
    cancel.check()?;
    if options.sharpen {
        let blurred = image::imageops::blur(&image, 1.0);
        for (p, b) in image.pixels_mut().zip(blurred.pixels()) {
            for c in 0..3 {
                p[c] = (1.6 * f32::from(p[c]) - 0.6 * f32::from(b[c])).clamp(0.0, 255.0) as u8;
            }
        }
    }
    let alpha = 2.0_f64.powf(f64::from(options.contrast) / 50.0);
    let beta = f64::from(options.brightness) * 2.55 + 127.5 * (1.0 - alpha);
    if options.brightness != 0 || options.contrast != 0 {
        for p in image.pixels_mut() {
            for c in 0..3 {
                p[c] = (f64::from(p[c]) * alpha + beta).clamp(0.0, 255.0) as u8;
            }
        }
    }
    cancel.check()?;
    let ink_percent = ink_coverage(&image);
    if profile.color != ScanColor::Color {
        let mut gray = DynamicImage::ImageRgb8(image).to_luma8();
        if profile.color == ScanColor::BlackWhite {
            gray = balanced_black_white(&gray, profile.dpi, cancel)?;
        }
        image = DynamicImage::ImageLuma8(gray).to_rgb8();
    }
    if options.denoise {
        // Thresholding can turn faint background noise into fresh black specks.
        // Clean the final image, retaining text pixels rather than blurring them.
        image = despeckle::remove_dust(&image, profile.dpi, cancel)?;
    }
    cancel.check()?;
    Ok(CorrectedPage {
        image,
        angle,
        ink_percent,
        blank: ink_percent < options.blank_ink_percent,
    })
}
