// SPDX-License-Identifier: MIT OR Apache-2.0
//! Page clean-up: crop scanner borders, even out shadows, straighten, remove specks.
//!
//! Every step works on the scanned image alone, with no machine-learning model, so the
//! results are predictable and fast. Pages are kept as 8-bit grey or 8-bit RGB throughout.

use image::imageops::{self, FilterType};
use image::{DynamicImage, GrayImage, Luma, Rgb};
use imageproc::contrast::otsu_level;
use imageproc::geometric_transformations::{Interpolation, rotate_about_center};
use imageproc::region_labelling::{Connectivity, connected_components};

use crate::settings::{ColorMode, Level, ScanSettings};

/// A cleaned page ready to be written out.
#[derive(Clone)]
pub struct Page {
    pub image: std::sync::Arc<DynamicImage>,
    /// Unmodified acquisition, retained for review and recovery.
    pub original: std::sync::Arc<DynamicImage>,
    /// Original file, when importing. Session persistence copies its exact bytes.
    pub source: Option<std::path::PathBuf>,
    /// Resized on the processing worker, never in the GUI event loop.
    pub preview: std::sync::Arc<image::RgbImage>,
    /// Resolution of `image` in dots per inch.
    pub dpi: u32,
}

impl Page {
    pub fn from_image(image: DynamicImage, dpi: u32) -> Self {
        Self::processed(std::sync::Arc::new(image.clone()), image, dpi)
    }
    fn processed(original: std::sync::Arc<DynamicImage>, image: DynamicImage, dpi: u32) -> Self {
        let preview = std::sync::Arc::new(image.thumbnail(1200, 1600).to_rgb8());
        Self {
            image: std::sync::Arc::new(image),
            original,
            source: None,
            preview,
            dpi,
        }
    }
}

/// Largest tilt, in degrees, that deskewing looks for.
const MAX_SKEW_DEGREES: f32 = 10.0;
/// Tilts smaller than this are left alone, since rotating blurs the page slightly.
const MIN_SKEW_DEGREES: f32 = 0.15;
/// Rows or columns darker than this, on a 0-255 scale, count as scanner border.
const BORDER_DARKNESS: f32 = 70.0;
/// Blur radius, in pixels at 300 dpi, that hides a printed dot pattern.
const DESCREEN_SIGMA_AT_300_DPI: f32 = 1.0;
/// A page with a smaller share of dark pixels than this counts as blank.
const BLANK_INK_FRACTION: f64 = 0.0008;

/// Applies the clean-up steps chosen in `settings` to one scanned image.
///
/// Returns `None` when blank-page removal is on and the page is blank.
pub fn clean_page(image: DynamicImage, settings: &ScanSettings) -> Option<Page> {
    let original = std::sync::Arc::new(image.clone());
    let mut image = to_working_format(image, settings.color_mode);
    let dpi = settings.resolution.max(1);
    let options = &settings.optimize_opts;

    // Each correction has its own switch and runs whether or not compression is on.
    if options.crop_borders {
        image = crop_dark_borders(&image);
    }
    if let Some(target) = paper_target(options.background_removal) {
        image = flatten_background(&image, target);
    }
    if options.deskew {
        image = deskew(&image);
    }
    if options.descreen {
        image = descreen(&image, dpi);
    }
    if options.despeckle {
        image = despeckle(&image, dpi);
    }
    if let Some(sigma) = sharpen_sigma(options.text_sharpening) {
        image = image.unsharpen(sigma * dpi as f32 / 300.0, 2);
    }
    if options.remove_blank_pages && is_blank(&image.to_luma8()) {
        return None;
    }
    // In Automatic colour mode, a page with no colour on it is kept as greys: a third the data.
    if settings.color_mode == ColorMode::Auto && is_colourless(&image) {
        image = DynamicImage::ImageLuma8(image.into_luma8());
    }
    if settings.color_mode == ColorMode::BlackWhite {
        image = DynamicImage::ImageLuma8(binarize(&image.to_luma8()));
    }
    Some(Page::processed(original, image, dpi))
}

/// Largest difference between a pixel's red, green and blue that still counts as grey.
const GREY_TOLERANCE: u8 = 32;
/// A page with a smaller share of coloured pixels than this counts as colourless.
const COLOURED_FRACTION: f64 = 0.002;

/// Whether a page has no real colour on it (a little scanner colour noise is allowed).
pub fn is_colourless(image: &DynamicImage) -> bool {
    let DynamicImage::ImageRgb8(rgb) = image else {
        return true;
    };
    let coloured = rgb
        .pixels()
        .filter(|pixel| {
            let [r, g, b] = pixel.0;
            r.max(g).max(b) - r.min(g).min(b) > GREY_TOLERANCE
        })
        .count();
    (coloured as f64) < COLOURED_FRACTION * rgb.pixels().len() as f64
}

