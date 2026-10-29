//! Compares PNG compression settings on the sprite sheets (measurement aid).
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use silkie_scramble::gfx::sprites::{self, Sheet};
fn main() {
    let m = sprites::manifest();
    let factor: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut totals = [0usize; 4];
    for spec in &m.sheets {
        let sheet = Sheet::decode(spec, sprites::sheet_png(&spec.name).unwrap()).unwrap().scaled(factor);
        let mut row = vec![];
        for (i, (c, f)) in [(CompressionType::Fast, FilterType::Adaptive), (CompressionType::Default, FilterType::Adaptive), (CompressionType::Best, FilterType::Adaptive)].into_iter().enumerate() {
            let t = std::time::Instant::now();
            let mut out = Vec::new();
            PngEncoder::new_with_quality(&mut out, c, f).write_image(&sheet.rgba, sheet.width, sheet.height, image::ExtendedColorType::Rgba8).unwrap();
            row.push(format!("{:>7} B {:>5.0} ms", out.len(), t.elapsed().as_secs_f64() * 1000.0));
            totals[i] += out.len();
        }
        let z = silkie_scramble::term::kitty_gfx::zlib(&sheet.rgba).len();
        totals[3] += z;
        println!("{:<8} {}x{} raw {} | fast {} | default {} | best {} | zlib-rgba {}", spec.name, sheet.width, sheet.height, sheet.rgba.len(), row[0], row[1], row[2], z);
    }
    println!("totals: fast {} default {} best {} zlib {}", totals[0], totals[1], totals[2], totals[3]);
}
