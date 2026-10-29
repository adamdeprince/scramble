//! Sprite sheets: the manifest written by `artgen`, loading the PNGs, and
//! resampling them to the size a tile happens to be on screen.

use image::{ImageBuffer, Rgba, imageops};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SheetSpec {
    pub name: String,
    pub file: String,
    pub cell_w: u32,
    pub cell_h: u32,
    pub cols: u32,
    pub frames: u32,
    /// The ground point of the creature within a cell, in pixels.
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub anims: BTreeMap<String, Vec<u32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub tile_px: u32,
    pub sheets: Vec<SheetSpec>,
}

pub const MANIFEST: &str = include_str!("../../assets/sprites/manifest.json");

/// The PNG bytes of a sheet shipped with the game.
pub fn sheet_png(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "hen" => include_bytes!("../../assets/sprites/hen.png"),
        "chick" => include_bytes!("../../assets/sprites/chick.png"),
        "fox" => include_bytes!("../../assets/sprites/fox.png"),
        "weasel" => include_bytes!("../../assets/sprites/weasel.png"),
        "hawk" => include_bytes!("../../assets/sprites/hawk.png"),
        "items" => include_bytes!("../../assets/sprites/items.png"),
        "fx" => include_bytes!("../../assets/sprites/fx.png"),
        _ => return None,
    })
}

pub fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).expect("the shipped sprite manifest parses")
}

/// A sheet in memory, straight (not premultiplied) RGBA.
#[derive(Clone, Debug)]
pub struct Sheet {
    pub spec: SheetSpec,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Sheet {
    pub fn decode(spec: &SheetSpec, png: &[u8]) -> anyhow::Result<Sheet> {
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
        Ok(Sheet {
            spec: spec.clone(),
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw(),
        })
    }

    pub fn anim(&self, name: &str) -> Option<&[u32]> {
        self.spec.anims.get(name).map(|v| v.as_slice())
    }

    /// The source rectangle of a frame.
    pub fn frame_rect(&self, frame: u32) -> [u32; 4] {
        let cols = self.spec.cols.max(1);
        [
            (frame % cols) * self.spec.cell_w,
            (frame / cols) * self.spec.cell_h,
            self.spec.cell_w,
            self.spec.cell_h,
        ]
    }

    /// Resamples every cell by `factor`, keeping the cell grid exact.
    /// Filtering happens on premultiplied colour so transparent edges do not
    /// grow dark fringes.
    pub fn scaled(&self, factor: f32) -> Sheet {
        let cw = ((self.spec.cell_w as f32 * factor).round() as u32).max(1);
        let ch = ((self.spec.cell_h as f32 * factor).round() as u32).max(1);
        let cols = self.spec.cols.max(1);
        let rows = self.spec.frames.div_ceil(cols).max(1);
        let mut out = vec![0u8; (cols * cw * rows * ch * 4) as usize];
        let out_w = cols * cw;
        for f in 0..self.spec.frames {
            let [sx, sy, sw, sh] = self.frame_rect(f);
            let mut cell: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(sw, sh);
            for y in 0..sh {
                for x in 0..sw {
                    let i = (((sy + y) * self.width + sx + x) * 4) as usize;
                    let a = self.rgba[i + 3] as u32;
                    let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                    cell.put_pixel(x, y, Rgba([pm(self.rgba[i]), pm(self.rgba[i + 1]), pm(self.rgba[i + 2]), a as u8]));
                }
            }
            let filter = if factor < 1.0 { imageops::FilterType::CatmullRom } else { imageops::FilterType::Lanczos3 };
            let resized = imageops::resize(&cell, cw, ch, filter);
            let ox = (f % cols) * cw;
            let oy = (f / cols) * ch;
            for y in 0..ch {
                for x in 0..cw {
                    let p = resized.get_pixel(x, y).0;
                    let a = p[3] as u32;
                    let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
                    let i = (((oy + y) * out_w + ox + x) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&[un(p[0]), un(p[1]), un(p[2]), p[3]]);
                }
            }
        }
        let mut spec = self.spec.clone();
        spec.cell_w = cw;
        spec.cell_h = ch;
        spec.anchor_x *= cw as f32 / self.spec.cell_w as f32;
        spec.anchor_y *= ch as f32 / self.spec.cell_h as f32;
        Sheet {
            spec,
            width: out_w,
            height: rows * ch,
            rgba: out,
        }
    }
}
