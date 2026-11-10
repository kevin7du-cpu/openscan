//! Remove small neutral ink islands on near-white paper without blurring ink.
//! No OCR or angle heuristic: nearby text/graphics and larger marks stay intact.
use crate::{CancellationToken, Result};
use image::{Rgb, RgbImage};
use std::collections::VecDeque;

struct Speck {
    seed: u32,
    bounds: [u32; 4],
}

pub(super) fn remove_dust(
    original: &RgbImage,
    dpi: u32,
    cancel: &CancellationToken,
) -> Result<RgbImage> {
    let (w, h) = original.dimensions();
    let scale = f64::from(dpi.clamp(75, 600)) / 300.0;
    let max_area = (60.0 * scale * scale).round().max(2.0) as usize;
    let max_side = (12.0 * scale).round().max(2.0) as u32;
    let guard = (18.0 * scale).round().max(5.0) as u32;
    let cell = (6.0 * scale).round().max(2.0) as u32;
    let grid_w = w.div_ceil(cell);
    let mut tones = [0usize; 256];
    for p in original.pixels() {
        tones[usize::from(*p.0.iter().min().unwrap())] += 1;
    }
    let mut count = 0;
    let background_tone = tones
        .iter()
        .position(|n| {
            count += n;
            count >= (w as usize * h as usize) * 95 / 100
        })
        .unwrap_or(255) as u8;
    if background_tone < 160 {
        return Ok(original.clone());
    }
    let paper_floor = background_tone.saturating_sub(20).min(235);
    // 0 = paper, 1 = unvisited ink, 2 = protected ink, 3 = speck candidate.
    let mut mask: Vec<u8> = original
        .pixels()
        .map(|p| u8::from(*p.0.iter().min().unwrap() < paper_floor))
        .collect();
    let mut candidates = Vec::new();
    let mut queue = VecDeque::new();
    let mut points = Vec::with_capacity(max_area + 1);
    for y in 0..h {
        if y % 32 == 0 {
            cancel.check()?;
        }
        for x in 0..w {
            let seed = y * w + x;
            if mask[seed as usize] != 1 {
                continue;
            }
            points.clear();
            let (mut l, mut t, mut r, mut b) = (x, y, x, y);
            let mut count = 0usize;
            let mut colored = false;
            queue.push_back(seed);
            mask[seed as usize] = 2;
            while let Some(index) = queue.pop_front() {
                count += 1;
                if count.is_multiple_of(4096) {
                    cancel.check()?;
                }
                let (u, v) = (index % w, index / w);
                if points.len() <= max_area {
                    points.push(index);
                }
                l = l.min(u);
                t = t.min(v);
                r = r.max(u);
                b = b.max(v);
                let p = original.get_pixel(u, v);
                colored |= p.0.iter().max().unwrap() - p.0.iter().min().unwrap() > 20;
                for ny in v.saturating_sub(1)..=(v + 1).min(h - 1) {
                    for nx in u.saturating_sub(1)..=(u + 1).min(w - 1) {
                        let next = (ny * w + nx) as usize;
                        if mask[next] == 1 {
                            mask[next] = 2;
                            queue.push_back(next as u32);
                        }
                    }
                }
            }
            // Bound bookkeeping even on pathological noise or dense photographs.
            // Keep excess components; never degrade into indiscriminate filtering.
            if !colored
                && count <= max_area
                && r - l < max_side
                && b - t < max_side
                && candidates.len() < 200_000
            {
                for &index in &points {
                    mask[index as usize] = 3;
                }
                candidates.push(Speck {
                    seed,
                    bounds: [l, t, r, b],
                });
            }
        }
    }
    // A coarse occupancy grid conservatively protects a halo around actual ink,
    // including decimal points, accents, i/j dots, signatures, lines and barcodes.
    // Nearby specks do not protect each other, so noisy blank bands can be cleaned.
    let mut protected = vec![false; (grid_w * h.div_ceil(cell)) as usize];
    let patch = (48.0 * scale).round().max(12.0) as u32;
    let patch_w = w.div_ceil(patch);
    let mut density = vec![0u16; (patch_w * h.div_ceil(patch)) as usize];
    for y in 0..h {
        if y % 64 == 0 {
            cancel.check()?;
        }
        for x in 0..w {
            if mask[(y * w + x) as usize] == 2 {
                protected[((y / cell) * grid_w + x / cell) as usize] = true;
            }
            if mask[(y * w + x) as usize] != 0 {
                density[((y / patch) * patch_w + x / patch) as usize] += 1;
            }
        }
    }
    let mut output = original.clone();
    for (n, speck) in candidates.iter().enumerate() {
        if n % 256 == 0 {
            cancel.check()?;
        }
        let [l, t, r, b] = speck.bounds;
        let near_ink = (t.saturating_sub(guard) / cell..=(b + guard).min(h - 1) / cell).any(|v| {
            (l.saturating_sub(guard) / cell..=(r + guard).min(w - 1) / cell)
                .any(|u| protected[(v * grid_w + u) as usize])
        });
        if near_ink {
            continue;
        }
        // Dense halftone/graphic patches must not be treated as many dust dots.
        if (t / patch..=b / patch).any(|v| {
            (l / patch..=r / patch).any(|u| {
                let area = patch.min(w - u * patch) * patch.min(h - v * patch);
                u32::from(density[(v * patch_w + u) as usize]) * 8 > area
            })
        }) {
            continue;
        }
        // Replace with the surrounding paper tone, not an arbitrary white hole.
        let mut paper = [0u32; 3];
        let mut samples = 0;
        for v in t.saturating_sub(2)..=(b + 2).min(h - 1) {
            for u in l.saturating_sub(2)..=(r + 2).min(w - 1) {
                if u >= l && u <= r && v >= t && v <= b {
                    continue;
                }
                let p = original.get_pixel(u, v);
                if p.0.iter().all(|c| *c >= paper_floor) {
                    for c in 0..3 {
                        paper[c] += u32::from(p[c]);
                    }
                    samples += 1;
                }
            }
        }
        if samples == 0 {
            continue;
        }
        let background = Rgb(paper.map(|c| (c / samples) as u8));
        queue.push_back(speck.seed);
        mask[speck.seed as usize] = 0;
        while let Some(index) = queue.pop_front() {
            let (u, v) = (index % w, index / w);
            output.put_pixel(u, v, background);
            for ny in v.saturating_sub(1)..=(v + 1).min(h - 1) {
                for nx in u.saturating_sub(1)..=(u + 1).min(w - 1) {
                    let next = (ny * w + nx) as usize;
                    if mask[next] == 3 {
                        mask[next] = 0;
                        queue.push_back(next as u32);
                    } else if mask[next] == 0 {
                        // Remove only the very light neutral antialias fringe.
                        let p = original.get_pixel(nx, ny);
                        if p.0.iter().all(|c| *c >= paper_floor)
                            && p.0.iter().max().unwrap() - p.0.iter().min().unwrap() <= 20
                        {
                            output.put_pixel(nx, ny, background);
                        }
                    }
                }
            }
        }
    }
    cancel.check()?;
    Ok(output)
}
