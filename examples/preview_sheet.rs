//! Paints one sprite sheet over a garden-coloured background (art review aid).
use silkie_scramble::art;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args.get(1).cloned().unwrap_or_else(|| "hen".into());
    let px: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(96);
    let out = args.get(3).cloned().unwrap_or_else(|| "sheet.png".into());
    let first: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let count: u32 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(u32::MAX);
    let (spec, pm) = art::all_sheets(px).into_iter().find(|(s, _)| s.name == name).expect("sheet");
    let count = count.min(spec.frames.saturating_sub(first));
    let per_row = count.clamp(1, 8);
    let rows = count.div_ceil(per_row).max(1);
    let mut bg = tiny_skia::Pixmap::new(per_row * spec.cell_w, rows * spec.cell_h).unwrap();
    bg.fill(tiny_skia::Color::from_rgba8(0x8c, 0xc1, 0x69, 255));
    for i in 0..count {
        let f = first + i;
        let (sx, sy) = ((f % spec.cols) * spec.cell_w, (f / spec.cols) * spec.cell_h);
        let cell = pm.clone_rect(tiny_skia::IntRect::from_xywh(sx as i32, sy as i32, spec.cell_w, spec.cell_h).unwrap()).unwrap();
        bg.draw_pixmap(((i % per_row) * spec.cell_w) as i32, ((i / per_row) * spec.cell_h) as i32, cell.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    }
    bg.save_png(&out).unwrap();
    println!("{} {} frames {}x{} cells, sheet {}x{}; anims: {:?}", spec.name, spec.frames, spec.cell_w, spec.cell_h, pm.width(), pm.height(), spec.anims.keys().collect::<Vec<_>>());
}
