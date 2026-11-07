//! Image PDF export using the MIT-licensed lopdf writer. PDF processing stays in Rust.
use crate::*;
use lopdf::{
    Document, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::path::Path;

pub fn export(
    pages: &[Page],
    destination: &Path,
    p: &Profile,
    overwrite: bool,
    cancel: &Cancellation,
) -> Result<()> {
    p.validate()?;
    let included: Vec<_> = pages.iter().filter(|p| !p.excluded).collect();
    if included.is_empty() || included.len() > 250 {
        return Err(error("Export needs 1–250 included pages."));
    }
    let ocr = if p.searchable_pdf {
        Some(ocr::Ocr::load(&ocr::model_directory()?)?)
    } else {
        None
    };
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let mut kids = Vec::new();
    for page in included {
        cancel.check()?;
        if !(75..=600).contains(&page.dpi) {
            return Err(error("Page resolution must be between 75 and 600 DPI."));
        }
        let image = correction::read(&page.image_path)?;
        let width = image.width();
        let height = image.height();
        let unit = 72. / f64::from(page.dpi);
        let words = if let Some(engine) = &ocr {
            engine.recognize(&image, cancel)?
        } else {
            Vec::new()
        };
        let (bytes, space) = if p.color == Color::BlackWhite {
            (image.to_luma8().into_raw(), "DeviceGray")
        } else {
            let mut encoded = Vec::new();
            let rgb = image.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, p.jpeg_quality)
                .encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)?;
            (encoded, "DeviceRGB")
        };
        let mut stream = Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>width,"Height"=>height,"ColorSpace"=>space,"BitsPerComponent"=>8},
            bytes,
        );
        if p.color == Color::BlackWhite {
            stream.compress().map_err(|e| error(e.to_string()))?;
        } else {
            stream.dict.set("Filter", "DCTDecode");
        }
        let image_id = doc.add_object(stream);
        let w = f64::from(width) * unit;
        let h = f64::from(height) * unit;
        let mut content = Content {
            operations: vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![w.into(), 0.into(), 0.into(), h.into(), 0.into(), 0.into()],
                ),
                Operation::new("Do", vec![Object::Name(b"Im0".to_vec())]),
                Operation::new("Q", vec![]),
            ],
        };
        let fonts = text_layer(&mut doc, &mut content, &words, unit, h)?;
        let stream = doc.add_object(Stream::new(
            dictionary! {},
            content.encode().map_err(|e| error(e.to_string()))?,
        ));
        let id=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),w.into(),h.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"Im0"=>image_id},"Font"=>fonts},"Contents"=>stream});
        kids.push(Object::Reference(id));
    }
    doc.objects.insert(
        root,
        dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|e| error(e.to_string()))?;
    cancel.check()?;
    storage::atomic_write(destination, &bytes, overwrite)
}

/// Append into a new file, retaining the original catalog, form, and page subtrees.
pub fn append(
    source: &Path,
    pages: &[Page],
    destination: &Path,
    p: &Profile,
    cancel: &Cancellation,
) -> Result<()> {
    if destination.exists() {
        return Err(error(
            "Save the appended PDF to a new filename; existing files are preserved.",
        ));
    }
    if std::fs::metadata(source)?.len() > 512 * 1024 * 1024 {
        return Err(error("The PDF to append to exceeds 512 MB."));
    }
    let mut original = Document::load(source).map_err(|e| error(e.to_string()))?;
    if original.is_encrypted() {
        return Err(error(
            "Unlock the PDF in PdfCraft before appending scanned pages.",
        ));
    }
    if original.objects.values().any(|o| {
        o.as_dict()
            .ok()
            .is_some_and(|d| d.get(b"ByteRange").is_ok())
    }) {
        return Err(error(
            "This PDF contains a digital signature. Append in PdfCraft after reviewing its signing restrictions, or save a separate scan.",
        ));
    }
    let temp = tempfile::tempdir()?;
    let scanned = temp.path().join("scan.pdf");
    export(pages, &scanned, p, false, cancel)?;
    let mut scan = Document::load(scanned).map_err(|e| error(e.to_string()))?;
    scan.renumber_objects_with(original.max_id.saturating_add(1));
    let root = original
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| error(e.to_string()))?;
    let old_pages = original
        .get_dictionary(root)
        .and_then(|d| d.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|e| error(e.to_string()))?;
    let scan_root = scan
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| error(e.to_string()))?;
    let scan_pages = scan
        .get_dictionary(scan_root)
        .and_then(|d| d.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|e| error(e.to_string()))?;
    let count = original
        .get_pages()
        .len()
        .checked_add(scan.get_pages().len())
        .ok_or_else(|| error("Page count overflow."))?;
    original.max_id = scan.max_id;
    original.objects.extend(scan.objects);
    let new_pages=original.add_object(dictionary!{"Type"=>"Pages","Count"=>count as i64,"Kids"=>vec![Object::Reference(old_pages),Object::Reference(scan_pages)]});
    for id in [old_pages, scan_pages] {
        original
            .get_dictionary_mut(id)
            .map_err(|e| error(e.to_string()))?
            .set("Parent", new_pages);
    }
    original
        .get_dictionary_mut(root)
        .map_err(|e| error(e.to_string()))?
        .set("Pages", new_pages);
    let mut bytes = Vec::new();
    original
        .save_to(&mut bytes)
        .map_err(|e| error(e.to_string()))?;
    cancel.check()?;
    storage::atomic_write(destination, &bytes, false)
}

