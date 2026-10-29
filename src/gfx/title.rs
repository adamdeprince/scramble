//! Lettering with the Lilita One typeface (SIL Open Font License, see
//! `assets/fonts/OFL.txt`): the title logo and the glyphs of score pop-ups.

use super::sprites::{Sheet, SheetSpec};
use super::vector::{Affine, Color, PathData};
use std::collections::BTreeMap;
use tiny_skia::{FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Pixmap, Shader, SpreadMode, Stroke, Transform};

pub const FONT: &[u8] = include_bytes!("../../assets/fonts/LilitaOne-Regular.ttf");

struct Outline(PathData, f32, f32, f32);

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(self.2 + x * self.1, self.3 - y * self.1);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(self.2 + x * self.1, self.3 - y * self.1);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(self.2 + x1 * self.1, self.3 - y1 * self.1, self.2 + x * self.1, self.3 - y * self.1);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(
            self.2 + x1 * self.1,
            self.3 - y1 * self.1,
            self.2 + x2 * self.1,
            self.3 - y2 * self.1,
            self.2 + x * self.1,
            self.3 - y * self.1,
        );
    }
    fn close(&mut self) {
        self.0.close();
    }
}

/// Outlines of `text` at `size` units per em, baseline at y = 0, starting
/// at x = 0. Returns the outlines and the advance width.
pub fn text_path(text: &str, size: f32) -> (PathData, f32) {
    let Ok(face) = ttf_parser::Face::parse(FONT, 0) else {
        return (PathData::new(), 0.0);
    };
    let scale = size / face.units_per_em() as f32;
    let mut x = 0.0;
    let mut out = Outline(PathData::new(), scale, 0.0, 0.0);
    for c in text.chars() {
        let Some(g) = face.glyph_index(c) else {
            x += size * 0.3;
            continue;
        };
        out.2 = x;
        face.outline_glyph(g, &mut out);
        x += face.glyph_hor_advance(g).unwrap_or(0) as f32 * scale;
    }
    (out.0, x)
}

pub fn cap_height(size: f32) -> f32 {
    ttf_parser::Face::parse(FONT, 0)
        .ok()
        .and_then(|f| f.capital_height().map(|h| h as f32 / f.units_per_em() as f32 * size))
        .unwrap_or(size * 0.7)
}

fn paint(c: Color) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(c.skia()),
        anti_alias: true,
        ..Default::default()
    }
}

fn stroke(w: f32) -> Stroke {
    Stroke {
        width: w,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Default::default()
    }
}

/// Draws chunky cartoon lettering: a dark outline, a cream rim, and a warm
/// gradient face, with a soft drop shadow.
pub fn draw_lettering(pm: &mut Pixmap, d: &PathData, t: Transform, size: f32, top: Color, bottom: Color) {
    let Some(path) = d.to_skia() else { return };
    let o = size * 0.11;
    let shadow_t = t.pre_translate(o * 0.25, o * 0.45);
    pm.stroke_path(&path, &paint(Color::rgba(20, 40, 10, 110)), &stroke(o * 2.2), shadow_t, None);
    pm.fill_path(&path, &paint(Color::rgba(20, 40, 10, 110)), FillRule::Winding, shadow_t, None);
    pm.stroke_path(&path, &paint(Color::hex(0x4a2a1a)), &stroke(o * 2.0), t, None);
    let bounds = d.bounds().unwrap_or((0.0, -size, 1.0, 0.0));
    let grad = LinearGradient::new(
        (0.0, bounds.1).into(),
        (0.0, bounds.3).into(),
        vec![GradientStop::new(0.0, top.skia()), GradientStop::new(1.0, bottom.skia())],
        SpreadMode::Pad,
        Transform::identity(),
    );
    let fill = Paint {
        shader: grad.unwrap_or(Shader::SolidColor(top.skia())),
        anti_alias: true,
        ..Default::default()
    };
    pm.fill_path(&path, &fill, FillRule::Winding, t, None);
}

/// Logo scene size in user units.
pub const LOGO_W: f32 = 1000.0;
pub const LOGO_H: f32 = 240.0;

