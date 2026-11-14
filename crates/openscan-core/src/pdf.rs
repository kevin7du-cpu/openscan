// SPDX-License-Identifier: MIT OR Apache-2.0
//! Writing PDFs: a small image-only writer, and appending pages to an existing PDF.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use fax::encoder::Encoder;
use fax::{Color, VecWriter};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::cleanup::Page;

/// How a page image is compressed inside the PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    /// JPEG: for grey and colour pages. Small, at the cost of slight softening.
    Jpeg { grey: bool },
    /// CCITT Group 4 (fax) coding: for pure black-and-white pages. Very small and exact.
    Fax,
}

/// One page prepared for a PDF: compressed data plus what the PDF needs to place it.
pub struct EncodedPage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Size of the page on paper, in points (72 to the inch).
    pub width_pt: f32,
    pub height_pt: f32,
    pub coding: Coding,
}

/// Compresses a page in the smallest way that suits it.
///
/// Pure black-and-white pages are stored exactly, at full resolution, with fax coding.
/// Grey and colour pages are stored as JPEG at `quality` (1 to 100), first reduced to
/// `max_dpi` when that is non-zero and lower than the page's own resolution.
pub fn encode_page(page: &Page, quality: u8, max_dpi: u32) -> Result<EncodedPage> {
    let dpi = page.dpi.max(1);
    let width_pt = page.image.width() as f32 * 72.0 / dpi as f32;
    let height_pt = page.image.height() as f32 * 72.0 / dpi as f32;

    if let DynamicImage::ImageLuma8(grey) = &*page.image
        && grey.pixels().all(|pixel| pixel[0] == 0 || pixel[0] == 255)
    {
        return Ok(EncodedPage {
            data: encode_fax(grey)?,
            width: grey.width(),
            height: grey.height(),
            width_pt,
            height_pt,
            coding: Coding::Fax,
        });
    }

    let reduced;
    let image = if max_dpi > 0 && max_dpi < dpi {
        let scale = max_dpi as f32 / dpi as f32;
        let width = ((page.image.width() as f32 * scale).round() as u32).max(1);
        let height = ((page.image.height() as f32 * scale).round() as u32).max(1);
        reduced = page.image.resize_exact(width, height, FilterType::Lanczos3);
        &reduced
    } else {
        &page.image
    };
    let mut data = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut data, quality.clamp(1, 100));
    image
        .write_with_encoder(encoder)
        .context("encoding page as JPEG")?;
    Ok(EncodedPage {
        data,
        width: image.width(),
        height: image.height(),
        width_pt,
        height_pt,
        coding: Coding::Jpeg {
            grey: matches!(image, DynamicImage::ImageLuma8(_)),
        },
    })
}

/// Compresses a black-and-white image (0 is ink, anything else paper) with fax coding.
fn encode_fax(image: &GrayImage) -> Result<Vec<u8>> {
    let width = u16::try_from(image.width()).context("page is too wide for fax coding")?;
    let mut encoder = Encoder::new(VecWriter::new());
    for row in image.rows() {
        let colours = row.map(|pixel| {
            if pixel[0] == 0 {
                Color::Black
            } else {
                Color::White
            }
        });
        // Writing into memory cannot fail.
        let Ok(()) = encoder.encode_line(colours, width);
    }
    let Ok(writer) = encoder.finish();
    Ok(writer.finish())
}

/// The entries of a PDF image dictionary that depend on how the page is compressed.
fn coding_entries(page: &EncodedPage) -> String {
    match page.coding {
        Coding::Jpeg { grey: true } => {
            "/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode".to_owned()
        }
        Coding::Jpeg { grey: false } => {
            "/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode".to_owned()
        }
        Coding::Fax => format!(
            "/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode \
             /DecodeParms << /K -1 /Columns {} /Rows {} >>",
            page.width, page.height
        ),
    }
}

