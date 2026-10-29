//! Renders a level's scenery to a PNG (art review aid).
use silkie_scramble::gfx::scenery::{self, TILE};
use silkie_scramble::world::level;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let index: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let px: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(48.0);
    let out = args.get(3).cloned().unwrap_or_else(|| "level.png".into());
    let lvl = level::builtin(index).expect("level");
    let t0 = std::time::Instant::now();
    let s = scenery::build(&lvl);
    let built = t0.elapsed();
    let scale = px / TILE;
    let mut pm = tiny_skia::Pixmap::new((s.width * scale) as u32, (s.height * scale) as u32).unwrap();
    let t1 = std::time::Instant::now();
    s.ground.render(&mut pm, tiny_skia::Transform::from_scale(scale, scale));
    s.canopy.render(&mut pm, tiny_skia::Transform::from_scale(scale, scale));
    let rendered = t1.elapsed();
    pm.save_png(&out).unwrap();
    let rec = s.ground.records();
    let z = silkie_scramble::term::kitty_gfx::zlib(rec.as_bytes());
    let png = pm.encode_png().unwrap();
    println!("{}: {} ground + {} canopy elements, records {} bytes ({} zlib), raster {}x{} png {} bytes; build {:?} render {:?}",
        lvl.name, s.ground.len(), s.canopy.len(), rec.len(), z.len(), pm.width(), pm.height(), png.len(), built, rendered);
}
