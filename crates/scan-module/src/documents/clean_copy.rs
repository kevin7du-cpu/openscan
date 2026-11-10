//! Conservative scan cleanup with explicit, reviewed text replacement.
//! Acquisition originals are never modified. OCR guesses never replace ink by default.
use super::{TextLine, content_cleanup};
use crate::{CancellationToken, processing::MAX_PIXELS};
use ab_glyph::{Font, FontRef};
use anyhow::{Result, ensure};
use image::{Rgb, RgbImage};
use serde::{Deserialize, Serialize};

const FONT: &[u8] = include_bytes!("../../assets/Ubuntu-Light.ttf");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CleanLine {
    pub line: TextLine,
    /// False retains scanned ink instead of replacing it with recognized characters.
    pub rebuild: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CleanOptions {
    pub remove_specks: bool,
    pub reduce_shadows: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            remove_specks: true,
            reduce_shadows: true,
        }
    }
}

fn rect(image: &RgbImage, bounds: [f32; 4]) -> Result<[u32; 4]> {
    ensure!(
        bounds.iter().all(|x| x.is_finite())
            && bounds[0] >= 0.0
            && bounds[1] >= 0.0
            && bounds[2] >= 1.0
            && bounds[3] >= 1.0,
        "Invalid clean-copy text bounds"
    );
    let [x, y, w, h] = bounds;
    ensure!(
        x < image.width() as f32
            && y < image.height() as f32
            && x + w <= image.width() as f32 + 1.0
            && y + h <= image.height() as f32 + 1.0,
        "Clean-copy region lies outside its source page"
    );
    Ok([
        x.floor() as u32,
        y.floor() as u32,
        (x + w).ceil().min(image.width() as f32) as u32,
        (y + h).ceil().min(image.height() as f32) as u32,
    ])
}

/// OCR is not a reliable handwriting, barcode or graphic classifier. Never
/// substitute recognized characters until the user explicitly selects a region.
pub fn prepare(image: &RgbImage, lines: Vec<TextLine>) -> Result<Vec<CleanLine>> {
    ensure!(lines.len() <= 5000, "Too many OCR regions on one page");
    lines
        .into_iter()
        .map(|line| {
            rect(image, line.bounds)?;
            Ok(CleanLine {
                line,
                rebuild: false,
            })
        })
        .collect()
}

/// Clean high-confidence paper gaps within recognized text while retaining ink.
/// Unrecognized areas remain unchanged, including missed text and signatures.
/// Dark folds and uncertain marks remain; retyping is an explicit reviewed opt-in.
pub fn render(
    original: &RgbImage,
    lines: &[CleanLine],
    cancel: &CancellationToken,
) -> Result<RgbImage> {
    render_with_options(original, lines, CleanOptions::default(), cancel)
}

pub fn render_with_options(
    original: &RgbImage,
    lines: &[CleanLine],
    options: CleanOptions,
    cancel: &CancellationToken,
) -> Result<RgbImage> {
    render_with_options_at_dpi(original, lines, options, 300, cancel)
}