/// The same image dictionary as `coding_entries`, for use with `lopdf`.
fn image_dictionary(page: &EncodedPage) -> Dictionary {
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"XObject".to_vec()));
    dictionary.set("Subtype", Object::Name(b"Image".to_vec()));
    dictionary.set("Width", i64::from(page.width));
    dictionary.set("Height", i64::from(page.height));
    let (space, bits, filter): (&[u8], i64, &[u8]) = match page.coding {
        Coding::Jpeg { grey: true } => (b"DeviceGray", 8, b"DCTDecode"),
        Coding::Jpeg { grey: false } => (b"DeviceRGB", 8, b"DCTDecode"),
        Coding::Fax => (b"DeviceGray", 1, b"CCITTFaxDecode"),
    };
    dictionary.set("ColorSpace", Object::Name(space.to_vec()));
    dictionary.set("BitsPerComponent", bits);
    dictionary.set("Filter", Object::Name(filter.to_vec()));
    if page.coding == Coding::Fax {
        let mut parameters = Dictionary::new();
        parameters.set("K", -1_i64);
        parameters.set("Columns", i64::from(page.width));
        parameters.set("Rows", i64::from(page.height));
        dictionary.set("DecodeParms", parameters);
    }
    dictionary
}

/// Puts each page's image behind the hidden text of a text-only PDF made by OCR.
///
/// Recognising text and compressing the picture are kept apart this way: the text is read
/// from the untouched scan, and the picture can then be made as small as wanted.
pub fn add_page_images(text_pdf: &Path, pages: &[EncodedPage], output: &Path) -> Result<()> {
    let mut document =
        Document::load(text_pdf).with_context(|| format!("opening {}", text_pdf.display()))?;
    let page_ids: Vec<ObjectId> = document.get_pages().into_values().collect();
    if page_ids.len() != pages.len() {
        bail!(
            "OCR returned {} page(s) for {} scanned",
            page_ids.len(),
            pages.len()
        );
    }
    const IMAGE_NAME: &str = "ScanPage";
    for (page_id, page) in page_ids.into_iter().zip(pages) {
        let image = Stream::new(image_dictionary(page), page.data.clone()).with_compression(false);
        let image_id = document.add_object(image);
        let draw = format!(
            "q {:.2} 0 0 {:.2} 0 0 cm /{IMAGE_NAME} Do Q\n",
            page.width_pt, page.height_pt
        );
        let draw_id = document.add_object(Stream::new(Dictionary::new(), draw.into_bytes()));

        // The page's resources may sit in the page itself or in an object of their own.
        let resources_id = match document.get_dictionary(page_id)?.get(b"Resources") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        };
        let resources = match resources_id {
            Some(id) => document.get_object_mut(id)?.as_dict_mut()?,
            None => {
                let page_dictionary = document.get_object_mut(page_id)?.as_dict_mut()?;
                if !page_dictionary.has(b"Resources") {
                    page_dictionary.set("Resources", Dictionary::new());
                }
                page_dictionary.get_mut(b"Resources")?.as_dict_mut()?
            }
        };
        if !resources.has(b"XObject") {
            resources.set("XObject", Dictionary::new());
        }
        resources
            .get_mut(b"XObject")?
            .as_dict_mut()?
            .set(IMAGE_NAME, image_id);

        // Draw the picture first, so the text layer stays on top of it.
        let page_dictionary = document.get_object_mut(page_id)?.as_dict_mut()?;
        let mut contents = vec![Object::Reference(draw_id)];
        match page_dictionary.get(b"Contents") {
            Ok(Object::Array(existing)) => contents.extend(existing.iter().cloned()),
            Ok(existing) => contents.push(existing.clone()),
            Err(_) => {}
        }
        page_dictionary.set("Contents", contents);
    }
    document
        .save(output)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