/// Converts to 8-bit grey or 8-bit RGB according to the colour mode.
fn to_working_format(image: DynamicImage, mode: ColorMode) -> DynamicImage {
    let grey = match mode {
        ColorMode::Color => false,
        ColorMode::Grayscale | ColorMode::BlackWhite => true,
        ColorMode::Auto => !image.color().has_color(),
    };
    if grey {
        DynamicImage::ImageLuma8(image.into_luma8())
    } else {
        DynamicImage::ImageRgb8(image.into_rgb8())
    }
}

/// Trims rows and columns of black scanner background from the page edges.
pub fn crop_dark_borders(image: &DynamicImage) -> DynamicImage {
    let grey = image.to_luma8();
    let (width, height) = grey.dimensions();
    let row_mean = |y: u32| {
        (0..width)
            .map(|x| grey.get_pixel(x, y)[0] as f32)
            .sum::<f32>()
            / width as f32
    };
    let col_mean = |x: u32| {
        (0..height)
            .map(|y| grey.get_pixel(x, y)[0] as f32)
            .sum::<f32>()
            / height as f32
    };

    // Never trim more than a sixth from any side, so a dark page is not cropped away.
    let dark_run = |limit: u32, mean_at: &dyn Fn(u32) -> f32| {
        (0..limit / 6)
            .take_while(|&i| mean_at(i) < BORDER_DARKNESS)
            .count() as u32
    };
    let top = dark_run(height, &|i| row_mean(i));
    let bottom = dark_run(height, &|i| row_mean(height - 1 - i));
    let left = dark_run(width, &|i| col_mean(i));
    let right = dark_run(width, &|i| col_mean(width - 1 - i));

    if top + bottom + left + right == 0 {
        return image.clone();
    }
    image.crop_imm(left, top, width - left - right, height - top - bottom)
}

/// Brightness bare paper is lifted to for each strength of background removal.
///
/// Values above 255 clip to white, which also clears faint texture left in the paper.
pub fn paper_target(level: Level) -> Option<f32> {
    match level {
        Level::Off => None,
        Level::Low => Some(255.0),
        Level::Medium => Some(270.0),
        Level::High => Some(295.0),
    }
}

/// Blur radius of the sharpening mask, in pixels at 300 dpi, for each strength.
fn sharpen_sigma(level: Level) -> Option<f32> {
    match level {
        Level::Off => None,
        Level::Low => Some(0.8),
        Level::Medium => Some(1.5),
        Level::High => Some(2.5),
    }
}

/// Smooths away the dot pattern of printed photographs with a light blur.
pub fn descreen(image: &DynamicImage, dpi: u32) -> DynamicImage {
    image.blur(DESCREEN_SIGMA_AT_300_DPI * dpi as f32 / 300.0)
}

/// Evens out shadows from creases, wrinkles and uneven lighting.
///
/// The paper's brightness is estimated at every point by ignoring the ink, and each pixel is
/// then brightened by however much the paper there falls short of white. This removes the
/// shading a wrinkle causes; it does not undo the bending of the text itself.
pub fn flatten_background(image: &DynamicImage, target: f32) -> DynamicImage {
    let background = paper_brightness(&image.to_luma8());
    let correct = |value: u8, paper: u8| -> u8 {
        // Aim a little past white so faint leftover texture in the paper is cleared too,
        // and limit the boost so dark areas such as photographs are not blown out.
        let gain = (target / paper.max(1) as f32).min(2.5);
        (value as f32 * gain).min(255.0) as u8
    };
    match image {
        DynamicImage::ImageLuma8(grey) => {
            let mut out = grey.clone();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                pixel[0] = correct(pixel[0], background.get_pixel(x, y)[0]);
            }
            DynamicImage::ImageLuma8(out)
        }
        other => {
            let mut out = other.to_rgb8();
            for (x, y, pixel) in out.enumerate_pixels_mut() {
                let paper = background.get_pixel(x, y)[0];
                *pixel = Rgb(pixel.0.map(|channel| correct(channel, paper)));
            }
            DynamicImage::ImageRgb8(out)
        }
    }
}

