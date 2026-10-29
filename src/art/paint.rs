//! A small painting toolkit for sprite art: fluffy outlines, soft
//! gradients, inked silhouettes and a transform stack for posing.
//!
//! Art is drawn in "art units" where a tile is 64 units, with the origin at
//! the creature's ground point (its anchor in the sprite cell).

use crate::gfx::vector::{Affine, Color, PathData};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tiny_skia::{FillRule, GradientStop, LineCap, LineJoin, Paint, Pixmap, RadialGradient, Shader, SpreadMode, Stroke, Transform};

pub struct Painter {
    pub pm: Pixmap,
    base: Transform,
    stack: Vec<Affine>,
}

impl Painter {
    /// A canvas of `w`x`h` pixels, `scale` pixels per art unit, with the
    /// art origin at pixel (`ox`, `oy`).
    pub fn new(w: u32, h: u32, scale: f32, ox: f32, oy: f32) -> Self {
        Painter {
            pm: Pixmap::new(w.max(1), h.max(1)).expect("non-zero canvas"),
            base: Transform::from_row(scale, 0.0, 0.0, scale, ox, oy),
            stack: Vec::new(),
        }
    }

    fn t(&self) -> Transform {
        let mut t = self.base;
        for a in &self.stack {
            t = t.pre_concat(a.skia());
        }
        t
    }

    /// Applies `a` to everything drawn until the matching `pop`.
    pub fn push(&mut self, a: Affine) {
        self.stack.push(a);
    }

    pub fn pop(&mut self) {
        self.stack.pop();
    }

    /// Draws into a fresh layer and composites it at `opacity`, so
    /// overlapping shapes inside it do not darken each other.
    pub fn layer(&mut self, opacity: f32, f: impl FnOnce(&mut Painter)) {
        let mut sub = Painter {
            pm: Pixmap::new(self.pm.width(), self.pm.height()).expect("same size as the canvas"),
            base: self.base,
            stack: self.stack.clone(),
        };
        f(&mut sub);
        let paint = tiny_skia::PixmapPaint {
            opacity,
            ..Default::default()
        };
        self.pm.draw_pixmap(0, 0, sub.pm.as_ref(), &paint, Transform::identity(), None);
    }

    pub fn fill(&mut self, d: &PathData, c: Color) {
        if let Some(p) = d.to_skia() {
            let paint = Paint {
                shader: Shader::SolidColor(c.skia()),
                anti_alias: true,
                ..Default::default()
            };
            let t = self.t();
            self.pm.fill_path(&p, &paint, FillRule::Winding, t, None);
        }
    }

    pub fn stroke(&mut self, d: &PathData, c: Color, w: f32) {
        if let Some(p) = d.to_skia() {
            let paint = Paint {
                shader: Shader::SolidColor(c.skia()),
                anti_alias: true,
                ..Default::default()
            };
            let s = Stroke {
                width: w,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Default::default()
            };
            let t = self.t();
            self.pm.stroke_path(&p, &paint, &s, t, None);
        }
    }

    /// Fills with a radial gradient centred at (cx, cy).
    pub fn radial(&mut self, d: &PathData, cx: f32, cy: f32, r: f32, stops: &[(f32, Color)]) {
        let Some(p) = d.to_skia() else { return };
        let Some(shader) = RadialGradient::new(
            (cx, cy).into(),
            0.0,
            (cx, cy).into(),
            r.max(0.01),
            stops.iter().map(|(o, c)| GradientStop::new(*o, c.skia())).collect(),
            SpreadMode::Pad,
            Transform::identity(),
        ) else {
            self.fill(d, stops.last().map_or(Color::rgb(0, 0, 0), |s| s.1));
            return;
        };
        let paint = Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        };
        let t = self.t();
        self.pm.fill_path(&p, &paint, FillRule::Winding, t, None);
    }

    /// Outline then fill: the inked look.
    pub fn inked(&mut self, d: &PathData, fill: Color, ink: Color, w: f32) {
        self.stroke(d, ink, w * 2.0);
        self.fill(d, fill);
    }

    /// Outline, then a radial gradient lit from `light` (centre x, centre
    /// y, radius).
    pub fn inked_radial(&mut self, d: &PathData, ink: Color, w: f32, light: (f32, f32, f32), stops: &[(f32, Color)]) {
        self.stroke(d, ink, w * 2.0);
        self.radial(d, light.0, light.1, light.2, stops);
    }
}

/// A cotton-wool shape: an elliptical core ringed with overlapping round
/// bumps. Inking it fills a grown copy in the ink colour first, so the
/// outline follows the whole silhouette and no inner seams show.
#[derive(Clone, Debug)]
pub struct Cloud {
    pub cx: f32,
    pub cy: f32,
    pub rx: f32,
    pub ry: f32,
    pub bumps: Vec<(f32, f32, f32)>,
}

