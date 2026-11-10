use image::{Rgb, RgbImage};
use scan_module::{CancellationToken, ScanColor, ScanProfile, processing};

fn profile(dpi: u32) -> ScanProfile {
    let mut profile = ScanProfile {
        dpi,
        ..Default::default()
    };
    profile.cleanup.deskew = false;
    profile.cleanup.auto_crop = false;
    profile.cleanup.denoise = true;
    profile
}
fn rect(image: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, value: [u8; 3]) {
    for v in y..y + h {
        for u in x..x + w {
            image.put_pixel(u, v, Rgb(value));
        }
    }
}

#[test]
fn removes_clustered_specks_but_preserves_punctuation_lines_and_colored_marks() {
    for factor in [1, 2] {
        let mut image = RgbImage::from_pixel(400 * factor, 400 * factor, Rgb([255; 3]));
        // Larger-than-median-kernel dust, close enough that older isolation tests failed.
        for x in [40, 48, 56] {
            rect(
                &mut image,
                x * factor,
                40 * factor,
                4 * factor,
                4 * factor,
                [0; 3],
            );
        }
        // Thin letters, dot above i, decimal point and underline must be exact.
        rect(
            &mut image,
            100 * factor,
            150 * factor,
            2 * factor,
            30 * factor,
            [0; 3],
        );
        rect(
            &mut image,
            100 * factor,
            143 * factor,
            2 * factor,
            2 * factor,
            [0; 3],
        );
        rect(
            &mut image,
            112 * factor,
            177 * factor,
            3 * factor,
            3 * factor,
            [0; 3],
        );
        rect(
            &mut image,
            125 * factor,
            150 * factor,
            2 * factor,
            30 * factor,
            [0; 3],
        );
        rect(
            &mut image,
            95 * factor,
            188 * factor,
            60 * factor,
            factor,
            [0; 3],
        );
        rect(
            &mut image,
            300 * factor,
            40 * factor,
            3 * factor,
            3 * factor,
            [20, 40, 180],
        );
        let before = image.clone();
        let result = processing::correct(
            &image,
            &profile(300 * factor),
            0,
            &CancellationToken::default(),
        )
        .unwrap()
        .image;
        for x in [40, 48, 56] {
            assert_eq!(result.get_pixel(x * factor, 40 * factor), &Rgb([255; 3]));
        }
        for y in 130 * factor..200 * factor {
            for x in 90 * factor..160 * factor {
                assert_eq!(result.get_pixel(x, y), image.get_pixel(x, y));
            }
        }
        assert_eq!(
            result.get_pixel(300 * factor, 40 * factor),
            image.get_pixel(300 * factor, 40 * factor)
        );
        assert_eq!(image, before);
    }
}

#[test]
fn preserves_dense_halftone_and_cancels_without_mutating_input() {
    let mut image = RgbImage::from_pixel(400, 400, Rgb([255; 3]));
    for y in 96..192 {
        for x in 96..192 {
            if x % 2 == 0 && y % 2 == 0 {
                image.put_pixel(x, y, Rgb([0; 3]));
            }
        }
    }
    let before = image.clone();
    let result =
        processing::correct(&image, &profile(300), 0, &CancellationToken::default()).unwrap();
    assert_eq!(result.image, image);
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(processing::correct(&image, &profile(300), 0, &cancel).is_err());
    assert_eq!(image, before);
}

#[test]
fn removes_noise_created_by_black_white_conversion_and_is_optional() {
    let mut image = RgbImage::from_pixel(400, 400, Rgb([255; 3]));
    // Balanced thresholding already suppresses lighter paper mottling. Darker
    // dust still needs the optional component filter after binarization.
    rect(&mut image, 40, 40, 4, 4, [120; 3]);
    rect(&mut image, 100, 150, 2, 30, [0; 3]);
    let mut p = profile(300);
    p.color = ScanColor::BlackWhite;
    p.cleanup.denoise = false;
    let off = processing::correct(&image, &p, 0, &CancellationToken::default())
        .unwrap()
        .image;
    assert_eq!(off.get_pixel(40, 40), &Rgb([0; 3]));
    p.cleanup.denoise = true;
    let on = processing::correct(&image, &p, 0, &CancellationToken::default())
        .unwrap()
        .image;
    assert_eq!(on.get_pixel(40, 40), &Rgb([255; 3]));
    assert_eq!(on.get_pixel(100, 150), &Rgb([0; 3]));
}
