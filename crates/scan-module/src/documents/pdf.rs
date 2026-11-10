//! PDF 1.7 image pages with an invisible, Unicode-mapped text layer.
use super::{ExportDocument, ExportPage};
use crate::{CancellationToken, ScanProfile, processing, session::ScanSession};
use anyhow::{Result, ensure};
use lopdf::{
    Document as Pdf, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::{collections::BTreeMap, io::Write, path::Path};

const FONT: &[u8] = include_bytes!("../../assets/Hack-Regular.ttf");
fn utf16(text: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

pub fn export(
    document: &ExportDocument,
    jpeg_quality: Option<u8>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    ensure!(
        !document.pages.is_empty() && document.pages.len() <= 250,
        "Include 1–250 pages for export"
    );
    ensure!(
        jpeg_quality.is_none_or(|q| (20..=100).contains(&q)),
        "JPEG quality must be 20–100"
    );
    cancel.check()?;
    let face = ttf_parser::Face::parse(FONT, 0)?;
    let scale = 1000.0 / face.units_per_em() as f32;
    let mut glyphs = BTreeMap::new();
    for ch in document.text().chars().filter(|c| !c.is_control()) {
        let glyph = face.glyph_index(ch).ok_or_else(|| {
            anyhow::anyhow!(
                "The export font does not support {ch:?}; correct the text or use a supported font"
            )
        })?;
        glyphs.insert(glyph.0, ch);
    }
    let mut pdf = Pdf::with_version("1.7");
    let pages_id = pdf.new_object_id();
    let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(
        &glyphs.keys().copied().collect::<Vec<_>>(),
    );
    let subset = subsetter::subset(FONT, 0, &remapper)?;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&subset);
    let font_name = format!(
        "{}+Hack-Regular",
        digest[..6]
            .iter()
            .map(|b| (b'A' + b % 26) as char)
            .collect::<String>()
    );
    let font_file = pdf.add_object(Stream::new(
        dictionary! {"Length1"=>subset.len() as i64},
        subset,
    ));
    let bbox = face.global_bounding_box();
    let descriptor=pdf.add_object(dictionary! {
        "Type"=>"FontDescriptor", "FontName"=>Object::Name(font_name.clone().into_bytes()), "Flags"=>33, "ItalicAngle"=>0,
        "Ascent"=>face.ascender() as f32*scale, "Descent"=>face.descender() as f32*scale,
        "CapHeight"=>face.capital_height().unwrap_or(face.ascender()) as f32*scale,
        "StemV"=>80, "FontBBox"=>vec![(bbox.x_min as f32*scale).into(),(bbox.y_min as f32*scale).into(),(bbox.x_max as f32*scale).into(),(bbox.y_max as f32*scale).into()], "FontFile2"=>font_file
    });
    let mut widths: Vec<Object> = Vec::new();
    for &gid in glyphs.keys() {
        widths.push((remapper.get(gid).unwrap() as i64).into());
        widths.push(Object::Array(vec![
            (face
                .glyph_hor_advance(ttf_parser::GlyphId(gid))
                .unwrap_or(600) as f32
                * scale)
                .into(),
        ]));
    }
    let descendant=pdf.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"CIDFontType2", "BaseFont"=>Object::Name(font_name.clone().into_bytes()),
        "CIDSystemInfo"=>dictionary!{"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},
        "FontDescriptor"=>descriptor, "CIDToGIDMap"=>"Identity", "DW"=>600, "W"=>widths
    });
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /OpenScanUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries = glyphs.iter().collect::<Vec<_>>();
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            let unicode = ch
                .to_string()
                .encode_utf16()
                .map(|n| format!("{n:04X}"))
                .collect::<String>();
            cmap.push_str(&format!(
                "<{:04X}> <{unicode}>\n",
                remapper.get(**gid).unwrap()
            ));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let unicode = pdf.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
    let font=pdf.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type0","BaseFont"=>Object::Name(font_name.into_bytes()),"Encoding"=>"Identity-H","DescendantFonts"=>vec![descendant.into()],"ToUnicode"=>unicode});
    let mut pages = Vec::<Object>::new();
    for page in &document.pages {
        cancel.check()?;
        ensure!((72..=1200).contains(&page.dpi), "Invalid page DPI");
        let image = processing::load(&page.image)?;
        ensure!(
            image.width() == page.width && image.height() == page.height,
            "Page dimensions changed; refresh before export"
        );
        let k = 72.0 / page.dpi as f32;
        let width = page.width as f32 * k;
        let height = page.height as f32 * k;
        let mut picture_dictionary = dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>page.width as i64,"Height"=>page.height as i64,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8};
        let picture = if let Some(quality) = jpeg_quality {
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
                .encode_image(&image)?;
            picture_dictionary.set("Filter", "DCTDecode");
            Stream::new(picture_dictionary, bytes)
        } else {
            lossless_image(image, cancel)?
        };
        let picture = pdf.add_object(picture);
        let mut operations = vec![
            Operation::new("q", vec![]),
            Operation::new(
                "cm",
                vec![
                    width.into(),
                    0.into(),
                    0.into(),
                    height.into(),
                    0.into(),
                    0.into(),
                ],
            ),
            Operation::new("Do", vec!["PageImage".into()]),
            Operation::new("Q", vec![]),
        ];
        for line in &page.lines {
            ensure!(
                line.bounds.iter().all(|v| v.is_finite())
                    && line.bounds[2] > 0.0
                    && line.bounds[3] > 0.0,
                "Invalid OCR text coordinates"
            );
            let text = line
                .text
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>();
            if text.is_empty() {
                continue;
            }
            let size = (line.bounds[3] * k).max(1.0);
            let mut encoded = Vec::new();
            let mut advance = 0.0;
            for ch in text.chars() {
                let gid = face
                    .glyph_index(ch)
                    .ok_or_else(|| anyhow::anyhow!("Unsupported character"))?;
                encoded.extend(remapper.get(gid.0).unwrap().to_be_bytes());
                advance += face.glyph_hor_advance(gid).unwrap_or(600) as f32
                    / face.units_per_em() as f32
                    * size;
            }
            let horizontal_scale = 100.0 * line.bounds[2] * k / advance.max(0.1);
            operations.extend([
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["Text".into(), size.into()]),
                Operation::new("Tr", vec![3.into()]),
                Operation::new("Tz", vec![horizontal_scale.into()]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        (line.bounds[0] * k).into(),
                        (height - (line.bounds[1] + line.bounds[3] * 0.85) * k).into(),
                    ],
                ),
                Operation::new(
                    "Tj",
                    vec![Object::String(encoded, lopdf::StringFormat::Hexadecimal)],
                ),
                Operation::new("ET", vec![]),
            ]);
        }
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            Content { operations }.encode()?,
        ));
        let fonts = if page.lines.is_empty() {
            dictionary! {}
        } else {
            dictionary! {"Text"=>font}
        };
        let id=pdf.add_object(dictionary!{"Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),width.into(),height.into()],"Contents"=>content,"Resources"=>dictionary!{"Font"=>fonts,"XObject"=>dictionary!{"PageImage"=>picture}}});
        pages.push(id.into());
    }
    pdf.objects.insert(
        pages_id,
        dictionary! {"Type"=>"Pages","Count"=>pages.len() as i64,"Kids"=>pages}.into(),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
    let info=pdf.add_object(dictionary!{"Title"=>utf16(&document.title),"Subject"=>utf16(&document.document_type),"Keywords"=>utf16(&document.tags),"Creator"=>Object::string_literal("OpenScan Rust"),"Producer"=>Object::string_literal("scan-module / lopdf")});
    pdf.trailer.set("Root", root);
    pdf.trailer.set("Info", info);
    pdf.prune_objects();
    pdf.compress();
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes)?;
    cancel.check()?;
    Ok(bytes)
}

