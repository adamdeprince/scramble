//! Paints every sprite sheet at the master resolution and writes them to
//! `assets/sprites/` with `manifest.json`. The art is deterministic: running
//! this again produces the same files.

use silkie_scramble::art::{self, MASTER_TILE_PX};
use silkie_scramble::gfx::sprites::Manifest;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/sprites"));
    std::fs::create_dir_all(&dir)?;
    let mut sheets = Vec::new();
    for (spec, pixmap) in art::all_sheets(MASTER_TILE_PX) {
        let path = dir.join(&spec.file);
        // tiny-skia pixmaps are premultiplied; encode_png writes straight
        // alpha as PNG requires.
        let png = pixmap.encode_png()?;
        std::fs::write(&path, &png)?;
        println!(
            "{:<8} {:>3} frames, {:>4}x{:<4} cells, {:>5}x{:<5} {:>8} bytes",
            spec.name,
            spec.frames,
            spec.cell_w,
            spec.cell_h,
            pixmap.width(),
            pixmap.height(),
            png.len()
        );
        sheets.push(spec);
    }
    let manifest = Manifest {
        tile_px: MASTER_TILE_PX,
        sheets,
    };
    std::fs::write(dir.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    Ok(())
}