/// Writes a PDF with one full-page image per page and no text layer.
///
/// The compressed data is stored as it is, which PDF supports directly.
pub fn write_image_pdf(pages: &[EncodedPage], output: &Path) -> Result<()> {
    if pages.is_empty() {
        bail!("no pages to write");
    }
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    // Byte position of each object, needed for the table at the end of the file.
    let mut offsets: Vec<usize> = Vec::new();
    let mut begin_object = |out: &mut Vec<u8>| {
        offsets.push(out.len());
        offsets.len()
    };

    // Objects 1 and 2 are the catalog and page list; each page then uses three objects.
    let page_object = |index: usize| 3 + index * 3;
    let number = begin_object(&mut out);
    write!(
        out,
        "{number} 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n"
    )?;
    let number = begin_object(&mut out);
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", page_object(i)))
        .collect();
    write!(
        out,
        "{number} 0 obj\n<< /Type /Pages /Count {} /Kids [{}] >>\nendobj\n",
        pages.len(),
        kids.join(" ")
    )?;

    for page in pages {
        let (width_pt, height_pt) = (page.width_pt, page.height_pt);

        let number = begin_object(&mut out);
        write!(
            out,
            "{number} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width_pt:.2} {height_pt:.2}] \
             /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>\nendobj\n",
            number + 2,
            number + 1
        )?;

        let content = format!("q {width_pt:.2} 0 0 {height_pt:.2} 0 0 cm /Im0 Do Q");
        let number = begin_object(&mut out);
        write!(
            out,
            "{number} 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len()
        )?;

        let coding = coding_entries(page);
        let number = begin_object(&mut out);
        write!(
            out,
            "{number} 0 obj\n<< /Type /XObject /Subtype /Image /Width {} /Height {} {coding} /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.data.len()
        )?;
        out.extend_from_slice(&page.data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    let table_start = out.len();
    write!(out, "xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1)?;
    for offset in &offsets {
        writeln!(out, "{offset:010} 00000 n ")?;
    }
    write!(
        out,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table_start}\n%%EOF\n",
        offsets.len() + 1
    )?;

    std::fs::write(output, out).with_context(|| format!("writing {}", output.display()))
}

/// Dictionary entries a page picked up from its parents: key and value pairs.
type InheritedEntries = Vec<(Vec<u8>, Object)>;

/// Writes to `output` a PDF holding every page of `base` followed by every page of `addition`.
pub fn append_pdf(base: &Path, addition: &Path, output: &Path) -> Result<()> {
    let mut document =
        Document::load(base).with_context(|| format!("opening {}", base.display()))?;
    let mut extra =
        Document::load(addition).with_context(|| format!("opening {}", addition.display()))?;

    // Give the added objects numbers that cannot clash with the existing ones.
    extra.renumber_objects_with(document.max_id + 1);
    let new_pages: Vec<ObjectId> = extra.get_pages().into_values().collect();
    if new_pages.is_empty() {
        bail!("{} has no pages", addition.display());
    }

    // Settings a page may inherit from its parents must travel with the page.
    const INHERITED: [&[u8]; 4] = [b"MediaBox", b"Resources", b"Rotate", b"CropBox"];
    let mut carried: Vec<(ObjectId, InheritedEntries)> = Vec::new();
    for &page_id in &new_pages {
        let mut found = Vec::new();
        for key in INHERITED {
            let mut current = Some(page_id);
            while let Some(id) = current {
                let dictionary = extra.get_dictionary(id)?;
                if let Ok(value) = dictionary.get(key) {
                    if id != page_id {
                        found.push((key.to_vec(), value.clone()));
                    }
                    break;
                }
                current = dictionary
                    .get(b"Parent")
                    .and_then(Object::as_reference)
                    .ok();
            }
        }
        carried.push((page_id, found));
    }

    let page_list = document.catalog()?.get(b"Pages")?.as_reference()?;
    document.max_id = extra.max_id;
    document.objects.extend(extra.objects);

    for (page_id, inherited) in carried {
        let page = document.get_object_mut(page_id)?.as_dict_mut()?;
        page.set("Parent", page_list);
        for (key, value) in inherited {
            page.set(key, value);
        }
    }
    let list = document.get_object_mut(page_list)?.as_dict_mut()?;
    let count = list.get(b"Count")?.as_i64()?;
    list.set("Count", count + new_pages.len() as i64);
    let kids = list.get_mut(b"Kids")?.as_array_mut()?;
    kids.extend(new_pages.iter().map(|&id| Object::Reference(id)));

    // Drop the added file's own catalog and page list, which nothing refers to now.
    document.prune_objects();
    document
        .save(output)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

/// Records a title, the producing program and the creation time inside the PDF at `path`.
pub fn add_metadata(path: &Path) -> Result<()> {
    add_named_metadata(path, path)
}

pub fn add_named_metadata(path: &Path, destination: &Path) -> Result<()> {
    let mut document =
        Document::load(path).with_context(|| format!("opening {}", path.display()))?;
    let title = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut info = Dictionary::new();
    info.set("Title", Object::string_literal(title));
    info.set(
        "Producer",
        Object::string_literal(concat!("OpenScan ", env!("CARGO_PKG_VERSION"))),
    );
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    info.set("CreationDate", Object::string_literal(pdf_date(seconds)));
    let info_id = document.add_object(info);
    document.trailer.set("Info", info_id);
    document
        .save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Formats seconds since 1970 (UTC) the way PDF files write dates.
fn pdf_date(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|s| chrono::DateTime::<chrono::Utc>::from_timestamp(s, 0))
        .unwrap_or(chrono::DateTime::UNIX_EPOCH)
        .format("D:%Y%m%d%H%M%SZ")
        .to_string()
}

/// Number of pages in a PDF.
#[cfg_attr(not(test), allow(dead_code))]
pub fn page_count(path: &Path) -> Result<usize> {
    Ok(Document::load(path)?.get_pages().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_dates_are_formatted_in_utc() {
        assert_eq!(pdf_date(0), "D:19700101000000Z");
        // 13 November 2026, 09:30:05 UTC.
        assert_eq!(pdf_date(1_794_562_205), "D:20261113093005Z");
        // 29 February of a leap year.
        assert_eq!(pdf_date(1_709_164_800), "D:20240229000000Z");
    }
    use image::{Luma, RgbImage};

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("scan-module-pdf-{}-{name}", std::process::id()))
    }

    fn grey_page() -> Page {
        Page::from_image(
            DynamicImage::ImageLuma8(GrayImage::from_pixel(850, 1100, Luma([240]))),
            100,
        )
    }

    fn colour_page() -> Page {
        Page::from_image(
            DynamicImage::ImageRgb8(RgbImage::from_pixel(425, 550, image::Rgb([200, 220, 240]))),
            50,
        )
    }

    #[test]
    fn black_and_white_pages_use_fax_coding_and_stay_small() {
        // Black bars on white, like lines of text, at letter size and 100 dpi.
        let bars = GrayImage::from_fn(850, 1100, |x, y| {
            Luma([if y % 40 < 12 && x % 90 < 70 { 0 } else { 255 }])
        });
        let page = Page::from_image(DynamicImage::ImageLuma8(bars), 100);
        let fax = encode_page(&page, 50, 50).unwrap();
        assert_eq!(fax.coding, Coding::Fax);
        // Kept at full resolution even though a lower one was asked for.
        assert_eq!((fax.width, fax.height), (850, 1100));
        assert!(fax.data.len() < 5_000, "{} bytes", fax.data.len());

        let path = temp("fax.pdf");
        write_image_pdf(&[fax], &path).unwrap();
        assert_eq!(page_count(&path).unwrap(), 1);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn grey_pages_are_reduced_but_keep_their_paper_size() {
        let full = encode_page(&grey_page(), 50, 0).unwrap();
        let half = encode_page(&grey_page(), 50, 50).unwrap();
        assert_eq!(half.coding, Coding::Jpeg { grey: true });
        assert_eq!((half.width, half.height), (425, 550));
        assert_eq!(
            (half.width_pt, half.height_pt),
            (full.width_pt, full.height_pt)
        );
    }

    #[test]
    fn image_pdf_has_the_right_pages_and_size() {
        let path = temp("image.pdf");
        let pages = [
            encode_page(&grey_page(), 75, 0).unwrap(),
            encode_page(&colour_page(), 75, 0).unwrap(),
        ];
        write_image_pdf(&pages, &path).unwrap();

        let document = Document::load(&path).unwrap();
        assert_eq!(document.get_pages().len(), 2);
        // 850 pixels at 100 dpi is 8.5 inches, which is 612 points.
        let first = document.get_dictionary(document.get_pages()[&1]).unwrap();
        let media_box = first.get(b"MediaBox").unwrap().as_array().unwrap();
        assert_eq!(media_box[2].as_float().unwrap().round(), 612.0);
        assert_eq!(media_box[3].as_float().unwrap().round(), 792.0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn appending_adds_pages_after_the_existing_ones() {
        let (base, addition, merged) = (temp("base.pdf"), temp("add.pdf"), temp("merged.pdf"));
        write_image_pdf(&[encode_page(&grey_page(), 75, 0).unwrap()], &base).unwrap();
        let two = [
            encode_page(&colour_page(), 75, 0).unwrap(),
            encode_page(&colour_page(), 75, 0).unwrap(),
        ];
        write_image_pdf(&two, &addition).unwrap();

        append_pdf(&base, &addition, &merged).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 3);
        // Appending onto the result again keeps working.
        append_pdf(&merged, &addition, &base).unwrap();
        assert_eq!(page_count(&base).unwrap(), 5);
        for path in [base, addition, merged] {
            std::fs::remove_file(path).unwrap();
        }
    }
}
