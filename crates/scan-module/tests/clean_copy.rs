#![cfg(feature = "documents")]
mod support;
use image::{GenericImageView, Rgb, RgbImage};
use scan_module::{
    CancellationToken,
    documents::{
        TextLine,
        clean_copy::{self, CleanLine},
    },
};

#[test]
fn explicit_rebuild_preserves_graphics_and_unrecognized_paper() {
    let mut original = RgbImage::from_pixel(600, 800, Rgb([245, 245, 242]));
    for y in 80..110 {
        for x in 60..300 {
            original.put_pixel(x, y, Rgb([100, 100, 100]));
        }
    }
    // A photo with varied colors, including bright highlights, must retain its pixels.
    for y in 300..500 {
        for x in 50..250 {
            original.put_pixel(x, y, Rgb([(x % 190 + 40) as u8, (y % 160 + 30) as u8, 90]));
        }
    }
    for y in 640..670 {
        for x in 320..510 {
            original.put_pixel(x, y, Rgb([45, 45, 130]));
        }
    }
    let before = original.clone();
    let lines = vec![
        CleanLine {
            line: TextLine {
                text: "Reviewed TEST-4827".into(),
                bounds: [60., 80., 240., 30.],
            },
            rebuild: true,
        },
        CleanLine {
            line: TextLine {
                text: "signature".into(),
                bounds: [310., 630., 210., 50.],
            },
            rebuild: false,
        },
    ];
    let rendered = clean_copy::render(&original, &lines, &CancellationToken::default()).unwrap();
    assert_eq!(rendered.dimensions(), original.dimensions());
    assert_eq!(rendered.get_pixel(400, 200), original.get_pixel(400, 200));
    assert_ne!(rendered.get_pixel(65, 85), original.get_pixel(65, 85));
    assert!(
        image::imageops::crop_imm(&rendered, 60, 80, 240, 30)
            .pixels()
            .any(|(_, _, p)| p[0] < 20)
    );
    for y in 300..500 {
        for x in 50..250 {
            assert_eq!(rendered.get_pixel(x, y), original.get_pixel(x, y));
        }
    }
    for y in 630..680 {
        for x in 310..520 {
            assert_eq!(rendered.get_pixel(x, y), original.get_pixel(x, y));
        }
    }
    assert_eq!(original, before);
}

#[test]
fn clean_copy_never_automatically_retypes_ocr_guesses() {
    let mut image = RgbImage::from_pixel(400, 300, Rgb([245, 245, 245]));
    for y in 180..260 {
        for x in 20..350 {
            image.put_pixel(x, y, Rgb([30, 80, 150]));
        }
    }
    let lines = vec![
        TextLine {
            text: "Printed text".into(),
            bounds: [20., 30., 200., 30.],
        },
        TextLine {
            text: "Photo caption".into(),
            bounds: [30., 200., 200., 30.],
        },
    ];
    let prepared = clean_copy::prepare(&image, lines).unwrap();
    assert!(!prepared[0].rebuild);
    assert!(!prepared[1].rebuild);
}

