//! Local acceptance: balanced_cleanup INPUT NEW_OUTPUT_DIRECTORY.
//! Produces separate raw-threshold and normal-pipeline images plus a PDF.
use scan_module::{
    CancellationToken, ScanColor, ScanProfile,
    documents::{ExportDocument, ExportPage, pdf},
    processing,
};
use std::{fs, path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(
        args.len() == 2,
        "Usage: balanced_cleanup INPUT NEW_OUTPUT_DIRECTORY"
    );
    let (input, output) = (&args[0], &args[1]);
    anyhow::ensure!(!output.exists(), "Choose a fresh output directory");
    fs::create_dir_all(output)?;
    let original_bytes = fs::read(input)?;
    let image = processing::load(input)?;
    let dpi = processing::resolution(input).unwrap_or(300);
    let token = CancellationToken::default();
    let gray = image::DynamicImage::ImageRgb8(image.clone()).to_luma8();
    let started = Instant::now();
    let balanced = processing::balanced_black_white(&gray, dpi, &token)?;
    let balanced_ms = started.elapsed().as_millis();
    balanced.save(output.join("balanced.png"))?;
    let profile = ScanProfile {
        color: ScanColor::BlackWhite,
        dpi,
        ..Default::default()
    };
    let corrected = processing::correct(&image, &profile, 0, &token)?;
    let path = output.join("corrected.png");
    processing::save_png_with_dpi(&corrected.image, &path, dpi)?;
    let doc = ExportDocument {
        title: "Balanced cleanup verification".into(),
        document_type: "Scan".into(),
        tags: String::new(),
        pages: vec![ExportPage {
            image: path,
            width: corrected.image.width(),
            height: corrected.image.height(),
            dpi,
            lines: vec![],
        }],
    };
    pdf::save_new(
        &output.join("corrected.pdf"),
        &pdf::export(&doc, None, &token)?,
    )?;
    anyhow::ensure!(fs::read(input)? == original_bytes, "Source bytes changed");
    let report = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"), "dpi": dpi,
        "algorithm": "balanced_sauvola", "k": 0.34, "r": 128,
        "source_size": image.dimensions(), "corrected_size": corrected.image.dimensions(),
        "source_unchanged": true, "balanced_ms": balanced_ms,
        "deskew_angle": corrected.angle, "profile": profile,
    });
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!(
        "PASS: balanced Sauvola, normal corrections and PDF export; original unchanged ({balanced_ms} ms for binarization)."
    );
    Ok(())
}
