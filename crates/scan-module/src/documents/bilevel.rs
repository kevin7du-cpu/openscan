//! Generic-region JBIG2 only. No symbol matching, substitution or thresholding.
use anyhow::{Result, ensure};

pub(super) fn encode_verified(packed: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let stride = width.div_ceil(8) as usize;
    let binary = (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| {
                u8::from(packed[y as usize * stride + x as usize / 8] & (0x80 >> (x % 8)) == 0)
            })
        })
        .collect::<Vec<_>>();
    let mut encoded = jbig2enc_rust::encode_single_image_lossless(&binary, width, height, true)?;
    ensure!(
        encoded.global_data.as_ref().is_none_or(Vec::is_empty),
        "Unexpected JBIG2 symbol dictionary"
    );
    normalize_terminators(&mut encoded.page_data)?;
    // Verify every output pixel with an independent decoder before accepting it.
    // Any codec error falls back to the already encoded Group4/Flate image.
    let page = hayro_jbig2::Page::new_embedded(
        &encoded.page_data,
        None,
        &hayro_jbig2::DecodeSettings { strict: true },
    )?;
    ensure!(
        page.width() == width && page.height() == height,
        "JBIG2 dimensions changed"
    );
    let mut verifier = Verifier {
        packed,
        width,
        height,
        x: 0,
        y: 0,
        equal: true,
    };
    page.decode(&mut verifier)?;
    ensure!(
        verifier.equal && verifier.y == height && verifier.x == 0,
        "JBIG2 pixels changed"
    );
    Ok(encoded.page_data)
}

// jbig2enc-rust 0.5.4 flush(true) emits FF FF AC when its pending byte is
// already FF. The arithmetic terminator is FF AC; the duplicate makes Poppler
// report an extraneous segment byte. Normalize only this known encoder shape,
// update its declared length, then independently verify every decoded pixel.
// This is not a parser/repair function for imported PDFs or arbitrary JBIG2.
fn normalize_terminators(data: &mut Vec<u8>) -> Result<()> {
    let mut position = 0;
    while position < data.len() {
        ensure!(
            data.len() - position >= 11,
            "Truncated generated JBIG2 header"
        );
        let kind = data[position + 4] & 63;
        ensure!(
            data[position + 4] & 64 == 0 && data[position + 5] >> 5 == 0 && matches!(kind, 38 | 48),
            "Unexpected generated JBIG2 segment"
        );
        let length =
            u32::from_be_bytes(data[position + 7..position + 11].try_into().unwrap()) as usize;
        let start = position + 11;
        ensure!(
            length <= data.len() - start,
            "Truncated generated JBIG2 segment"
        );
        let mut end = start + length;
        if kind == 38
            && length >= 29
            && data[start + 17] & 1 == 0
            && data[end - 3..end] == [0xff, 0xff, 0xac]
        {
            data.remove(end - 3);
            data[position + 7..position + 11].copy_from_slice(&((length - 1) as u32).to_be_bytes());
            end -= 1;
        }
        position = end;
    }
    Ok(())
}

struct Verifier<'a> {
    packed: &'a [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    equal: bool,
}
impl hayro_jbig2::Decoder for Verifier<'_> {
    fn push_pixel(&mut self, black: bool) {
        if self.x >= self.width || self.y >= self.height {
            self.equal = false;
            return;
        }
        let value = self.packed
            [self.y as usize * self.width.div_ceil(8) as usize + self.x as usize / 8]
            & (0x80 >> (self.x % 8))
            == 0;
        self.equal &= value == black;
        self.x += 1;
    }
    fn push_pixel_chunk(&mut self, black: bool, count: u32) {
        if count > self.width.div_ceil(8) {
            self.equal = false;
            return;
        }
        for _ in 0..count * 8 {
            self.push_pixel(black);
        }
    }
    fn next_line(&mut self) {
        self.equal &= self.x == self.width;
        self.x = 0;
        self.y += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_arithmetic_marker_is_normalized_without_truncating_other_data() {
        let mut data = vec![0u8; 11 + 29];
        data[4] = 38;
        data[6] = 1;
        data[7..11].copy_from_slice(&29u32.to_be_bytes());
        data[37..40].copy_from_slice(&[0xff, 0xff, 0xac]);
        normalize_terminators(&mut data).unwrap();
        assert_eq!(data.len(), 39);
        assert_eq!(&data[37..], &[0xff, 0xac]);
        assert_eq!(u32::from_be_bytes(data[7..11].try_into().unwrap()), 28);
        let normalized = data.clone();
        normalize_terminators(&mut data).unwrap();
        assert_eq!(data, normalized);
        assert!(normalize_terminators(&mut vec![0; 7]).is_err());
    }
    #[test]
    fn generic_jbig2_preserves_odd_width_noise_blank_solid_and_text_pixels() {
        for pattern in 0..4 {
            let (width, height) = (517_u32, 400_u32);
            let mut packed = vec![0u8; width.div_ceil(8) as usize * height as usize];
            for y in 0..height {
                for x in 0..width {
                    let white = match pattern {
                        0 => true,
                        1 => false,
                        2 => (x.wrapping_mul(71) ^ y.wrapping_mul(131)) % 17 < 8,
                        _ => y % 41 > 18 || x % 19 < 5,
                    };
                    if white {
                        packed[y as usize * width.div_ceil(8) as usize + x as usize / 8] |=
                            0x80 >> (x % 8);
                    }
                }
            }
            assert!(!encode_verified(&packed, width, height).unwrap().is_empty());
        }
    }
}