/// Save new output atomically. Existing documents are never replaced.
pub fn save_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)?;
    Ok(())
}
pub fn export_session(
    session: &ScanSession,
    profile: &ScanProfile,
    models: &Path,
    cancel: &CancellationToken,
    progress: &mut dyn FnMut(String),
) -> Result<Vec<u8>> {
    profile.validate()?;
    let mut ocr = None;
    let mut pages = Vec::new();
    for (index, page) in session.pages.iter().filter(|p| !p.excluded).enumerate() {
        cancel.check()?;
        progress(format!("Preparing PDF page {}", index + 1));
        let path = session.resolve(&page.image_path)?;
        let image = processing::load(&path)?;
        let lines = if profile.searchable_pdf {
            super::ocr::session_words(session, page, models, &mut ocr, cancel)?
        } else {
            Vec::new()
        };
        pages.push(ExportPage {
            image: path,
            width: image.width(),
            height: image.height(),
            dpi: page.dpi,
            lines,
        });
    }
    export(
        &ExportDocument {
            title: profile.name.clone(),
            document_type: "Scanned document".into(),
            tags: String::new(),
            pages,
        },
        None, // Lossless compression; never downsample or recompress scan pixels as JPEG.
        cancel,
    )
}

/// Encode exact pixels for PDF. Bilevel pages choose the smallest lossless
/// JBIG2/Group4/Flate stream; gray/color use Flate. No pixel substitution.
pub fn lossless_image(image: image::RgbImage, cancel: &CancellationToken) -> Result<Stream> {
    cancel.check()?;
    let (width, height) = image.dimensions();
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= processing::MAX_PIXELS,
        "Invalid PDF image size"
    );
    let (pixels, grayscale, bits) = lossless_pixels(image);
    let mut stream = Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>width as i64,
            "Height"=>height as i64, "ColorSpace"=>if grayscale {"DeviceGray"} else {"DeviceRGB"},
            "BitsPerComponent"=>bits
        },
        pixels,
    );
    let fax = if bits == 1 {
        let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
        for row in stream.content.chunks_exact(width.div_ceil(8) as usize) {
            cancel.check()?;
            encoder.encode_line(
                (0..width).map(|x| {
                    if row[x as usize / 8] & (0x80 >> (x % 8)) == 0 {
                        fax::Color::Black
                    } else {
                        fax::Color::White
                    }
                }),
                width,
            )?;
        }
        Some(encoder.finish()?.finish())
    } else {
        None
    };
    let jbig2 = if bits == 1 {
        cancel.check()?;
        super::bilevel::encode_verified(&stream.content, width, height).ok()
    } else {
        None
    };
    stream.compress()?;
    // Account for the extra DecodeParms dictionary when comparing codecs.
    if let Some(bytes) = fax.filter(|bytes| bytes.len() + 100 < stream.content.len()) {
        stream.set_content(bytes);
        stream.dict.set("Filter", "CCITTFaxDecode");
        stream.dict.set(
            "DecodeParms",
            dictionary! {
                "K"=>-1, "Columns"=>width as i64, "Rows"=>height as i64,
                "BlackIs1"=>false, "EndOfBlock"=>true
            },
        );
    }
    if let Some(bytes) = jbig2.filter(|bytes| bytes.len() + 30 < stream.content.len()) {
        stream.set_content(bytes);
        stream.dict.set("Filter", "JBIG2Decode");
        stream.dict.remove(b"DecodeParms");
    }
    cancel.check()?;
    Ok(stream)
}

