//! Multipage TIFF import with the same allocation and page limits as scanner input.
use crate::*;
use image::{DynamicImage, ImageBuffer};
use std::{fs::File, io::BufReader, path::Path};
use tiff::{
    ColorType,
    decoder::{Decoder, DecodingResult, Limits},
};

impl Session {
    pub fn import(&mut self, path: &Path, profile: &Profile, cancel: &Cancellation) -> Result<()> {
        profile.validate()?;
        cancel.check()?;
        if std::fs::metadata(path)?.len() > correction::MAX_BYTES {
            return Err(error("Image exceeds 256 MB."));
        }
        let reader = image::ImageReader::open(path)?.with_guessed_format()?;
        if reader.format() != Some(image::ImageFormat::Tiff) {
            return self.add(
                RawPage {
                    path: path.into(),
                    dpi: profile.dpi,
                },
                profile,
                cancel,
            );
        }
        // Preserve the entire source before decoding; a later bad frame leaves it recoverable.
        let directory = self.acquisition_directory()?;
        let source = directory.join("imported-original.tiff");
        std::fs::copy(path, &source)?;
        let mut limits = Limits::default();
        limits.decoding_buffer_size = correction::MAX_PIXELS as usize * 8;
        limits.intermediate_buffer_size = correction::MAX_BYTES as usize;
        let mut decoder = Decoder::new(BufReader::new(File::open(source)?))
            .map_err(|e| error(e.to_string()))?
            .with_limits(limits);
        loop {
            cancel.check()?;
            if self.pages.len() >= 250 {
                return Err(error(
                    "A session can hold 250 pages. Received TIFF pages were kept.",
                ));
            }
            let (w, h) = decoder.dimensions().map_err(|e| error(e.to_string()))?;
            if !(16..=16384).contains(&w)
                || !(16..=16384).contains(&h)
                || u64::from(w) * u64::from(h) > correction::MAX_PIXELS
            {
                return Err(error("TIFF page exceeds the supported image dimensions."));
            }
            let color = decoder.colortype().map_err(|e| error(e.to_string()))?;
            if decoder
                .get_tag_u32(tiff::tags::Tag::PlanarConfiguration)
                .ok()
                == Some(2)
            {
                return Err(error(
                    "Planar TIFF color images are not supported. Convert to interleaved RGB first.",
                ));
            }
            let decoded = decoder.read_image().map_err(|e| error(e.to_string()))?;
            let bytes = match decoded {
                DecodingResult::U8(bytes) => bytes,
                DecodingResult::U16(samples) => {
                    samples.into_iter().map(|s| (s >> 8) as u8).collect()
                }
                _ => {
                    return Err(error(
                        "TIFF import supports integer grayscale and RGB images up to 16 bits per channel.",
                    ));
                }
            };
            let invalid = || error("TIFF pixel data does not match its declared dimensions.");
            let raster = match color {
                ColorType::Gray(bits @ (1 | 2 | 4)) => {
                    let stride = (w as usize * bits as usize).div_ceil(8);
                    if bytes.len() != stride * h as usize {
                        return Err(invalid());
                    }
                    let mask = (1u16 << bits) - 1;
                    let mut unpacked = Vec::with_capacity(w as usize * h as usize);
                    for row in bytes.chunks_exact(stride) {
                        for x in 0..w as usize {
                            let bit = x * bits as usize;
                            let sample =
                                (row[bit / 8] >> (8 - bits as usize - bit % 8)) as u16 & mask;
                            unpacked.push((sample * 255 / mask) as u8);
                        }
                    }
                    DynamicImage::ImageLuma8(
                        ImageBuffer::from_raw(w, h, unpacked).ok_or_else(invalid)?,
                    )
                }
                ColorType::Gray(8 | 16) => DynamicImage::ImageLuma8(
                    ImageBuffer::from_raw(w, h, bytes).ok_or_else(invalid)?,
                ),
                ColorType::GrayA(8 | 16) => DynamicImage::ImageLumaA8(
                    ImageBuffer::from_raw(w, h, bytes).ok_or_else(invalid)?,
                ),
                ColorType::RGB(8 | 16) => {
                    DynamicImage::ImageRgb8(ImageBuffer::from_raw(w, h, bytes).ok_or_else(invalid)?)
                }
                ColorType::RGBA(8 | 16) => DynamicImage::ImageRgba8(
                    ImageBuffer::from_raw(w, h, bytes).ok_or_else(invalid)?,
                ),
                _ => {
                    return Err(error(
                        "Unsupported TIFF color format. Convert it to RGB or grayscale first.",
                    ));
                }
            };
            let page = directory.join(format!("frame-{:04}.png", self.pages.len() + 1));
            raster.save(&page)?;
            self.add(
                RawPage {
                    path: page,
                    dpi: profile.dpi,
                },
                profile,
                cancel,
            )?;
            if !decoder.more_images() {
                break;
            }
            decoder.next_image().map_err(|e| error(e.to_string()))?;
        }
        Ok(())
    }
}