/// Estimates how bright the bare paper is at each point of the page.
fn paper_brightness(grey: &GrayImage) -> GrayImage {
    let (width, height) = grey.dimensions();
    // Work at one-eighth size: paper shading changes slowly and this is much faster.
    let small_width = (width / 8).max(16);
    let small_height = (height / 8).max(16);
    let small = imageops::resize(grey, small_width, small_height, FilterType::Triangle);
    // Taking the brightest nearby value erases ink, which is darker and thinner than paper.
    let without_ink = max_filter(&small, 6);
    let smooth = imageops::blur(&without_ink, 3.0);
    imageops::resize(&smooth, width, height, FilterType::Triangle)
}

/// Replaces each pixel with the brightest pixel within `radius` in both directions.
fn max_filter(image: &GrayImage, radius: u32) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut horizontal = image.clone();
    for y in 0..height {
        for x in 0..width {
            let from = x.saturating_sub(radius);
            let to = (x + radius).min(width - 1);
            let brightest = (from..=to)
                .map(|i| image.get_pixel(i, y)[0])
                .max()
                .unwrap_or(0);
            horizontal.put_pixel(x, y, Luma([brightest]));
        }
    }
    let mut out = horizontal.clone();
    for y in 0..height {
        let from = y.saturating_sub(radius);
        let to = (y + radius).min(height - 1);
        for x in 0..width {
            let brightest = (from..=to)
                .map(|i| horizontal.get_pixel(x, i)[0])
                .max()
                .unwrap_or(0);
            out.put_pixel(x, y, Luma([brightest]));
        }
    }
    out
}

/// Measures how far the lines of text are tilted, in degrees clockwise.
///
/// Tries a range of angles and picks the one at which the ink lines up into the sharpest
/// rows, which is where lines of text and the gaps between them are most distinct.
pub fn estimate_skew_degrees(grey: &GrayImage) -> f32 {
    let (width, height) = grey.dimensions();
    let scale = (600.0 / width.max(height) as f32).min(1.0);
    let small_width = ((width as f32 * scale) as u32).max(1);
    let small_height = ((height as f32 * scale) as u32).max(1);
    let small = imageops::resize(grey, small_width, small_height, FilterType::Triangle);

    let ink_level = otsu_level(&small);
    let ink: Vec<(f32, f32)> = small
        .enumerate_pixels()
        .filter(|(_, _, pixel)| pixel[0] <= ink_level)
        .map(|(x, y, _)| (x as f32, y as f32))
        .collect();
    // Too little ink to judge, or so much that the "ink" is really a picture.
    let total = (small_width * small_height) as usize;
    if ink.len() < 50 || ink.len() > total / 2 {
        return 0.0;
    }

    let offset = small_width as f32 * MAX_SKEW_DEGREES.to_radians().tan() + 2.0;
    let rows = small_height as usize + 2 * offset as usize + 2;
    let sharpness = |degrees: f32| -> f64 {
        let slope = degrees.to_radians().tan();
        let mut histogram = vec![0u32; rows];
        for &(x, y) in &ink {
            histogram[(y - x * slope + offset) as usize] += 1;
        }
        histogram.iter().map(|&count| (count as f64).powi(2)).sum()
    };
    let best_of = |from: f32, to: f32, step: f32| -> f32 {
        let steps = ((to - from) / step).round() as i32;
        (0..=steps)
            .map(|i| from + i as f32 * step)
            .max_by(|a, b| sharpness(*a).total_cmp(&sharpness(*b)))
            .unwrap_or(0.0)
    };
    let coarse = best_of(-MAX_SKEW_DEGREES, MAX_SKEW_DEGREES, 0.5);
    best_of(coarse - 0.5, coarse + 0.5, 0.05)
}

/// Rotates the page so its lines of text are level.
pub fn deskew(image: &DynamicImage) -> DynamicImage {
    let degrees = estimate_skew_degrees(&image.to_luma8());
    if degrees.abs() < MIN_SKEW_DEGREES {
        return image.clone();
    }
    let angle = -degrees.to_radians();
    match image {
        DynamicImage::ImageLuma8(grey) => DynamicImage::ImageLuma8(rotate_about_center(
            grey,
            angle,
            Interpolation::Bilinear,
            Luma([255]),
        )),
        other => DynamicImage::ImageRgb8(rotate_about_center(
            &other.to_rgb8(),
            angle,
            Interpolation::Bilinear,
            Rgb([255, 255, 255]),
        )),
    }
}

