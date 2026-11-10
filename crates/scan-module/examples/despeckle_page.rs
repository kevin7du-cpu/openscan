//! Native, OCR-free dust-removal acceptance: despeckle_page INPUT NEW_OUTPUT_DIR
use scan_module::{
    CancellationToken, ScanProfile,
    documents::{ExportDocument, ExportPage, pdf},
    processing,
};
use std::{fs, path::PathBuf};
fn main() -> anyhow::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(
        args.len() == 2,
        "Usage: despeckle_page INPUT NEW_OUTPUT_DIRECTORY"
    );
    let (input, output) = (&args[0], &args[1]);
    anyhow::ensure!(!output.exists(), "Choose a fresh output directory");
    fs::create_dir_all(output)?;
    let bytes = fs::read(input)?;
    let image = processing::load(input)?;
    let mut profile = ScanProfile::default();
    profile.cleanup.deskew = false;
    profile.cleanup.auto_crop = false;
    profile.cleanup.denoise = true;
    profile.dpi = processing::resolution(input).unwrap_or(300);
    let token = CancellationToken::default();
    let cleaned = processing::correct(&image, &profile, 0, &token)?.image;
    anyhow::ensure!(
        image.dimensions() == cleaned.dimensions(),
        "Dimensions changed"
    );
    let changed = image
        .pixels()
        .zip(cleaned.pixels())
        .filter(|(a, b)| a != b)
        .count();
    let path = output.join("cleaned.png");
    processing::save_png_with_dpi(&cleaned, &path, profile.dpi)?;
    let doc = ExportDocument {
        title: "Dust removal preview".into(),
        document_type: "Scan".into(),
        tags: String::new(),
        pages: vec![ExportPage {
            image: path,
            width: cleaned.width(),
            height: cleaned.height(),
            dpi: profile.dpi,
            lines: vec![],
        }],
    };
    pdf::save_new(
        &output.join("despeckled.pdf"),
        &pdf::export(&doc, None, &token)?,
    )?;
    anyhow::ensure!(fs::read(input)? == bytes, "Source changed");
    println!("PASS: {changed} pixels cleaned, source and dimensions unchanged, no OCR used.");
    Ok(())
}