impl Cloud {
    /// `n` bumps whose radius is `bump` times the mean radius.
    pub fn new(cx: f32, cy: f32, rx: f32, ry: f32, n: usize, bump: f32, seed: u64) -> Cloud {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let phase: f32 = rng.random_range(0.0..std::f32::consts::TAU);
        let mean = (rx + ry) / 2.0;
        let bumps = (0..n.max(3))
            .map(|i| {
                let a = phase + (i as f32 + rng.random_range(-0.2..0.2)) / n as f32 * std::f32::consts::TAU;
                let r = bump * mean * rng.random_range(0.85..1.15);
                let k = 1.0 - r / mean * 0.55;
                (cx + a.cos() * rx * k, cy + a.sin() * ry * k, r)
            })
            .collect();
        Cloud { cx, cy, rx, ry, bumps }
    }

    pub fn path(&self, grow: f32) -> PathData {
        let mut d = PathData::ellipse(self.cx, self.cy, self.rx * 0.88 + grow, self.ry * 0.88 + grow);
        for &(x, y, r) in &self.bumps {
            d.extend(&PathData::circle(x, y, r + grow));
        }
        d
    }
}

impl Painter {
    pub fn cloud(&mut self, c: &Cloud, ink: Color, w: f32, fill: Color) {
        self.fill(&c.path(w), ink);
        self.fill(&c.path(0.0), fill);
    }

    /// A cloud filled with a radial gradient lit from (lx, ly).
    pub fn cloud_lit(&mut self, c: &Cloud, ink: Color, w: f32, lx: f32, ly: f32, stops: &[(f32, Color)]) {
        self.fill(&c.path(w), ink);
        let r = (c.rx.max(c.ry)) * 1.35;
        self.radial(&c.path(0.0), lx, ly, r, stops);
    }
}

/// A fluffy outline: an ellipse whose edge is a ring of soft tufts. `amp`
/// is the tuft depth as a fraction of the radius; `seed` varies the tufts
/// so no two blobs are alike.
pub fn fluff(cx: f32, cy: f32, rx: f32, ry: f32, tufts: usize, amp: f32, seed: u64) -> PathData {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let n = tufts.max(3) * 2;
    let phase: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    let pts: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = phase + i as f32 / n as f32 * std::f32::consts::TAU;
            let wobble: f32 = rng.random_range(-0.25..0.25);
            let k = if i % 2 == 0 { 1.0 + amp * (1.0 + wobble) } else { 1.0 - amp * 0.35 };
            (cx + a.cos() * rx * k, cy + a.sin() * ry * k)
        })
        .collect();
    PathData::smooth_closed(&pts, 1.0)
}

/// Short curved strokes scattered over the lower part of an ellipse: the
/// hair-like feathers of a Silkie, lying downwards.
pub fn wisps(cx: f32, cy: f32, rx: f32, ry: f32, count: usize, len: f32, seed: u64) -> PathData {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut d = PathData::new();
    for _ in 0..count {
        let a: f32 = rng.random_range(0.15..std::f32::consts::PI - 0.15);
        let r: f32 = rng.random_range(0.45..0.92f32);
        let x = cx + a.cos() * rx * r;
        let y = cy + a.sin() * ry * r;
        // Feathers lie outwards and down, curling slightly.
        let dir = (a.cos() * 0.45, 0.9);
        let l = len * rng.random_range(0.6..1.1);
        let bend: f32 = rng.random_range(-0.6..0.6);
        d.move_to(x, y).quad_to(x + bend * l * 0.5, y + l * 0.5, x + dir.0 * l, y + dir.1 * l);
    }
    d
}

pub fn rot(deg: f32) -> Affine {
    Affine::rotate(deg.to_radians())
}

/// Rotation about a point.
pub fn rot_about(deg: f32, x: f32, y: f32) -> Affine {
    Affine::translate(-x, -y).then(rot(deg)).then(Affine::translate(x, y))
}

/// Scale about a point.
pub fn scale_about(sx: f32, sy: f32, x: f32, y: f32) -> Affine {
    Affine::translate(-x, -y).then(Affine::scale(sx, sy)).then(Affine::translate(x, y))
}

/// Mirror left-right about x = 0 (the anchor).
pub fn mirror() -> Affine {
    Affine::scale(-1.0, 1.0)
}

pub fn shadow(p: &mut Painter, rx: f32, ry: f32) {
    p.fill(&PathData::ellipse(0.0, 1.0, rx, ry), Color::rgba(40, 50, 20, 70));
}

/// A dark eye with a highlight; `open` from 0 (shut) to 1.
pub fn eye(p: &mut Painter, x: f32, y: f32, r: f32, open: f32, ink: Color) {
    if open < 0.2 {
        let mut lid = PathData::new();
        lid.move_to(x - r, y).quad_to(x, y + r * 0.8, x + r, y);
        p.stroke(&lid, ink, r * 0.55);
        return;
    }
    p.fill(&PathData::ellipse(x, y, r, r * open.max(0.3)), ink);
    p.fill(&PathData::circle(x + r * 0.35, y - r * 0.35 * open, r * 0.38), Color::rgb(255, 255, 255));
}

/// A happy closed eye, like a little arch.
pub fn happy_eye(p: &mut Painter, x: f32, y: f32, r: f32, ink: Color) {
    let mut a = PathData::new();
    a.move_to(x - r, y + r * 0.3).quad_to(x, y - r * 0.9, x + r, y + r * 0.3);
    p.stroke(&a, ink, r * 0.55);
}
