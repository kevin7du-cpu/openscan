//! Synthetic import/recovery/real-OCR/PDF check. Never operates scanner hardware.
use image::{Rgb, RgbImage};
use scan_module::{
    CancellationToken, ScanProfile, documents::pdf, processing, session::ScanSession,
};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let output = arguments.next().map(PathBuf::from).ok_or_else(|| {
        anyhow::anyhow!("Usage: verify_workflow NEW_OUTPUT_DIRECTORY MODELS_DIRECTORY")
    })?;
    let models = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Supply the verified OCR model directory"))?;
    anyhow::ensure!(arguments.next().is_none(), "Too many arguments");
    anyhow::ensure!(!output.exists(), "Choose a new output directory");
    std::fs::create_dir_all(&output)?;
    let sample = sample_page();
    let first = output.join("Synthetic letter.png");
    let second = output.join("Second page.png");
    let blank = output.join("Blank.png");
    processing::save_png_with_dpi(&sample, &first, 150)?;
    processing::save_png_with_dpi(&sample, &second, 150)?;
    processing::save_png_with_dpi(
        &RgbImage::from_pixel(1240, 1754, Rgb([255, 255, 255])),
        &blank,
        150,
    )?;
    let mut profile = ScanProfile {
        name: "OpenScan synthetic verification".into(),
        dpi: 150,
        searchable_pdf: true,
        ..Default::default()
    };
    profile.cleanup.skip_blank_pages = true;
    let cancel = CancellationToken::default();
    let mut session = ScanSession::create(output.join("session"))?;
    session.import_images(&[first, second, blank], &profile, &cancel, &mut |p| {
        println!("{p}");
    })?;
    anyhow::ensure!(session.pages.len() == 3, "Import page count");
    anyhow::ensure!(session.pages[2].excluded, "Blank page exclusion");
    let recovered = ScanSession::load(&session.directory)?;
    let bytes = pdf::export_session(&recovered, &profile, &models, &cancel, &mut |p| {
        println!("{p}");
    })?;
    let document = lopdf::Document::load_mem(&bytes)?;
    anyhow::ensure!(document.get_pages().len() == 2, "PDF page count");
    let text = document.extract_text(&[1, 2])?;
    anyhow::ensure!(text.contains("TEST-4827"), "OCR identifier missing: {text}");
    let pdf_path = output.join("OpenScan-searchable.pdf");
    pdf::save_new(&pdf_path, &bytes)?;
    anyhow::ensure!(pdf::save_new(&pdf_path, &bytes).is_err(), "Overwrite guard");
    recovered.export_images(&output.join("PNG-pages"), &cancel)?;
    println!(
        "PASS: three-page recovery, blank exclusion, real OCR, two-page searchable PDF, no overwrite, and PNG export: {}",
        pdf_path.display()
    );
    Ok(())
}

fn sample_page() -> RgbImage {
    let font = ab_glyph::FontArc::try_from_slice(include_bytes!("../assets/Hack-Regular.ttf"))
        .expect("bundled font");
    let mut image = RgbImage::from_pixel(1240, 1754, Rgb([255, 255, 255]));
    for (index, text) in [
        "OPENSCAN SAMPLE DOCUMENT",
        "Synthetic data - no real patient",
        "",
        "Document type: Office letter",
        "Date: 2026-11-08",
        "",
        "Invoice number: TEST-4827",
        "Amount: 125.50 USD",
        "",
        "OCR should find TEST-4827.",
        "Review all text before export.",
    ]
    .iter()
    .enumerate()
    {
        imageproc::drawing::draw_text_mut(
            &mut image,
            Rgb([25, 32, 42]),
            75,
            100 + index as i32 * 75,
            38.0,
            &font,
            text,
        );
    }
    image
}
