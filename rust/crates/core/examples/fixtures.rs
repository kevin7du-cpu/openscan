use image::{Rgb, RgbImage};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args().nth(1).ok_or("output directory required")?;
    let directory = std::path::Path::new(&destination);
    std::fs::create_dir_all(directory)?;
    let mut signature = RgbImage::from_pixel(600, 180, Rgb([255; 3]));
    let points = [
        (30., 140.),
        (90., 25.),
        (75., 125.),
        (160., 80.),
        (145., 130.),
        (210., 90.),
        (195., 130.),
        (280., 40.),
        (250., 135.),
        (410., 95.),
        (540., 130.),
    ];
    for segment in points.windows(2) {
        for offset in 0..3 {
            imageproc::drawing::draw_line_segment_mut(
                &mut signature,
                (segment[0].0, segment[0].1 + offset as f32),
                (segment[1].0, segment[1].1 + offset as f32),
                Rgb([20, 40, 90]),
            );
        }
    }
    signature.save(directory.join("synthetic-signature.png"))?;
    Ok(())
}
