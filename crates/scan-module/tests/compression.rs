#![cfg(feature = "documents")]
mod support;
use image::{Rgb, RgbImage};
use scan_module::{CancellationToken, documents::pdf::lossless_image};

#[test]
fn group4_restores_every_pixel_in_odd_width_text_like_image() {
    let image = RgbImage::from_fn(1703, 2200, |x, y| {
        let ink = (90..1600).contains(&x) && (70..2100).contains(&y) && y % 44 < 20 && x % 31 < 13;
        Rgb([if ink { 0 } else { 255 }; 3])
    });
    let stream = lossless_image(image.clone(), &CancellationToken::default()).unwrap();
    // A deliberately repetitive grid can favor Flate. Also exercise Group4
    // directly in its unit test; here decode whichever exact codec won.
    let filter = stream.dict.get(b"Filter").unwrap().as_name().unwrap();
    if filter == b"CCITTFaxDecode" {
        let mut y = 0;
        fax::decoder::decode_g4(stream.content.iter().copied(), 1703, Some(2200), |line| {
            for (x, color) in fax::decoder::pels(line, 1703).enumerate() {
                assert_eq!(
                    image.get_pixel(x as u32, y)[0] == 0,
                    color == fax::Color::Black
                );
            }
            y += 1;
        })
        .unwrap();
        assert_eq!(y, 2200);
    } else {
        assert!(filter == b"FlateDecode" || filter == b"JBIG2Decode");
        let pixels = support::decoded_samples(&stream, 1703, 2200);
        for (x, y, p) in image.enumerate_pixels() {
            assert_eq!(
                p[0] == 255,
                pixels[y as usize * 213 + x as usize / 8] & (0x80 >> (x % 8)) != 0
            );
        }
    }
}

#[test]
fn lossless_encoder_honors_cancellation() {
    let token = CancellationToken::default();
    token.cancel();
    assert!(lossless_image(RgbImage::from_pixel(64, 64, Rgb([255; 3])), &token).is_err());
}
