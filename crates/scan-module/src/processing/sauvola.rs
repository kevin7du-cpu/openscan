//! Balanced document binarization using Sauvola's local mean/variance formula.
//! See https://github.com/DanBloomberg/leptonica/blob/master/src/binarize.c.
//! This independent Rust implementation uses rolling sums, not full-page maps.
use crate::{CancellationToken, Result, ScanError};
use image::GrayImage;

const STRIPE_WIDTH: usize = 4096;
const K: f64 = 0.34;
const R: f64 = 128.0;

#[derive(Clone, Copy, Default)]
struct Moments {
    sum: u64,
    squared: u64,
}

fn update_columns<const ADD: bool>(columns: &mut [Moments], row: &[u8]) {
    for (column, &value) in columns.iter_mut().zip(row) {
        let value = u64::from(value);
        if ADD {
            column.sum += value;
            column.squared += value * value;
        } else {
            column.sum -= value;
            column.squared -= value * value;
        }
    }
}

/// Convert grayscale document pixels to black/white with balanced Sauvola.
///
/// Uses k=0.34, R=128 and a DPI-scaled radius (25 at 300 DPI: a 51-pixel
/// window). Windows clip at image edges. Exact black stays black, including
/// solid fills and already-bilevel input; no OCR or character replacement occurs.
///
/// Apart from the output image, scratch storage is bounded to 4,196 pairs of
/// u64 values (about 66 KiB at 600 DPI). Cancellation is checked each row,
/// including initialization. The caller should run this on a background worker.
/// Input pixels are never modified. Gray/color modes should retain their normal
/// rendering when photographs or faint shading must be preserved.
pub fn balanced_black_white(
    gray: &GrayImage,
    dpi: u32,
    cancel: &CancellationToken,
) -> Result<GrayImage> {
    cancel.check()?;
    let (width, height) = gray.dimensions();
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > super::MAX_PIXELS
        || !(75..=600).contains(&dpi)
    {
        return Err(ScanError::InvalidRequest(
            "Balanced cleanup requires a nonempty image up to 60 megapixels and 75-600 DPI".into(),
        ));
    }
    let (width, height) = (width as usize, height as usize);
    let radius = (25.0 * f64::from(dpi) / 300.0).round() as usize;
    let source = gray.as_raw();
    let mut output = GrayImage::new(width as u32, height as u32);
    let pixels = output.as_mut();

    // Each stripe includes a halo so internal boundaries give exactly the same
    // result as a single full-image window. Memory stays bounded for wide scans.
    for start in (0..width).step_by(STRIPE_WIDTH) {
        let end = (start + STRIPE_WIDTH).min(width);
        let left = start.saturating_sub(radius);
        let right = (end + radius).min(width);
        let mut columns = vec![Moments::default(); right - left];
        for y in 0..(radius + 1).min(height) {
            cancel.check()?;
            update_columns::<true>(&mut columns, &source[y * width + left..y * width + right]);
        }
        for y in 0..height {
            cancel.check()?;
            if y > 0 {
                if y > radius {
                    let row = y - radius - 1;
                    update_columns::<false>(
                        &mut columns,
                        &source[row * width + left..row * width + right],
                    );
                }
                if y + radius < height {
                    let row = y + radius;
                    update_columns::<true>(
                        &mut columns,
                        &source[row * width + left..row * width + right],
                    );
                }
            }
            let row_count = (y + radius + 1).min(height) - y.saturating_sub(radius);
            let mut window = Moments::default();
            for column in &columns[..(start + radius + 1).min(width) - left] {
                window.sum += column.sum;
                window.squared += column.squared;
            }
            for x in start..end {
                let count = row_count * ((x + radius + 1).min(width) - x.saturating_sub(radius));
                let mean = window.sum as f64 / count as f64;
                let variance = (window.squared as f64 / count as f64 - mean * mean).max(0.0);
                let threshold = mean * (1.0 + K * (variance.sqrt() / R - 1.0));
                let value = source[y * width + x];
                // A uniform black neighborhood has mean=threshold=0. Preserve
                // its ink rather than turning solid fills or binary pages white.
                pixels[y * width + x] = if value == 0 || f64::from(value) < threshold {
                    0
                } else {
                    255
                };
                if x + 1 < end {
                    if x >= radius {
                        let column = columns[x - radius - left];
                        window.sum -= column.sum;
                        window.squared -= column.squared;
                    }
                    if x + radius + 1 < width {
                        let column = columns[x + radius + 1 - left];
                        window.sum += column.sum;
                        window.squared += column.squared;
                    }
                }
            }
        }
    }
    cancel.check()?;
    Ok(output)
}