/// Removes isolated specks: dark blobs too small to be part of any letter.
pub fn despeckle(image: &DynamicImage, dpi: u32) -> DynamicImage {
    let grey = image.to_luma8();
    // Look for specks with a threshold halfway between the ink level and white, so faint
    // grey dots count too. Letters are unaffected: their soft edges stay attached to them.
    let otsu = otsu_level(&grey);
    let ink_level = otsu + (255 - otsu) / 2;
    let mut mask = grey.clone();
    for pixel in mask.pixels_mut() {
        pixel[0] = if pixel[0] <= ink_level { 255 } else { 0 };
    }
    let labels = connected_components(&mask, Connectivity::Eight, Luma([0u8]));
    let label_count = labels.pixels().map(|p| p[0]).max().unwrap_or(0) as usize + 1;
    let mut areas = vec![0u32; label_count];
    for pixel in labels.pixels() {
        areas[pixel[0] as usize] += 1;
    }
    // About eight pixels at 300 dpi: smaller than a full stop or the dot of an "i".
    let largest_speck = ((dpi as f32 / 300.0).powi(2) * 8.0).round().max(2.0) as u32;
    let is_speck = |x: u32, y: u32| {
        let label = labels.get_pixel(x, y)[0] as usize;
        label != 0 && areas[label] <= largest_speck
    };

    let (width, height) = grey.dimensions();
    let speck_pixels: Vec<(u32, u32)> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| is_speck(x, y))
        .collect();
    if speck_pixels.is_empty() {
        return image.clone();
    }

    // Paint each speck with the average of the clean paper around it.
    let paper_near = |x: u32, y: u32, read: &dyn Fn(u32, u32) -> [f32; 3]| -> [u8; 3] {
        let mut sum = [0.0f32; 3];
        let mut count = 0.0f32;
        for ny in y.saturating_sub(2)..=(y + 2).min(height - 1) {
            for nx in x.saturating_sub(2)..=(x + 2).min(width - 1) {
                if mask.get_pixel(nx, ny)[0] == 0 {
                    let value = read(nx, ny);
                    for channel in 0..3 {
                        sum[channel] += value[channel];
                    }
                    count += 1.0;
                }
            }
        }
        if count == 0.0 {
            [255; 3]
        } else {
            sum.map(|total| (total / count) as u8)
        }
    };

    match image {
        DynamicImage::ImageLuma8(original) => {
            let mut out = original.clone();
            for &(x, y) in &speck_pixels {
                let value = paper_near(x, y, &|nx, ny| [original.get_pixel(nx, ny)[0] as f32; 3]);
                out.put_pixel(x, y, Luma([value[0]]));
            }
            DynamicImage::ImageLuma8(out)
        }
        other => {
            let original = other.to_rgb8();
            let mut out = original.clone();
            for &(x, y) in &speck_pixels {
                let value = paper_near(x, y, &|nx, ny| {
                    original.get_pixel(nx, ny).0.map(|c| c as f32)
                });
                out.put_pixel(x, y, Rgb(value));
            }
            DynamicImage::ImageRgb8(out)
        }
    }
}

/// Whether the page has so little ink that it can be treated as empty.
pub fn is_blank(grey: &GrayImage) -> bool {
    let dark = grey.pixels().filter(|pixel| pixel[0] < 128).count();
    (dark as f64) < grey.pixels().len() as f64 * BLANK_INK_FRACTION
}