/// Empty Type3 glyphs plus ToUnicode preserve searchable Unicode without a system font.
/// This uses the PDF text-layer technique also used by PdfCraft's engine::ocr::layer.
fn text_layer(
    doc: &mut Document,
    content: &mut Content,
    words: &[ocr::Word],
    unit: f64,
    height: f64,
) -> Result<lopdf::Dictionary> {
    use std::collections::{BTreeMap, BTreeSet};
    let chars: Vec<_> = words
        .iter()
        .flat_map(|w| w.text.chars())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if chars.len() > 4096 {
        return Err(error("OCR text has too many distinct characters."));
    }
    let mut fonts = lopdf::Dictionary::new();
    let mut codes = BTreeMap::new();
    let empty = doc.add_object(Stream::new(
        dictionary! {},
        b"1000 0 0 -200 1000 800 d1\n".to_vec(),
    ));
    for (group, chunk) in chars.chunks(255).enumerate() {
        let name = format!("OCR{group}");
        let mut differences = vec![1.into()];
        let mut procs = lopdf::Dictionary::new();
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (OpenScan) /Ordering (Unicode) /Supplement 0 >> def\n/CMapName /OpenScanOCR def\n/CMapType 2 def\n1 begincodespacerange\n<01> <FF>\nendcodespacerange\n",
        );
        for (batch, part) in chunk.chunks(100).enumerate() {
            cmap.push_str(&format!("{} beginbfchar\n", part.len()));
            for (i, ch) in part.iter().enumerate() {
                let code = (batch * 100 + i + 1) as u8;
                let glyph = format!("g{code:02X}");
                differences.push(Object::Name(glyph.as_bytes().to_vec()));
                procs.set(glyph, empty);
                let mut units = [0u16; 2];
                let hex: String = ch
                    .encode_utf16(&mut units)
                    .iter()
                    .map(|u| format!("{u:04X}"))
                    .collect();
                cmap.push_str(&format!("<{code:02X}> <{hex}>\n"));
                codes.insert(*ch, (name.clone(), code));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let map = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
        let font=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type3","FontBBox"=>vec![0.into(),(-200).into(),1000.into(),800.into()],"FontMatrix"=>vec![0.001.into(),0.into(),0.into(),0.001.into(),0.into(),0.into()],"FirstChar"=>1,"LastChar"=>chunk.len() as i64,"Widths"=>vec![Object::Integer(1000);chunk.len()],"Encoding"=>dictionary!{"Type"=>"Encoding","Differences"=>differences},"CharProcs"=>procs,"Resources"=>dictionary!{},"ToUnicode"=>map});
        fonts.set(name, font);
    }
    content.operations.extend([
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("Tr", vec![3.into()]),
    ]);
    for word in words {
        let count = word.text.chars().count();
        if count == 0 {
            continue;
        }
        let [left, top, right, bottom] = word.rect;
        let size = (bottom - top) * unit;
        if size <= 0. || !size.is_finite() {
            continue;
        }
        let advance = (right - left) * unit / count as f64;
        let mut runs: Vec<(String, usize, Vec<u8>)> = Vec::new();
        for (i, ch) in word.text.chars().enumerate() {
            if let Some((font, code)) = codes.get(&ch) {
                if let Some((last_font, _, bytes)) = runs.last_mut()
                    && last_font == font
                {
                    bytes.push(*code);
                } else {
                    runs.push((font.clone(), i, vec![*code]));
                }
            }
        }
        for (font, i, bytes) in runs {
            content.operations.extend([
                Operation::new(
                    "Tf",
                    vec![Object::Name(font.as_bytes().to_vec()), size.into()],
                ),
                Operation::new("Tz", vec![(100. * advance / size).into()]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        (left * unit + i as f64 * advance).into(),
                        (height - bottom * unit + size * 0.2).into(),
                    ],
                ),
                Operation::new(
                    "Tj",
                    vec![Object::String(bytes, lopdf::StringFormat::Hexadecimal)],
                ),
            ]);
        }
    }
    content.operations.push(Operation::new("ET", vec![]));
    content.operations.push(Operation::new("Q", vec![]));
    Ok(fonts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_text_is_encoded_in_whole_words_with_unicode_mapping() {
        let mut doc = Document::with_version("1.7");
        let mut content = Content {
            operations: Vec::new(),
        };
        let words = [ocr::Word {
            text: "Scan café".into(),
            rect: [10., 10., 190., 40.],
        }];
        let fonts = text_layer(&mut doc, &mut content, &words, 1., 600.).expect("text layer");
        let strings: Vec<_> = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tj")
            .collect();
        assert_eq!(
            strings.len(),
            1,
            "fragmenting every letter breaks search/extraction"
        );
        let root = doc.new_object_id();
        let stream = doc.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("encode"),
        ));
        let page = doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),600.into(),600.into()],"Resources"=>dictionary!{"Font"=>fonts},"Contents"=>stream});
        doc.objects.insert(
            root,
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1}.into(),
        );
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
        doc.trailer.set("Root", catalog);
        // lopdf's extractor prefers Encoding over ToUnicode, contrary to PDF text
        // extraction rules. Verify its CMap decoder explicitly; integration checks
        // also extract the untouched exported PDF with PdfCraft's independent engine.
        for object in doc.objects.values_mut() {
            if let Ok(dict) = object.as_dict_mut()
                && dict.get(b"ToUnicode").is_ok()
            {
                dict.remove(b"Encoding");
            }
        }
        let text = doc.extract_text(&[1]).expect("extract");
        assert!(text.contains("Scan café"), "extracted: {text:?}");
    }
}
