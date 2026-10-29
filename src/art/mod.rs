//! Original sprite art, painted procedurally.
//!
//! Every creature and object is drawn here from shapes and gradients with a
//! deterministic seed. `cargo run --bin artgen` paints the sheets at the
//! master resolution and writes them to `assets/sprites/` as PNG files with
//! a JSON manifest; the game loads those bitmaps like any other art, so the
//! sheets could equally be replaced by hand-painted ones.

pub mod chick;
pub mod critters;
pub mod fx;
pub mod hen;
pub mod paint;

use crate::gfx::sprites::SheetSpec;
use paint::Painter;
use std::collections::BTreeMap;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// Art units per tile.
pub const UNITS: f32 = 64.0;
/// Pixels per tile at which the master sheets are painted.
pub const MASTER_TILE_PX: u32 = 96;

pub struct SheetBuilder {
    name: String,
    cell_w: u32,
    cell_h: u32,
    anchor: (f32, f32),
    scale: f32,
    frames: Vec<Pixmap>,
    anims: BTreeMap<String, Vec<u32>>,
}

impl SheetBuilder {
    /// A sheet whose cells are `cell` art units with the ground point at
    /// `anchor` (art units from the cell's top left).
    pub fn new(name: &str, cell: (f32, f32), anchor: (f32, f32), tile_px: u32) -> Self {
        let scale = tile_px as f32 / UNITS;
        SheetBuilder {
            name: name.to_string(),
            cell_w: (cell.0 * scale).round() as u32,
            cell_h: (cell.1 * scale).round() as u32,
            anchor: (anchor.0 * scale, anchor.1 * scale),
            scale,
            frames: Vec::new(),
            anims: BTreeMap::new(),
        }
    }

    /// Paints one frame and appends it to animation `anim`.
    pub fn frame(&mut self, anim: &str, draw: impl FnOnce(&mut Painter)) -> u32 {
        let mut p = Painter::new(self.cell_w, self.cell_h, self.scale, self.anchor.0, self.anchor.1);
        draw(&mut p);
        let index = self.frames.len() as u32;
        self.frames.push(p.pm);
        self.anims.entry(anim.to_string()).or_default().push(index);
        index
    }

    /// Reuses an existing frame in another animation.
    pub fn alias(&mut self, anim: &str, frame: u32) {
        self.anims.entry(anim.to_string()).or_default().push(frame);
    }

    pub fn finish(self) -> (SheetSpec, Pixmap) {
        let n = self.frames.len().max(1) as u32;
        let cols = n.min(16);
        let rows = n.div_ceil(cols);
        let mut sheet = Pixmap::new(cols * self.cell_w, rows * self.cell_h).expect("sheet size");
        for (i, f) in self.frames.iter().enumerate() {
            let i = i as u32;
            let x = (i % cols) * self.cell_w;
            let y = (i / cols) * self.cell_h;
            sheet.draw_pixmap(x as i32, y as i32, f.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        (
            SheetSpec {
                name: self.name.clone(),
                file: format!("{}.png", self.name),
                cell_w: self.cell_w,
                cell_h: self.cell_h,
                cols,
                frames: n,
                anchor_x: self.anchor.0,
                anchor_y: self.anchor.1,
                anims: self.anims,
            },
            sheet,
        )
    }
}

/// Paints every sheet at `tile_px` pixels per tile.
pub fn all_sheets(tile_px: u32) -> Vec<(SheetSpec, Pixmap)> {
    vec![
        hen::sheet(tile_px),
        chick::sheet(tile_px),
        critters::fox_sheet(tile_px),
        critters::weasel_sheet(tile_px),
        critters::hawk_sheet(tile_px),
        fx::items_sheet(tile_px),
        fx::fx_sheet(tile_px),
    ]
}