#[test]
fn default_cleanup_preserves_barcode_diagonal_ink_punctuation_and_detected_text() {
    let mut image = RgbImage::from_pixel(600, 800, Rgb([242, 242, 242]));
    // Barcode wrongly recognized as one letter: formerly stretched across it.
    for x in (40..350).step_by(6) {
        for y in 40..80 {
            image.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    // Diagonal ink is valid content; direction alone cannot identify a crease.
    for x in 400..460 {
        image.put_pixel(x, 160 + x - 400, Rgb([25, 25, 25]));
    }
    image.put_pixel(462, 221, Rgb([25, 25, 25])); // punctuation near ink
    // Faint text inside an OCR region must survive shadow reduction exactly.
    for y in 300..310 {
        for x in 60..200 {
            image.put_pixel(x, y, Rgb([205, 205, 205]));
        }
    }
    let draft = clean_copy::prepare(
        &image,
        vec![
            TextLine {
                text: "S".into(),
                bounds: [35., 35., 320., 50.],
            },
            TextLine {
                text: "faint".into(),
                bounds: [55., 295., 150., 20.],
            },
        ],
    )
    .unwrap();
    let result = clean_copy::render(&image, &draft, &CancellationToken::default()).unwrap();
    for y in 35..85 {
        for x in 35..355 {
            assert_eq!(result.get_pixel(x, y), image.get_pixel(x, y));
        }
    }
    for y in 295..315 {
        for x in 55..205 {
            assert_eq!(result.get_pixel(x, y), image.get_pixel(x, y));
        }
    }
    for x in 400..460 {
        assert_eq!(
            result.get_pixel(x, 160 + x - 400),
            image.get_pixel(x, 160 + x - 400)
        );
    }
    assert_eq!(result.get_pixel(462, 221), image.get_pixel(462, 221));
    assert_eq!(result.get_pixel(500, 500), image.get_pixel(500, 500));
}

#[test]
fn unrecognized_specks_and_light_marks_are_kept_even_when_cleanup_is_enabled() {
    use clean_copy::CleanOptions;
    let mut image = RgbImage::from_pixel(400, 400, Rgb([245, 245, 245]));
    image.put_pixel(100, 100, Rgb([40, 40, 40]));
    for y in 200..300 {
        for x in 200..300 {
            image.put_pixel(x, y, Rgb([200 + ((x - 200) / 4) as u8; 3]));
        }
    }
    let token = CancellationToken::default();
    let cleaned = clean_copy::render(&image, &[], &token).unwrap();
    assert_eq!(cleaned, image);
    let kept = clean_copy::render_with_options(
        &image,
        &[],
        CleanOptions {
            remove_specks: false,
            reduce_shadows: false,
        },
        &token,
    )
    .unwrap();
    assert_eq!(kept.get_pixel(100, 100), image.get_pixel(100, 100));
    assert_eq!(kept.get_pixel(220, 220), image.get_pixel(220, 220));
}

#[test]
fn explicit_replacement_keeps_normal_character_proportions() {
    let image = RgbImage::from_pixel(800, 200, Rgb([255; 3]));
    let result = clean_copy::render(
        &image,
        &[CleanLine {
            line: TextLine {
                text: "S".into(),
                bounds: [20., 30., 700., 40.],
            },
            rebuild: true,
        }],
        &CancellationToken::default(),
    )
    .unwrap();
    let ink: Vec<_> = result
        .enumerate_pixels()
        .filter(|(_, _, p)| p[0] < 100)
        .collect();
    assert!(!ink.is_empty());
    let width = ink.iter().map(|(x, _, _)| *x).max().unwrap()
        - ink.iter().map(|(x, _, _)| *x).min().unwrap();
    assert!(
        width < 50,
        "one letter must not stretch to 700 pixels: {width}"
    );
}

#[test]
fn clean_copy_rejects_invalid_bounds_and_honors_cancellation() {
    let image = RgbImage::new(100, 100);
    let mut line = CleanLine {
        line: TextLine {
            text: "Test".into(),
            bounds: [10., 10., 80., 20.],
        },
        rebuild: true,
    };
    let token = CancellationToken::default();
    token.cancel();
    assert!(clean_copy::render(&image, &[line.clone()], &token).is_err());
    for bounds in [
        [f32::NAN, 0., 10., 10.],
        [95., 0., 50., 10.],
        [-1., 0., 10., 10.],
        [0., 0., 0., 10.],
    ] {
        line.line.bounds = bounds;
        assert!(
            clean_copy::render(&image, &[line.clone()], &CancellationToken::default()).is_err()
        );
    }
}
#[test]
fn erase_marks_changes_only_selected_pixels_and_validates_before_editing() {
    let mut image = RgbImage::from_pixel(300, 400, Rgb([50; 3]));
    let token = CancellationToken::default();
    let before = image.clone();
    assert!(
        clean_copy::erase_marks(
            &mut image,
            &[[10., 20., 30., 40.], [f32::NAN, 0., 2., 2.]],
            &token
        )
        .is_err()
    );
    assert_eq!(image, before);
    clean_copy::erase_marks(&mut image, &[[10., 20., 30., 40.]], &token).unwrap();
    for (x, y, p) in image.enumerate_pixels() {
        assert_eq!(
            p,
            &if (10..40).contains(&x) && (20..60).contains(&y) {
                Rgb([255; 3])
            } else {
                Rgb([50; 3])
            }
        );
    }
}

#[test]
fn pdf_lossless_color_grayscale_and_packed_black_white_roundtrip_exact_pixels() {
    use scan_module::documents::{ExportDocument, ExportPage, pdf};
    let temp = tempfile::tempdir().unwrap();
    let images = [
        RgbImage::from_fn(517, 800, |x, y| {
            let v = if (x + y) % 3 == 0 { 0 } else { 255 };
            Rgb([v, v, v])
        }),
        RgbImage::from_fn(517, 800, |x, y| {
            let v = ((x + y) % 256) as u8;
            Rgb([v, v, v])
        }),
        RgbImage::from_fn(517, 800, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 90])),
    ];
    let pages = images
        .iter()
        .enumerate()
        .map(|(i, image)| {
            let path = temp.path().join(format!("page-{i}.png"));
            image.save(&path).unwrap();
            ExportPage {
                image: path,
                width: 517,
                height: 800,
                dpi: 300,
                lines: vec![],
            }
        })
        .collect();
    let bytes = pdf::export(
        &ExportDocument {
            title: "Lossless".into(),
            document_type: "Test".into(),
            tags: String::new(),
            pages,
        },
        None,
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(bytes.len() < 517 * 800 * 3);
    let pdf = lopdf::Document::load_mem(&bytes).unwrap();
    let streams = pdf
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Image"))
        .collect::<Vec<_>>();
    assert_eq!(streams.len(), 3);
    for (index, stream) in streams.into_iter().enumerate() {
        assert_ne!(
            stream.dict.get(b"Filter").and_then(|o| o.as_name()).ok(),
            Some(b"DCTDecode".as_slice())
        );
        let data = support::decoded_samples(stream, 517, 800);
        let bits = stream
            .dict
            .get(b"BitsPerComponent")
            .unwrap()
            .as_i64()
            .unwrap();
        assert_eq!(bits, if index == 0 { 1 } else { 8 });
        for (x, y, p) in images[index].enumerate_pixels() {
            let offset = (y * 517 + x) as usize;
            let restored = match index {
                0 => {
                    let v = if data[y as usize * 65 + x as usize / 8] & (0x80 >> (x % 8)) == 0 {
                        0
                    } else {
                        255
                    };
                    [v, v, v]
                }
                1 => [data[offset]; 3],
                _ => [data[offset * 3], data[offset * 3 + 1], data[offset * 3 + 2]],
            };
            assert_eq!(restored, p.0);
        }
    }
}