/// The game's logo as a vector scene: chunky lettering with an outline,
/// a drop shadow and gradient faces. Silkie draws it from these records at
/// whatever size it is shown; elsewhere it is rasterized from the same
/// scene.
pub fn logo_scene() -> super::vector::Scene {
    use super::vector::{Element, Fill, Scene, StrokeStyle};
    let size = 100.0;
    let (silkie, sw) = text_path("Silkie", size);
    let mut scramble = PathData::new();
    let mut x = 0.0;
    for (i, ch) in "Scramble".chars().enumerate() {
        let (g, a) = text_path(&ch.to_string(), size);
        let dy = if i % 2 == 0 { -size * 0.04 } else { size * 0.03 };
        let rot = if i % 2 == 0 { -0.04 } else { 0.035 };
        scramble.extend(&g.transformed(Affine::rotate(rot).then(Affine::translate(x, dy))));
        x += a;
    }
    let gap = size * 0.28;
    let total = sw + gap + x;
    let line_h = size * 1.05;
    let scale = ((LOGO_W * 0.96) / total).min(LOGO_H * 0.9 / line_h);
    let x0 = (LOGO_W - total * scale) / 2.0;
    let base = (LOGO_H + cap_height(size) * scale) / 2.0;
    let place = Affine::scale(scale, scale).then(Affine::translate(x0, base));
    let silkie = silkie.transformed(place);
    let scramble = scramble.transformed(Affine::translate(sw + gap, 0.0).then(place));
    let mut both = silkie.clone();
    both.extend(&scramble);
    let o = size * 0.11 * scale;
    let mut s = Scene::new(LOGO_W, LOGO_H);
    s.set(0, Element::group(0));
    let top = base - cap_height(size) * scale * 1.1;
    s.set(10, Element::Linear { x1: 0.0, y1: top, x2: 0.0, y2: base, stops: vec![(0.0, Color::hex(0xffffff)), (1.0, Color::hex(0xd9cfe8))] });
    s.set(11, Element::Linear { x1: 0.0, y1: top, x2: 0.0, y2: base, stops: vec![(0.0, Color::hex(0xffd75e)), (1.0, Color::hex(0xf28a2e))] });
    let shadow = Color::rgba(20, 40, 10, 110);
    let mut sh = StrokeStyle::new(shadow, o * 2.2);
    sh.cap = super::vector::Cap::Round;
    s.set(
        20,
        Element::Path { group: 0, d: both.transformed(Affine::translate(o * 0.25, o * 0.45)), fill: Fill::Solid(shadow), even_odd: false, stroke: Some(sh), t: None },
    );
    s.set(21, Element::stroke_path(0, both.clone(), StrokeStyle::new(Color::hex(0x4a2a1a), o * 2.0)));
    s.set(22, Element::fill_path(0, silkie, Fill::Gradient(10)));
    s.set(23, Element::fill_path(0, scramble, Fill::Gradient(11)));
    s
}

/// The game's logo rasterized into `w` by `h` pixels.
pub fn logo(w: u32, h: u32) -> Pixmap {
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("logo size");
    let sx = w as f32 / LOGO_W;
    let sy = h as f32 / LOGO_H;
    let s = sx.min(sy);
    let t = Transform::from_row(s, 0.0, 0.0, s, (w as f32 - LOGO_W * s) / 2.0, (h as f32 - LOGO_H * s) / 2.0);
    logo_scene().render(&mut pm, t);
    pm
}

/// Glyphs for score pop-ups, as a sprite sheet at master resolution.
pub fn glyph_sheet(tile_px: u32) -> Sheet {
    let chars = "0123456789+x!";
    let size = tile_px as f32 * 0.55;
    let cell_w = (size * 0.75).ceil() as u32 + 8;
    let cell_h = (size * 1.05).ceil() as u32 + 8;
    let mut pm = Pixmap::new(cell_w * chars.len() as u32, cell_h).expect("glyph sheet");
    let mut anims = BTreeMap::new();
    for (i, c) in chars.chars().enumerate() {
        let (d, adv) = text_path(&c.to_string(), size);
        let x = i as f32 * cell_w as f32 + (cell_w as f32 - adv) / 2.0;
        let y = (cell_h as f32 + cap_height(size)) / 2.0;
        draw_lettering(&mut pm, &d, Transform::from_translate(x, y), size, Color::hex(0xfff6b0), Color::hex(0xffb13b));
        anims.insert(c.to_string(), vec![i as u32]);
    }
    let rgba = pm.take_demultiplied();
    Sheet {
        spec: SheetSpec {
            name: "glyphs".into(),
            file: String::new(),
            cell_w,
            cell_h,
            cols: chars.len() as u32,
            frames: chars.len() as u32,
            anchor_x: cell_w as f32 / 2.0,
            anchor_y: cell_h as f32 / 2.0,
            anims,
        },
        width: cell_w * chars.len() as u32,
        height: cell_h,
        rgba,
    }
}

/// Advance of a glyph in the pop-up sheet, as a fraction of its cell.
pub fn glyph_advance(c: char) -> f32 {
    match c {
        '1' => 0.55,
        '!' => 0.45,
        _ => 0.72,
    }
}