/// Content-aware cleanup at the scan's actual DPI. Unknown areas remain
/// unchanged; recognized text supplies context for conservative gap cleanup.
/// Large, dense, colored or otherwise ambiguous regions are retained.
pub fn render_with_options_at_dpi(
    original: &RgbImage,
    lines: &[CleanLine],
    options: CleanOptions,
    dpi: u32,
    cancel: &CancellationToken,
) -> Result<RgbImage> {
    ensure!((75..=600).contains(&dpi), "Cleanup requires 75-600 DPI");
    ensure!(
        original.width() >= 16
            && original.height() >= 16
            && u64::from(original.width()) * u64::from(original.height()) <= MAX_PIXELS,
        "Invalid clean-copy page dimensions"
    );
    ensure!(lines.len() <= 5000, "Too many OCR regions on one page");
    cancel.check()?;
    let font = FontRef::try_from_slice(FONT)?;
    let regions = lines
        .iter()
        .map(|line| rect(original, line.line.bounds))
        .collect::<Result<Vec<_>>>()?;
    for line in lines {
        ensure!(
            line.line.text.chars().count() <= 2000 && !line.line.text.chars().any(char::is_control),
            "Use a single text line of at most 2000 characters"
        );
        if line.rebuild {
            ensure!(
                line.line.text.chars().all(|c| font.glyph_id(c).0 != 0),
                "The clean-copy font cannot display a character; keep that region as an original image"
            );
        }
    }
    let analysis = lines
        .iter()
        .zip(&regions)
        .map(|(line, &bounds)| content_cleanup::TextRegion {
            bounds,
            characters: line
                .line
                .text
                .chars()
                .filter(|c| !c.is_whitespace())
                .count(),
        })
        .collect::<Vec<_>>();
    let mut result = content_cleanup::clean(original, &analysis, dpi, options, cancel)?;
    if lines.iter().all(|line| !line.rebuild) {
        return Ok(result);
    }
    // Explicit text replacement is separate from the default pixel-preserving
    // path. Unchecked overlapping regions retain the reviewed cleanup result.
    let cleaned = result.clone();
    let copy = |output: &mut RgbImage, [l, t, r, b]: [u32; 4]| {
        for y in t..b {
            for x in l..r {
                output.put_pixel(x, y, *cleaned.get_pixel(x, y));
            }
        }
    };
    for (line, &[l, t, r, b]) in lines.iter().zip(&regions) {
        cancel.check()?;
        if !line.rebuild {
            copy(&mut result, [l, t, r, b]);
            continue;
        }
        let text = line.line.text.trim();
        let (w, h) = (r - l, b - t);
        let height = (h as f32 * 1.4).clamp(12.0, 512.0);
        let (natural_w, _) = imageproc::drawing::text_size(height, &font, text);
        // Never stretch a short OCR guess to fill a barcode or a long mark.
        // Uniform scaling only, constrained by both dimensions, with no upscale.
        let scale = height * (w as f32 / natural_w.max(1) as f32).min(1.0);
        let mut glyphs =
            RgbImage::from_pixel(w + 4, (height.ceil() as u32) + 4, Rgb([255, 255, 255]));
        imageproc::drawing::draw_text_mut(&mut glyphs, Rgb([0, 0, 0]), 2, 0, scale, &font, text);
        let mut ink: Option<[u32; 4]> = None;
        for (x, y, p) in glyphs.enumerate_pixels() {
            if p[0] < 250 {
                ink = Some(match ink {
                    Some([a, c, d, e]) => [a.min(x), c.min(y), d.max(x + 1), e.max(y + 1)],
                    None => [x, y, x + 1, y + 1],
                });
            }
        }
        if let Some([a, c, d, e]) = ink {
            let glyphs = image::imageops::crop_imm(&glyphs, a, c, d - a, e - c).to_image();
            let fit = (w as f32 / glyphs.width() as f32)
                .min(h as f32 / glyphs.height() as f32)
                .min(1.0);
            let fw = (glyphs.width() as f32 * fit).round().max(1.0) as u32;
            let fh = (glyphs.height() as f32 * fit).round().max(1.0) as u32;
            let fitted =
                image::imageops::resize(&glyphs, fw, fh, image::imageops::FilterType::Lanczos3);
            // Erase only the approved box, never its neighbouring table rules.
            for y in t..b {
                for x in l..r {
                    result.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
            image::imageops::replace(
                &mut result,
                &fitted,
                i64::from(l),
                i64::from(t + (h - fh) / 2),
            );
        }
    }
    // Unchecked regions win if OCR boxes overlap a stamp, signature, or photo.
    for (line, &bounds) in lines.iter().zip(&regions) {
        if !line.rebuild {
            copy(&mut result, bounds);
        }
    }
    cancel.check()?;
    Ok(result)
}

/// Erase only explicitly selected areas on a working copy. Validate the whole
/// request first; these coordinates are source pixels, independent of zoom/DPI.
pub fn erase_marks(
    image: &mut RgbImage,
    areas: &[[f32; 4]],
    cancel: &CancellationToken,
) -> Result<()> {
    ensure!(areas.len() <= 500, "Too many marked areas");
    let regions = areas
        .iter()
        .map(|&b| rect(image, b))
        .collect::<Result<Vec<_>>>()?;
    cancel.check()?;
    for [l, t, r, b] in regions {
        for y in t..b {
            if y % 64 == 0 {
                cancel.check()?;
            }
            for x in l..r {
                image.put_pixel(x, y, Rgb([255; 3]));
            }
        }
    }
    Ok(())
}
