use image::{GrayImage, Luma};
use scan_module::{CancellationToken, ScanColor, ScanError, ScanProfile, processing};

fn reference(gray: &GrayImage, dpi: u32, x: u32, y: u32) -> u8 {
    // Deliberately direct per-window computation, independent of rolling sums,
    // stripes and their edge bookkeeping in the production implementation.
    let radius = (25.0 * f64::from(dpi) / 300.0).round() as u32;
    let mut values = Vec::new();
    for yy in y.saturating_sub(radius)..=(y + radius).min(gray.height() - 1) {
        for xx in x.saturating_sub(radius)..=(x + radius).min(gray.width() - 1) {
            values.push(f64::from(gray.get_pixel(xx, yy)[0]));
        }
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let threshold = mean * (1.0 + 0.34 * (variance.sqrt() / 128.0 - 1.0));
    let value = gray.get_pixel(x, y)[0];
    if value == 0 || f64::from(value) < threshold {
        0
    } else {
        255
    }
}

#[test]
fn matches_direct_formula_at_edges_dpi_scales_and_stripe_boundaries() {
    for (w, h) in [(63, 45), (1, 7), (9, 1)] {
        let gray = GrayImage::from_fn(w, h, |x, y| {
            Luma([((x * 83 + y * 31 + x * y * 17) % 256) as u8])
        });
        for dpi in [75, 300, 600] {
            let result =
                processing::balanced_black_white(&gray, dpi, &CancellationToken::default())
                    .unwrap();
            for (x, y, pixel) in result.enumerate_pixels() {
                assert_eq!(
                    pixel[0],
                    reference(&gray, dpi, x, y),
                    "{w}x{h} dpi {dpi}, ({x}, {y})"
                );
            }
        }
    }
    let wide = GrayImage::from_fn(8311, 13, |x, y| Luma([((x * 29 + y * 71) % 256) as u8]));
    for dpi in [75, 300, 600] {
        let result =
            processing::balanced_black_white(&wide, dpi, &CancellationToken::default()).unwrap();
        for y in 0..wide.height() {
            for x in [0, 25, 4094, 4095, 4096, 4097, 8191, 8192, 8193, 8310] {
                assert_eq!(result.get_pixel(x, y)[0], reference(&wide, dpi, x, y));
            }
        }
    }
}

#[test]
fn removes_mottled_paper_while_retaining_punctuation_rules_and_diagonal_ink() {
    for factor in [1, 2] {
        let image = GrayImage::from_fn(320 * factor, 240 * factor, |x, y| {
            let (x, y) = (x / factor, y / factor);
            let stroke = (100..103).contains(&x) && (70..105).contains(&y);
            let dot = (100..103).contains(&x) && (63..66).contains(&y);
            let decimal = (110..113).contains(&x) && (101..104).contains(&y);
            let line = (90..170).contains(&x) && y == 115;
            let diagonal = (180..210).contains(&x) && y == x - 100;
            let barcode = (230..270).contains(&x) && (60..110).contains(&y) && x % 5 < 2;
            if stroke || dot || decimal || line || diagonal || barcode {
                Luma([35])
            } else {
                Luma([200 + ((x * 37 + y * 19) % 36) as u8])
            }
        });
        let before = image.clone();
        let cleaned =
            processing::balanced_black_white(&image, 300 * factor, &CancellationToken::default())
                .unwrap();
        for (original, result) in image.pixels().zip(cleaned.pixels()) {
            assert_eq!(result[0], if original[0] == 35 { 0 } else { 255 });
        }
        assert_eq!(image, before);
    }
}

#[test]
fn preserves_binary_pages_solid_black_and_white_and_handles_cancellation() {
    let token = CancellationToken::default();
    for gray in [
        GrayImage::from_pixel(125, 143, Luma([0])),
        GrayImage::from_pixel(125, 143, Luma([255])),
        GrayImage::from_fn(125, 143, |x, y| {
            Luma([if x < 90 || y > 110 { 0 } else { 255 }])
        }),
    ] {
        let before = gray.clone();
        assert_eq!(
            processing::balanced_black_white(&gray, 300, &token).unwrap(),
            gray
        );
        assert_eq!(before, gray);
    }
    token.cancel();
    let gray = GrayImage::from_pixel(100, 100, Luma([190]));
    assert!(matches!(
        processing::balanced_black_white(&gray, 300, &token),
        Err(ScanError::Cancelled)
    ));
    assert!(
        processing::balanced_black_white(&GrayImage::new(0, 1), 300, &CancellationToken::default())
            .is_err()
    );
    assert!(processing::balanced_black_white(&gray, 0, &CancellationToken::default()).is_err());
}

#[test]
fn old_black_white_profiles_use_balanced_cleanup_and_other_modes_keep_their_pixels() {
    let mut profile: ScanProfile = serde_json::from_str(r#"{"color":"blackWhite","dpi":300,"cleanup":{"deskew":false,"autoCrop":false,"denoise":false}}"#).unwrap();
    let image = GrayImage::from_fn(96, 96, |x, y| {
        Luma([if x == 40 && (20..60).contains(&y) {
            40
        } else {
            200 + ((x + y) % 30) as u8
        }])
    });
    let rgb = image::DynamicImage::ImageLuma8(image.clone()).to_rgb8();
    let token = CancellationToken::default();
    let expected = processing::balanced_black_white(&image, 300, &token).unwrap();
    let actual = processing::correct(&rgb, &profile, 0, &token)
        .unwrap()
        .image;
    assert_eq!(image::DynamicImage::ImageRgb8(actual).to_luma8(), expected);
    for color in [ScanColor::Color, ScanColor::Grayscale] {
        profile.color = color;
        assert_eq!(
            processing::correct(&rgb, &profile, 0, &token)
                .unwrap()
                .image,
            rgb
        );
    }
}
