pub fn decoded_samples(stream: &lopdf::Stream, width: u32, height: u32) -> Vec<u8> {
    let filter = stream
        .dict
        .get(b"Filter")
        .and_then(|o| o.as_name())
        .unwrap_or(b"");
    if filter == b"JBIG2Decode" {
        struct Pixels {
            data: Vec<u8>,
            width: u32,
            x: u32,
            y: u32,
        }
        impl hayro_jbig2::Decoder for Pixels {
            fn push_pixel(&mut self, black: bool) {
                if !black {
                    self.data[self.y as usize * self.width.div_ceil(8) as usize
                        + self.x as usize / 8] |= 0x80 >> (self.x % 8);
                }
                self.x += 1;
            }
            fn push_pixel_chunk(&mut self, black: bool, n: u32) {
                for _ in 0..n * 8 {
                    self.push_pixel(black);
                }
            }
            fn next_line(&mut self) {
                assert_eq!(self.x, self.width);
                self.x = 0;
                self.y += 1;
            }
        }
        let mut pixels = Pixels {
            data: vec![0; width.div_ceil(8) as usize * height as usize],
            width,
            x: 0,
            y: 0,
        };
        let page =
            hayro_jbig2::Page::new_embedded(&stream.content, None, &Default::default()).unwrap();
        page.decode(&mut pixels).unwrap();
        assert_eq!(pixels.y, height);
        pixels.data
    } else if filter == b"CCITTFaxDecode" {
        let mut data = vec![0; width.div_ceil(8) as usize * height as usize];
        let mut y = 0;
        fax::decoder::decode_g4(
            stream.content.iter().copied(),
            width,
            Some(height),
            |line| {
                for (x, c) in fax::decoder::pels(line, width).enumerate() {
                    if c == fax::Color::White {
                        data[y * width.div_ceil(8) as usize + x / 8] |= 0x80 >> (x % 8);
                    }
                }
                y += 1;
            },
        )
        .unwrap();
        assert_eq!(y, height as usize);
        data
    } else if filter.is_empty() {
        stream.content.clone()
    } else {
        stream.decompressed_content().unwrap()
    }
}
