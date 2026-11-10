#![cfg(feature = "documents")]
use image::{Rgb, RgbImage};
use scan_module::{
    CancellationToken,
    documents::{
        TextLine,
        clean_copy::{self, CleanOptions},
    },
};

fn sample() -> (RgbImage, Vec<TextLine>) {
    let mut image = RgbImage::from_pixel(600, 320, Rgb([245; 3]));
    // Synthetic printed strokes, a detached dot/decimal and a faint stroke.
    for x in [215, 232, 295] {
        for y in 116..145 {
            for u in x..x + 3 {
                image.put_pixel(u, y, Rgb([35; 3]));
            }
        }
    }
    image.put_pixel(216, 110, Rgb([35; 3]));
    image.put_pixel(289, 143, Rgb([35; 3]));
    for y in 115..143 {
        image.put_pixel(250, y, Rgb([228; 3]));
    }
    image.put_pixel(410, 115, Rgb([45; 3])); // isolated tiny noise in recognized whitespace
    for y in 129..134 {
        for x in 395..401 {
            image.put_pixel(x, y, Rgb([240; 3]));
        }
    }
    // Unknown content outside recognition: tiny punctuation, signature, rule,
    // faint pencil line and a sparse photograph all need exact preservation.
    image.put_pixel(80, 50, Rgb([0; 3]));
    for x in 30..130 {
        image.put_pixel(x, 60 + (x % 20), Rgb([180; 3]));
    }
    for x in 20..570 {
        image.put_pixel(x, 210, Rgb([15; 3]));
    }
    for x in 30..130 {
        image.put_pixel(x, 240, Rgb([239; 3]));
    }
    for y in 250..300 {
        for x in 450..550 {
            image.put_pixel(x, y, Rgb([(x % 255) as u8, (y % 255) as u8, 180]));
        }
    }
    (
        image,
        vec![TextLine {
            text: "office 0.5 mg".into(),
            bounds: [200., 100., 240., 60.],
        }],
    )
}

#[test]
fn cleans_recognized_gaps_and_keeps_unknown_content_and_actual_ink() {
    let (image, lines) = sample();
    let before = image.clone();
    let draft = clean_copy::prepare(&image, lines).unwrap();
    let result = clean_copy::render(&image, &draft, &CancellationToken::default()).unwrap();
    assert_eq!(result.get_pixel(410, 115), &Rgb([245; 3]));
    assert_eq!(result.get_pixel(398, 131), &Rgb([245; 3]));
    for (x, y, p) in image.enumerate_pixels() {
        if !(200..440).contains(&x) || !(100..160).contains(&y) || x < 380 {
            assert_eq!(result.get_pixel(x, y), p, "Content changed at {x},{y}");
        }
    }
    assert_eq!(image, before);
}

#[test]
fn cleanup_scales_with_dpi_and_independent_options() {
    let (image, lines) = sample();
    let token = CancellationToken::default();
    let draft = clean_copy::prepare(&image, lines.clone()).unwrap();
    for (specks, shadows) in [(false, false), (true, false), (false, true)] {
        let out = clean_copy::render_with_options(
            &image,
            &draft,
            CleanOptions {
                remove_specks: specks,
                reduce_shadows: shadows,
            },
            &token,
        )
        .unwrap();
        assert_eq!(
            out.get_pixel(410, 115),
            if specks {
                &Rgb([245; 3])
            } else {
                image.get_pixel(410, 115)
            }
        );
        assert_eq!(
            out.get_pixel(398, 131),
            if shadows {
                &Rgb([245; 3])
            } else {
                image.get_pixel(398, 131)
            }
        );
        if !specks && !shadows {
            assert_eq!(out, image);
        }
    }
    let larger = image::imageops::resize(&image, 1200, 640, image::imageops::FilterType::Nearest);
    let lines = lines
        .into_iter()
        .map(|mut l| {
            l.bounds = l.bounds.map(|n| n * 2.);
            l
        })
        .collect();
    let draft = clean_copy::prepare(&larger, lines).unwrap();
    let result = clean_copy::render_with_options_at_dpi(
        &larger,
        &draft,
        CleanOptions::default(),
        600,
        &token,
    )
    .unwrap();
    assert_eq!(result.get_pixel(820, 230), &Rgb([245; 3]));
    assert_eq!(result.get_pixel(432, 220), larger.get_pixel(432, 220));
    assert_eq!(result.get_pixel(160, 100), larger.get_pixel(160, 100));
}

#[test]
fn missed_text_pictures_barcodes_and_regions_without_anchors_are_retained() {
    let (image, _) = sample();
    let token = CancellationToken::default();
    assert_eq!(clean_copy::render(&image, &[], &token).unwrap(), image);
    let mut image = RgbImage::from_pixel(500, 500, Rgb([245; 3]));
    for y in 50..180 {
        for x in 30..200 {
            image.put_pixel(x, y, Rgb([(x % 140 + 80) as u8, (y % 150 + 40) as u8, 180]));
        }
    }
    for x in (30..350).step_by(4) {
        for y in 220..260 {
            image.put_pixel(x, y, Rgb([10; 3]));
        }
    }
    image.put_pixel(55, 350, Rgb([0; 3]));
    image.put_pixel(65, 350, Rgb([0; 3]));
    image.put_pixel(75, 350, Rgb([0; 3]));
    let draft = clean_copy::prepare(
        &image,
        vec![
            TextLine {
                text: "Misread photograph".into(),
                bounds: [30., 50., 170., 130.],
            },
            TextLine {
                text: "S".into(),
                bounds: [25., 215., 330., 50.],
            },
            TextLine {
                text: "...".into(),
                bounds: [50., 340., 40., 20.],
            },
        ],
    )
    .unwrap();
    assert_eq!(clean_copy::render(&image, &draft, &token).unwrap(), image);
}

#[test]
fn retains_crossing_rules_and_fails_without_modifying_input() {
    let (mut image, lines) = sample();
    for x in 160..480 {
        image.put_pixel(x, 150, Rgb([0; 3]));
    }
    let draft = clean_copy::prepare(&image, lines).unwrap();
    let token = CancellationToken::default();
    let out = clean_copy::render(&image, &draft, &token).unwrap();
    for x in 160..480 {
        assert_eq!(out.get_pixel(x, 150), image.get_pixel(x, 150));
    }
    let before = image.clone();
    token.cancel();
    assert!(clean_copy::render(&image, &draft, &token).is_err());
    assert!(
        clean_copy::render_with_options_at_dpi(
            &image,
            &draft,
            CleanOptions::default(),
            1200,
            &CancellationToken::default()
        )
        .is_err()
    );
    assert_eq!(image, before);
}

#[test]
fn keeps_halftone_graphics_even_inside_a_misrecognized_text_region() {
    let mut image = RgbImage::from_pixel(320, 200, Rgb([245; 3]));
    for y in 55..105 {
        for x in 35..40 {
            image.put_pixel(x, y, Rgb([30; 3]));
        }
    }
    for y in (70..150).step_by(4) {
        for x in (180..270).step_by(4) {
            image.put_pixel(x, y, Rgb([60; 3]));
        }
    }
    let draft = clean_copy::prepare(
        &image,
        vec![TextLine {
            text: "misrecognized graphic and text".into(),
            bounds: [20., 40., 270., 125.],
        }],
    )
    .unwrap();
    assert_eq!(
        clean_copy::render(&image, &draft, &CancellationToken::default()).unwrap(),
        image
    );
}
