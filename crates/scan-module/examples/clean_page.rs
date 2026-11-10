//! Reusable pipeline demonstration: clean_page INPUT OUTPUT_DIRECTORY MODELS_DIRECTORY
//! Writes new files only. Review the OCR JSON and visual copy before using its text.
use anyhow::{Result, ensure};
use scan_module::{
    CancellationToken, ScanProfile,
    documents::{ExportDocument, ExportPage, clean_copy, ocr::LocalOcr, pdf},
    processing,
};
use std::path::PathBuf;
fn main() -> Result<()> {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    ensure!(
        args.len() == 3,
        "Usage: clean_page INPUT OUTPUT_DIRECTORY MODELS_DIRECTORY"
    );
    let (input, output, models) = (&args[0], &args[1], &args[2]);
    ensure!(!output.exists(), "Choose a fresh output directory");
    std::fs::create_dir_all(output)?;
    let token = CancellationToken::default();
    let input_bytes = std::fs::read(input)?;
    let original = processing::load(input)?;
    let mut profile = ScanProfile::default();
    // Exercise cleanup alone; do not confound its pixel checks with a crop or rotation.
    profile.cleanup.auto_crop = false;
    profile.cleanup.deskew = false;
    profile.cleanup.denoise = false;
    profile.dpi = processing::resolution(input).unwrap_or(300);
    let cropped = processing::correct(&original, &profile, 0, &token)?.image;
    processing::save_png_with_dpi(&cropped, &output.join("cropped.png"), profile.dpi)?;
    println!(
        "Crop: {:?} -> {:?}",
        original.dimensions(),
        cropped.dimensions()
    );
    let lines = LocalOcr::load(models)?.recognize_words(&cropped)?;
    let draft = clean_copy::prepare(&cropped, lines)?;
    let result = clean_copy::render_with_options_at_dpi(
        &cropped,
        &draft,
        Default::default(),
        profile.dpi,
        &token,
    )?;
    ensure!(
        draft.iter().all(|line| !line.rebuild),
        "OCR guesses must not replace ink by default"
    );
    let mut recognized = image::GrayImage::new(cropped.width(), cropped.height());
    for item in &draft {
        let [x, y, w, h] = item.line.bounds;
        for py in y.floor() as u32..((y + h).ceil() as u32).min(result.height()) {
            for px in x.floor() as u32..((x + w).ceil() as u32).min(result.width()) {
                recognized.put_pixel(px, py, image::Luma([1]));
            }
        }
    }
    let mut changed = 0usize;
    let mut outside = 0usize;
    for (x, y, p) in cropped.enumerate_pixels() {
        if recognized.get_pixel(x, y)[0] == 0 {
            outside += 1;
            ensure!(
                result.get_pixel(x, y) == p,
                "Unrecognized content changed at {x},{y}"
            );
        }
        if result.get_pixel(x, y) != p {
            changed += 1;
        }
    }
    std::fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version":env!("CARGO_PKG_VERSION"), "dpi":profile.dpi,
            "ocr_regions":draft.len(),"changed_pixels":changed,
            "unrecognized_pixels_preserved":outside,"original_unchanged":true,
            "dimensions":[result.width(),result.height()]
        }))?,
    )?;
    let image = output.join("clean.png");
    processing::save_png_with_dpi(&result, &image, profile.dpi)?;
    std::fs::write(
        output.join("review.json"),
        serde_json::to_vec_pretty(&draft)?,
    )?;
    println!(
        "Review: {} OCR regions; {} selected for replacement",
        draft.len(),
        draft.iter().filter(|l| l.rebuild).count()
    );
    println!(
        "Content-aware: {changed} changed pixels; {outside} unrecognized pixels preserved exactly"
    );
    let doc = ExportDocument {
        title: "OCR clean copy — review draft".into(),
        document_type: "Scan".into(),
        tags: "draft".into(),
        pages: vec![ExportPage {
            image,
            width: result.width(),
            height: result.height(),
            dpi: profile.dpi,
            lines: draft.into_iter().map(|l| l.line).collect(),
        }],
    };
    let bytes = pdf::export(&doc, None, &token)?;
    pdf::save_new(&output.join("clean-draft.pdf"), &bytes)?;
    let pdf = lopdf::Document::load(output.join("clean-draft.pdf"))?;
    let extracted = pdf.extract_text(&[1])?;
    ensure!(!extracted.trim().is_empty(), "Searchable text is missing");
    std::fs::write(output.join("extracted.txt"), extracted)?;
    ensure!(std::fs::read(input)? == input_bytes, "Source bytes changed");
    println!("Saved image, review data and searchable PDF; original unchanged.");
    Ok(())
}