fn lossless_pixels(image: image::RgbImage) -> (Vec<u8>, bool, i64) {
    if image.pixels().all(|p| p[0] == p[1] && p[1] == p[2]) {
        if image.pixels().all(|p| p[0] == 0 || p[0] == 255) {
            let stride = image.width().div_ceil(8) as usize;
            let mut packed = vec![0_u8; stride * image.height() as usize];
            for (x, y, p) in image.enumerate_pixels() {
                if p[0] == 255 {
                    packed[y as usize * stride + x as usize / 8] |= 0x80 >> (x % 8);
                }
            }
            (packed, true, 1)
        } else {
            (image.pixels().map(|p| p[0]).collect(), true, 8)
        }
    } else {
        (image.into_raw(), false, 8)
    }
}

/// Copy page trees into a new PDF, retaining inherited page attributes.
/// Document-level forms, bookmarks and signatures are not copied to the new catalog.
pub fn append_copy(
    source: &Path,
    additional: &[u8],
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    ensure!(
        std::fs::metadata(source)?.len() <= 256 * 1024 * 1024,
        "Existing PDF exceeds 256 MB"
    );
    let mut originals = Pdf::load(source)?;
    ensure!(
        !originals.is_encrypted(),
        "Encrypted PDFs cannot be appended"
    );
    let mut scanned = Pdf::load_mem(additional)?;
    let original_count = originals.get_pages().len();
    let scanned_count = scanned.get_pages().len();
    ensure!(
        original_count + scanned_count <= 2000,
        "Combined PDF exceeds 2000 pages"
    );
    cancel.check()?;
    originals.renumber_objects_with(1);
    scanned.renumber_objects_with(originals.max_id + 1);
    let old_pages = originals.catalog()?.get(b"Pages")?.as_reference()?;
    let new_pages = scanned.catalog()?.get(b"Pages")?.as_reference()?;
    let mut combined = Pdf::with_version("1.7");
    combined.max_id = scanned.max_id;
    combined.objects.extend(originals.objects);
    combined.objects.extend(scanned.objects);
    let parent = combined.new_object_id();
    combined
        .get_object_mut(old_pages)?
        .as_dict_mut()?
        .set("Parent", parent);
    combined
        .get_object_mut(new_pages)?
        .as_dict_mut()?
        .set("Parent", parent);
    combined.objects.insert(parent,dictionary!{"Type"=>"Pages","Kids"=>vec![old_pages.into(),new_pages.into()],"Count"=>(original_count+scanned_count) as i64}.into());
    let root = combined.add_object(dictionary! {"Type"=>"Catalog","Pages"=>parent});
    combined.trailer.set("Root", root);
    let mut bytes = Vec::new();
    combined.save_to(&mut bytes)?;
    cancel.check()?;
    Ok(bytes)
}
