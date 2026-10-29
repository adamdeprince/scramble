//! Where the yard sits in the terminal: a field of whole cells wide,
//! centred between a status line at the top and one at the bottom, scaled
//! so the whole yard fits.

use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub cols: u32,
    pub rows: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    /// Top-left cell of the field image.
    pub field_col: u32,
    pub field_row: u32,
    /// Field width in cells (the image is placed `c` cells wide).
    pub field_cols: u32,
    /// Cells the field image covers vertically (the last may be partial).
    pub field_rows: u32,
    pub field_w: u32,
    pub field_h: u32,
    pub tile_px: f32,
    pub grid_w: i32,
    pub grid_h: i32,
}

/// Rows kept for the status lines.
pub const HUD_ROWS: u32 = 2;

/// The smallest window (in cells) that shows the garden: the status lines
/// need about 20 columns, and the garden at least a few rows.
pub const MIN_COLS: u32 = 20;
pub const MIN_ROWS: u32 = HUD_ROWS + 6;

impl Layout {
    pub fn new(cols: u32, rows: u32, cell_w: u32, cell_h: u32, grid_w: i32, grid_h: i32) -> Option<Layout> {
        if cols < MIN_COLS || rows < MIN_ROWS || cell_w == 0 || cell_h == 0 || grid_w <= 0 || grid_h <= 0 {
            return None;
        }
        let avail_rows = rows - HUD_ROWS;
        let aspect = grid_h as f64 / grid_w as f64;
        let by_height = ((avail_rows * cell_h) as f64 / (aspect * cell_w as f64)).floor() as u32;
        let field_cols = cols.min(by_height).max(1);
        let field_w = field_cols * cell_w;
        let field_h = (field_w as f64 * aspect).round() as u32;
        let field_rows = field_h.div_ceil(cell_h);
        let field_col = (cols - field_cols) / 2;
        let field_row = 1 + avail_rows.saturating_sub(field_rows) / 2;
        Some(Layout {
            cols,
            rows,
            cell_w,
            cell_h,
            field_col,
            field_row,
            field_cols,
            field_rows,
            field_w,
            field_h,
            tile_px: field_w as f32 / grid_w as f32,
            grid_w,
            grid_h,
        })
    }

    pub fn origin_px(&self) -> (f32, f32) {
        ((self.field_col * self.cell_w) as f32, (self.field_row * self.cell_h) as f32)
    }

    /// Absolute pixel position (from the top left of the cell area) of a
    /// point in the yard.
    pub fn world_to_px(&self, p: Vec2) -> (f32, f32) {
        let (ox, oy) = self.origin_px();
        (ox + p.x * self.tile_px, oy + p.y * self.tile_px)
    }

    pub fn px_to_world(&self, x: f32, y: f32) -> Vec2 {
        let (ox, oy) = self.origin_px();
        Vec2::new((x - ox) / self.tile_px, (y - oy) / self.tile_px)
    }

    /// The cell and in-cell offset at which an image whose top-left corner
    /// is at absolute pixel (x, y) is placed. None when it would start above
    /// or left of the screen.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(u32, u32, u32, u32)> {
        let x = x.round();
        let y = y.round();
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (x, y) = (x as u32, y as u32);
        let col = x / self.cell_w;
        let row = y / self.cell_h;
        if col >= self.cols || row >= self.rows {
            return None;
        }
        Some((row, col, x % self.cell_w, y % self.cell_h))
    }

    pub fn contains_cell(&self, col: u32, row: u32) -> bool {
        col >= self.field_col && col < self.field_col + self.field_cols && row >= self.field_row && row < self.field_row + self.field_rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_fits_and_is_centred() {
        let l = Layout::new(200, 56, 18, 36, 32, 18).unwrap();
        assert!(l.field_cols <= 200);
        assert!(l.field_h <= (56 - HUD_ROWS) * 36);
        assert!((l.tile_px - l.field_w as f32 / 32.0).abs() < 1e-4);
        assert!(l.field_row >= 1);
        assert!(l.field_row + l.field_rows < 56);
        let (x, y) = l.world_to_px(Vec2::new(16.0, 9.0));
        let back = l.px_to_world(x, y);
        assert!((back - Vec2::new(16.0, 9.0)).length() < 1e-3);
    }

    #[test]
    fn narrow_windows_scale_by_width() {
        let l = Layout::new(60, 80, 10, 20, 32, 18).unwrap();
        assert_eq!(l.field_cols, 60);
        assert_eq!(l.field_w, 600);
    }

    #[test]
    fn tiny_windows_have_no_layout() {
        assert!(Layout::new(10, 5, 10, 20, 32, 18).is_none());
    }

    #[test]
    fn placement_offsets_stay_inside_a_cell() {
        let l = Layout::new(100, 40, 9, 18, 32, 18).unwrap();
        let (r, c, x, y) = l.cell_at(100.4, 37.0).unwrap();
        assert_eq!((r, c, x, y), (2, 11, 1, 1));
        assert!(l.cell_at(-1.0, 5.0).is_none());
    }
}
