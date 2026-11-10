//! Conservative, OCR-guided foreground/background separation.
//!
//! Recognition supplies context, never a licence to erase unknown content.
//! Pixels outside text areas and ambiguous regions remain exactly unchanged.
use super::clean_copy::CleanOptions;
use crate::CancellationToken;
use anyhow::Result;
use image::{Rgb, RgbImage};
use std::collections::VecDeque;

const MAX_PATCH_PIXELS: usize = 1_048_576;
const MAX_ANALYSIS_PIXELS: usize = 64_000_000;

pub(super) struct TextRegion {
    pub bounds: [u32; 4],
    pub characters: usize,
}

struct Speck {
    points: Vec<usize>,
    bounds: [usize; 4],
}

fn tones(pixel: &Rgb<u8>) -> (u8, u8) {
    let low = *pixel.0.iter().min().unwrap();
    (low, pixel.0.iter().max().unwrap() - low)
}

pub(super) fn clean(
    original: &RgbImage,
    regions: &[TextRegion],
    dpi: u32,
    options: CleanOptions,
    cancel: &CancellationToken,
) -> Result<RgbImage> {
    let mut output = original.clone();
    if !options.remove_specks && !options.reduce_shadows {
        cancel.check()?;
        return Ok(output);
    }
    let mut budget = MAX_ANALYSIS_PIXELS;
    let mut visited_regions = std::collections::BTreeSet::new();
    for region in regions {
        cancel.check()?;
        if !visited_regions.insert(region.bounds) || region.characters == 0 {
            continue;
        }
        let [l, t, r, b] = region.bounds;
        let (rw, rh) = (r - l, b - t);
        // A barcode or picture misread as one character is not a text line.
        // Large or ambiguous regions are retained, not subjected to a fallback.
        if rw as f64 > region.characters as f64 * f64::from(rh) * 2.5 {
            continue;
        }
        let scale = dpi as f32 / 300.0;
        let guard = (rh as f32 * 0.45).max(6.0 * scale).ceil().clamp(2.0, 64.0) as u32;
        let [pl, pt, pr, pb] = [
            l.saturating_sub(guard + 2),
            t.saturating_sub(guard + 2),
            (r + guard + 2).min(original.width()),
            (b + guard + 2).min(original.height()),
        ];
        let (w, h) = ((pr - pl) as usize, (pb - pt) as usize);
        let area = w * h;
        if area > MAX_PATCH_PIXELS {
            continue;
        }
        if area > budget {
            break;
        }
        budget -= area;
        let inside = |x: usize, y: usize| {
            let (x, y) = (x as u32 + pl, y as u32 + pt);
            x >= l && x < r && y >= t && y < b
        };
        let mut histogram = [0usize; 256];
        let mut colored = 0;
        for y in t..b {
            cancel.check()?;
            for x in l..r {
                let (tone, chroma) = tones(original.get_pixel(x, y));
                histogram[tone as usize] += 1;
                colored += usize::from(chroma > 16);
            }
        }
        let region_area = rw as usize * rh as usize;
        let mut count = 0;
        let paper = histogram
            .iter()
            .position(|n| {
                count += n;
                count >= region_area * 95 / 100
            })
            .unwrap_or(255) as u8;
        if paper < 185 || colored * 50 > region_area {
            continue;
        }
        let floor = paper.saturating_sub(7);
        // Smooth photographic gradations and dense halftones are ambiguous.
        // Require a predominantly uniform, light neutral paper field.
        let paper_pixels: usize = histogram[floor as usize..].iter().sum();
        if paper_pixels * 100 < region_area * 60 {
            continue;
        }

        // 0 paper, 1 unseen ink, 2 retained content, 3 tiny neutral candidate.
        let mut mask = vec![0u8; area];
        for y in 0..h {
            cancel.check()?;
            for x in 0..w {
                let (tone, chroma) = tones(original.get_pixel(pl + x as u32, pt + y as u32));
                mask[y * w + x] = u8::from(tone < floor || chroma > 16);
            }
        }
        let max_side = (3.0 * scale).round().max(1.0) as usize;
        let max_area = (8.0 * scale * scale).round().max(1.0) as usize;
        let block = (16.0 * scale).round().max(8.0) as usize;
        let grid_w = w.div_ceil(block);
        let mut density = vec![0u32; grid_w * h.div_ceil(block)];
        for (index, &ink) in mask.iter().enumerate() {
            if ink != 0 {
                density[((index / w) / block) * grid_w + (index % w) / block] += 1;
            }
        }
        let mut candidates = Vec::new();
        let mut queue = VecDeque::new();
        let mut anchor = false;
        for seed in 0..area {
            if seed.is_multiple_of(w) {
                cancel.check()?;
            }
            if mask[seed] != 1 {
                continue;
            }
            let (x, y) = (seed % w, seed / w);
            let (mut a, mut c, mut d, mut e) = (x, y, x, y);
            let mut points = Vec::with_capacity(max_area + 1);
            let (mut pixels, mut neutral, mut contained) = (0usize, true, true);
            queue.push_back(seed);
            mask[seed] = 2;
            while let Some(index) = queue.pop_front() {
                pixels += 1;
                if pixels.is_multiple_of(4096) {
                    cancel.check()?;
                }
                let (x, y) = (index % w, index / w);
                if points.len() <= max_area {
                    points.push(index);
                }
                a = a.min(x);
                c = c.min(y);
                d = d.max(x);
                e = e.max(y);
                contained &= inside(x, y);
                neutral &= tones(original.get_pixel(pl + x as u32, pt + y as u32)).1 <= 16;
                for ny in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let next = ny * w + nx;
                        if mask[next] == 1 {
                            mask[next] = 2;
                            queue.push_back(next);
                        }
                    }
                }
            }
            if neutral
                && contained
                && pixels <= max_area
                && d - a < max_side
                && e - c < max_side
                && candidates.len() < 20_000
            {
                for &index in &points {
                    mask[index] = 3;
                }
                candidates.push(Speck {
                    points,
                    bounds: [a, c, d, e],
                });
            } else if contained && pixels > max_area && e - c + 1 >= (rh as usize / 4).max(3) {
                anchor = true;
            }
        }
        // Without substantial ink supporting the OCR result, keep the entire
        // region. This also protects isolated punctuation and faint annotations.
        if !anchor {
            continue;
        }
        if options.remove_specks {
            for speck in candidates {
                cancel.check()?;
                let [a, c, d, e] = speck.bounds;
                let g = guard as usize;
                let near_content = (c.saturating_sub(g)..=(e + g).min(h - 1)).any(|y| {
                    (a.saturating_sub(g)..=(d + g).min(w - 1)).any(|x| mask[y * w + x] == 2)
                });
                // Decimal points, accents, dots and nearby unknown ink survive.
                if near_content {
                    continue;
                }
                // Dense small components may be a halftone, stippling or a
                // diagram texture. Keep them even if OCR called the area text.
                let textured = (c.saturating_sub(block) / block..=(e + block).min(h - 1) / block)
                    .any(|gy| {
                        (a.saturating_sub(block) / block..=(d + block).min(w - 1) / block).any(
                            |gx| {
                                let pixels = block.min(w - gx * block) * block.min(h - gy * block);
                                density[gy * grid_w + gx] as usize * 25 > pixels
                            },
                        )
                    });
                if textured {
                    continue;
                }
                for index in speck.points {
                    let (x, y) = (index % w, index / w);
                    output.put_pixel(pl + x as u32, pt + y as u32, Rgb([paper; 3]));
                }
            }
        }
        if options.reduce_shadows {
            for y in (t - pt) as usize..(b - pt) as usize {
                cancel.check()?;
                for x in (l - pl) as usize..(r - pl) as usize {
                    if mask[y * w + x] != 0 {
                        continue;
                    }
                    let halo = scale.ceil().max(1.0) as usize;
                    let near_ink = (y.saturating_sub(halo)..=(y + halo).min(h - 1)).any(|v| {
                        (x.saturating_sub(halo)..=(x + halo).min(w - 1))
                            .any(|u| mask[v * w + u] != 0)
                    });
                    if near_ink {
                        continue;
                    }
                    let p = original.get_pixel(pl + x as u32, pt + y as u32);
                    let (tone, chroma) = tones(p);
                    // Match surrounding paper rather than create a white box
                    // on a gray page. Never darken highlights or erase color.
                    if chroma <= 4 && tone >= floor && tone < paper {
                        output.put_pixel(pl + x as u32, pt + y as u32, Rgb([paper; 3]));
                    }
                }
            }
        }
    }
    cancel.check()?;
    Ok(output)
}