/// Turns a grey page into pure black and white.
pub fn binarize(grey: &GrayImage) -> GrayImage {
    let level = otsu_level(grey);
    let mut out = grey.clone();
    for pixel in out.pixels_mut() {
        pixel[0] = if pixel[0] <= level { 0 } else { 255 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white page with rows of black dashes, roughly like lines of text.
    fn text_like_page() -> GrayImage {
        let mut page = GrayImage::from_pixel(600, 800, Luma([255]));
        for line in 0..20 {
            let y0 = 80 + line * 32;
            for word in 0..9 {
                let x0 = 60 + word * 55;
                for y in y0..y0 + 10 {
                    for x in x0..x0 + 40 {
                        page.put_pixel(x, y, Luma([0]));
                    }
                }
            }
        }
        page
    }

    fn ink_count(image: &GrayImage) -> usize {
        image.pixels().filter(|p| p[0] < 128).count()
    }

    #[test]
    fn colour_is_noticed_and_grey_scans_are_not_mistaken_for_colour() {
        let mut page = image::RgbImage::from_pixel(200, 200, Rgb([236, 240, 232]));
        assert!(is_colourless(&DynamicImage::ImageRgb8(page.clone())));
        // A red stamp covering one percent of the page.
        for y in 0..20 {
            for x in 0..20 {
                page.put_pixel(x, y, Rgb([200, 30, 30]));
            }
        }
        assert!(!is_colourless(&DynamicImage::ImageRgb8(page)));
    }

    #[test]
    fn descreen_softens_a_dot_pattern_and_sharpening_keeps_text() {
        // A fine checkerboard stands in for the dots of a printed photograph.
        let dots = GrayImage::from_fn(200, 200, |x, y| {
            Luma([if (x + y) % 2 == 0 { 60 } else { 200 }])
        });
        let smooth = descreen(&DynamicImage::ImageLuma8(dots), 300).to_luma8();
        let centre: Vec<u8> = (90..110).map(|x| smooth.get_pixel(x, 100)[0]).collect();
        let spread = centre.iter().max().unwrap() - centre.iter().min().unwrap();
        assert!(spread < 40, "dot pattern still visible: spread {spread}");

        let mut settings = ScanSettings::default();
        settings.optimize_opts.text_sharpening = Level::High;
        settings.optimize_opts.descreen = true;
        let before = text_like_page();
        let page = clean_page(DynamicImage::ImageLuma8(before.clone()), &settings).unwrap();
        let after = page.image.to_luma8();
        assert_eq!(after.dimensions(), before.dimensions());
        let (was, now) = (ink_count(&before) as f64, ink_count(&after) as f64);
        assert!(
            (now - was).abs() / was < 0.1,
            "ink changed from {was} to {now}"
        );
    }

    #[test]
    fn skew_is_measured_and_removed() {
        let tilted = rotate_about_center(
            &text_like_page(),
            3.0f32.to_radians(),
            Interpolation::Bilinear,
            Luma([255]),
        );
        let measured = estimate_skew_degrees(&tilted);
        assert!((measured - 3.0).abs() < 0.3, "measured {measured}");

        let straightened = deskew(&DynamicImage::ImageLuma8(tilted)).to_luma8();
        let remaining = estimate_skew_degrees(&straightened);
        assert!(remaining.abs() < 0.3, "remaining {remaining}");
    }

    #[test]
    fn level_pages_are_left_untouched() {
        let page = DynamicImage::ImageLuma8(text_like_page());
        assert_eq!(deskew(&page).to_luma8(), page.to_luma8());
    }

    #[test]
    fn specks_go_and_text_stays() {
        let clean = text_like_page();
        let mut dirty = clean.clone();
        // Dots in the gaps between lines, so none touches a dash and becomes part of it.
        for i in 0..40 {
            dirty.put_pixel(20 + i * 13, 100 + (i % 20) * 32, Luma([0]));
        }
        let result = despeckle(&DynamicImage::ImageLuma8(dirty), 300).to_luma8();
        assert_eq!(ink_count(&result), ink_count(&clean));
    }

    #[test]
    fn black_scanner_borders_are_cropped() {
        let mut framed = GrayImage::from_pixel(700, 900, Luma([0]));
        imageops::replace(&mut framed, &text_like_page(), 60, 40);
        let cropped = crop_dark_borders(&DynamicImage::ImageLuma8(framed));
        assert_eq!((cropped.width(), cropped.height()), (600, 800));
    }

    #[test]
    fn shadows_are_lifted_without_losing_ink() {
        let clean = text_like_page();
        let mut shaded = clean.clone();
        for (x, _, pixel) in shaded.enumerate_pixels_mut() {
            // Paper darkens from white on the left to mid-grey on the right.
            let paper = 255.0 - 110.0 * x as f32 / 600.0;
            pixel[0] = (pixel[0] as f32 * paper / 255.0) as u8;
        }
        let flat = flatten_background(&DynamicImage::ImageLuma8(shaded), 270.0).to_luma8();
        // A patch of bare paper near the dark edge is close to white again.
        assert!(
            flat.get_pixel(580, 20)[0] > 235,
            "paper {}",
            flat.get_pixel(580, 20)[0]
        );
        // Ink is still ink.
        assert!(flat.get_pixel(80, 85)[0] < 40);
        let ratio = ink_count(&flat) as f32 / ink_count(&clean) as f32;
        assert!((0.9..1.1).contains(&ratio), "ink ratio {ratio}");
    }

    #[test]
    fn blank_pages_are_recognised() {
        assert!(is_blank(&GrayImage::from_pixel(300, 400, Luma([250]))));
        assert!(!is_blank(&text_like_page()));
    }

    #[test]
    fn blank_page_removal_drops_the_page_only_when_enabled() {
        let blank = DynamicImage::ImageLuma8(GrayImage::from_pixel(300, 400, Luma([250])));
        let mut settings = ScanSettings::default();
        assert!(clean_page(blank.clone(), &settings).is_some());
        settings.optimize_opts.remove_blank_pages = true;
        assert!(clean_page(blank, &settings).is_none());
    }
}
