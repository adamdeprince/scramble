//! A retained vector scene in the shape of Silkie's vector image format
//! (`f=200`): groups, paths, symbols and uses, pen strokes and gradients,
//! keyed by persistent element ids.
//!
//! The same scene is either sent to Silkie as records (and then patched one
//! element at a time), or rasterized here with tiny-skia for terminals that
//! only take pixels. One description of the garden therefore draws both
//! renderers, and the two cannot drift apart.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use tiny_skia::{
    BlendMode, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, PathBuilder, Pixmap,
    PixmapPaint, RadialGradient, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

/// An sRGB colour with straight alpha and an optional HDR brightness, in
/// percent of SDR white (100 = normal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub bright: u16,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255, bright: 100 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a, bright: 100 }
    }

    pub const fn hex(v: u32) -> Self {
        Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Color { a, ..self }
    }

    pub fn with_bright(self, pct: u16) -> Self {
        Color { bright: pct, ..self }
    }

    /// Linear interpolation in sRGB, good enough for art.
    pub fn mix(self, o: Color, t: f32) -> Color {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Color {
            r: l(self.r, o.r),
            g: l(self.g, o.g),
            b: l(self.b, o.b),
            a: l(self.a, o.a),
            bright: self.bright,
        }
    }

    pub fn shade(self, t: f32) -> Color {
        if t >= 0.0 {
            self.mix(Color::rgba(255, 255, 255, self.a), t)
        } else {
            self.mix(Color::rgba(0, 0, 0, self.a), -t)
        }
    }

    pub fn to_record(self) -> String {
        let mut s = if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        };
        if self.bright > 100 {
            let _ = write!(s, "*{}", self.bright);
        }
        s
    }

    pub fn skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fill {
    None,
    Solid(Color),
    Gradient(u32),
}

impl Fill {
    fn to_record(&self) -> String {
        match self {
            Fill::None => "none".into(),
            Fill::Solid(c) => c.to_record(),
            Fill::Gradient(id) => format!("@{id}"),
        }
    }
}

impl From<Color> for Fill {
    fn from(c: Color) -> Self {
        Fill::Solid(c)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

/// Path geometry with builders for the shapes the art uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathData(pub Vec<Seg>);

/// Cubic Bezier approximation constant for quarter circles.
const KAPPA: f32 = 0.552_284_8;

impl PathData {
    pub fn new() -> Self {
        PathData(Vec::new())
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.0.push(Seg::M(x, y));
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.0.push(Seg::L(x, y));
        self
    }

    pub fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) -> &mut Self {
        self.0.push(Seg::Q(x1, y1, x, y));
        self
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> &mut Self {
        self.0.push(Seg::C(x1, y1, x2, y2, x, y));
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.0.push(Seg::Z);
        self
    }

    pub fn extend(&mut self, o: &PathData) -> &mut Self {
        self.0.extend_from_slice(&o.0);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> PathData {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        let mut p = PathData::new();
        p.move_to(cx + rx, cy)
            .cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry)
            .cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy)
            .cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry)
            .cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy)
            .close();
        p
    }

    pub fn circle(cx: f32, cy: f32, r: f32) -> PathData {
        Self::ellipse(cx, cy, r, r)
    }

    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> PathData {
        let mut p = PathData::new();
        p.move_to(x, y).line_to(x + w, y).line_to(x + w, y + h).line_to(x, y + h).close();
        p
    }

    pub fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> PathData {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let k = r * KAPPA;
        let mut p = PathData::new();
        p.move_to(x + r, y)
            .line_to(x + w - r, y)
            .cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r)
            .line_to(x + w, y + h - r)
            .cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h)
            .line_to(x + r, y + h)
            .cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r)
            .line_to(x, y + r)
            .cubic_to(x, y + r - k, x + r - k, y, x + r, y)
            .close();
        p
    }

    pub fn polygon(pts: &[(f32, f32)]) -> PathData {
        let mut p = PathData::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            if i == 0 {
                p.move_to(x, y);
            } else {
                p.line_to(x, y);
            }
        }
        if !pts.is_empty() {
            p.close();
        }
        p
    }

    pub fn polyline(pts: &[(f32, f32)]) -> PathData {
        let mut p = PathData::new();
        for (i, &(x, y)) in pts.iter().enumerate() {
            if i == 0 {
                p.move_to(x, y);
            } else {
                p.line_to(x, y);
            }
        }
        p
    }

    /// A closed Catmull-Rom spline through the points, as cubics: soft,
    /// hand-drawn looking outlines.
    pub fn smooth_closed(pts: &[(f32, f32)], tension: f32) -> PathData {
        let n = pts.len();
        let mut p = PathData::new();
        if n < 3 {
            return Self::polygon(pts);
        }
        p.move_to(pts[0].0, pts[0].1);
        for i in 0..n {
            let p0 = pts[(i + n - 1) % n];
            let p1 = pts[i];
            let p2 = pts[(i + 1) % n];
            let p3 = pts[(i + 2) % n];
            let t = tension / 6.0;
            let c1 = (p1.0 + (p2.0 - p0.0) * t, p1.1 + (p2.1 - p0.1) * t);
            let c2 = (p2.0 - (p3.0 - p1.0) * t, p2.1 - (p3.1 - p1.1) * t);
            p.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
        }
        p.close();
        p
    }

    /// An open Catmull-Rom spline through the points.
    pub fn smooth_open(pts: &[(f32, f32)], tension: f32) -> PathData {
        let n = pts.len();
        let mut p = PathData::new();
        if n < 3 {
            return Self::polyline(pts);
        }
        p.move_to(pts[0].0, pts[0].1);
        for i in 0..n - 1 {
            let p0 = pts[i.saturating_sub(1)];
            let p1 = pts[i];
            let p2 = pts[i + 1];
            let p3 = pts[(i + 2).min(n - 1)];
            let t = tension / 6.0;
            let c1 = (p1.0 + (p2.0 - p0.0) * t, p1.1 + (p2.1 - p0.1) * t);
            let c2 = (p2.0 - (p3.0 - p1.0) * t, p2.1 - (p3.1 - p1.1) * t);
            p.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
        }
        p
    }

    pub fn transformed(&self, t: Affine) -> PathData {
        let f = |x: f32, y: f32| t.apply(x, y);
        PathData(
            self.0
                .iter()
                .map(|s| match *s {
                    Seg::M(x, y) => {
                        let (x, y) = f(x, y);
                        Seg::M(x, y)
                    }
                    Seg::L(x, y) => {
                        let (x, y) = f(x, y);
                        Seg::L(x, y)
                    }
                    Seg::Q(a, b, x, y) => {
                        let (a, b) = f(a, b);
                        let (x, y) = f(x, y);
                        Seg::Q(a, b, x, y)
                    }
                    Seg::C(a, b, c, d, x, y) => {
                        let (a, b) = f(a, b);
                        let (c, d) = f(c, d);
                        let (x, y) = f(x, y);
                        Seg::C(a, b, c, d, x, y)
                    }
                    Seg::Z => Seg::Z,
                })
                .collect(),
        )
    }

    pub fn to_record(&self) -> String {
        let mut s = String::with_capacity(self.0.len() * 16);
        for seg in &self.0 {
            match *seg {
                Seg::M(x, y) => {
                    let _ = write!(s, "M{},{}", num1(x), num1(y));
                }
                Seg::L(x, y) => {
                    let _ = write!(s, "L{},{}", num1(x), num1(y));
                }
                Seg::Q(a, b, x, y) => {
                    let _ = write!(s, "Q{},{},{},{}", num1(a), num1(b), num1(x), num1(y));
                }
                Seg::C(a, b, c, d, x, y) => {
                    let _ = write!(s, "C{},{},{},{},{},{}", num1(a), num1(b), num1(c), num1(d), num1(x), num1(y));
                }
                Seg::Z => s.push('Z'),
            }
        }
        s
    }

    pub fn to_skia(&self) -> Option<tiny_skia::Path> {
        let mut pb = PathBuilder::new();
        for seg in &self.0 {
            match *seg {
                Seg::M(x, y) => pb.move_to(x, y),
                Seg::L(x, y) => pb.line_to(x, y),
                Seg::Q(a, b, x, y) => pb.quad_to(a, b, x, y),
                Seg::C(a, b, c, d, x, y) => pb.cubic_to(a, b, c, d, x, y),
                Seg::Z => pb.close(),
            }
        }
        pb.finish()
    }

    /// Axis-aligned bounds of the control points.
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut b: Option<(f32, f32, f32, f32)> = None;
        let mut add = |x: f32, y: f32| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x), e.max(y)),
            });
        };
        for seg in &self.0 {
            match *seg {
                Seg::M(x, y) | Seg::L(x, y) => add(x, y),
                Seg::Q(a, c, x, y) => {
                    add(a, c);
                    add(x, y)
                }
                Seg::C(a, c, d, e, x, y) => {
                    add(a, c);
                    add(d, e);
                    add(x, y)
                }
                Seg::Z => {}
            }
        }
        b
    }
}

/// Number formatting for records: at most two decimals, no trailing zeros.
pub fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.2}");
        s.trim_end_matches('0').to_string()
    }
}

/// Coordinates: one decimal is a hundredth of a pixel at any sensible
/// display size, and keeps scenes small.
pub fn num1(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r:.1}")
    }
}

/// An affine transform `a b c d e f`, mapping (x, y) to
/// (a x + c y + e, b x + d y + f): the SVG and PDF convention.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f32, y: f32) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    pub fn rotate(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }

    /// `self` applied after `o`.
    pub fn then(self, o: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        Affine([
            a2 * a + c2 * b,
            b2 * a + d2 * b,
            a2 * c + c2 * d,
            b2 * c + d2 * d,
            a2 * e + c2 * f + e2,
            b2 * e + d2 * f + f2,
        ])
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    pub fn skia(self) -> Transform {
        let [a, b, c, d, e, f] = self.0;
        Transform::from_row(a, b, c, d, e, f)
    }

    fn to_record(self) -> String {
        let [a, b, c, d, e, f] = self.0;
        format!("{},{},{},{},{},{}", num(a), num(b), num(c), num(d), num1(e), num1(f))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub paint: Fill,
    pub width: f32,
    pub cap: Cap,
    pub join: Join,
    pub dash: Vec<f32>,
}

impl StrokeStyle {
    pub fn new(c: Color, width: f32) -> Self {
        StrokeStyle {
            paint: Fill::Solid(c),
            width,
            cap: Cap::Round,
            join: Join::Round,
            dash: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Group {
        parent: u32,
        t: Option<Affine>,
        /// Percent.
        opacity: f32,
        visible: bool,
        multiply: bool,
    },
    Path {
        group: u32,
        d: PathData,
        fill: Fill,
        even_odd: bool,
        stroke: Option<StrokeStyle>,
        t: Option<Affine>,
    },
    Symbol {
        d: PathData,
    },
    Use {
        group: u32,
        symbol: u32,
        t: Option<Affine>,
        fill: Fill,
    },
    /// A pen stroke: a ribbon through points with a width at each.
    Ink {
        group: u32,
        pts: Vec<[f32; 3]>,
        fill: Fill,
    },
    Linear {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stops: Vec<(f32, Color)>,
    },
    Radial {
        cx: f32,
        cy: f32,
        r: f32,
        fx: f32,
        fy: f32,
        stops: Vec<(f32, Color)>,
    },
}

impl Element {
    pub fn fill_path(group: u32, d: PathData, fill: impl Into<Fill>) -> Self {
        Element::Path {
            group,
            d,
            fill: fill.into(),
            even_odd: false,
            stroke: None,
            t: None,
        }
    }

    pub fn stroke_path(group: u32, d: PathData, stroke: StrokeStyle) -> Self {
        Element::Path {
            group,
            d,
            fill: Fill::None,
            even_odd: false,
            stroke: Some(stroke),
            t: None,
        }
    }

    pub fn group(parent: u32) -> Self {
        Element::Group {
            parent,
            t: None,
            opacity: 100.0,
            visible: true,
            multiply: false,
        }
    }

    pub fn parent(&self) -> Option<u32> {
        match self {
            Element::Group { parent, .. } => Some(*parent),
            Element::Path { group, .. } | Element::Use { group, .. } | Element::Ink { group, .. } => Some(*group),
            _ => None,
        }
    }

    fn is_drawable(&self) -> bool {
        matches!(self, Element::Path { .. } | Element::Use { .. } | Element::Ink { .. } | Element::Group { .. })
    }

    /// The record line for this element with id `id`.
    pub fn record(&self, id: u32) -> String {
        let mut s = String::new();
        match self {
            Element::Group {
                parent,
                t,
                opacity,
                visible,
                multiply,
            } => {
                let _ = write!(s, "g id={id}");
                if id != 0 {
                    let _ = write!(s, " p={parent}");
                }
                if let Some(t) = t {
                    let _ = write!(s, " t={}", t.to_record());
                }
                if *opacity < 100.0 {
                    let _ = write!(s, " o={}", num(*opacity));
                }
                if *multiply {
                    s.push_str(" blend=multiply");
                }
                if !visible {
                    s.push_str(" v=0");
                }
            }
            Element::Path {
                group,
                d,
                fill,
                even_odd,
                stroke,
                t,
            } => {
                let _ = write!(s, "p id={id}");
                if *group != 0 {
                    let _ = write!(s, " g={group}");
                }
                let _ = write!(s, " d={}", d.to_record());
                match (fill, stroke) {
                    (Fill::None, Some(_)) => {}
                    (f, _) => {
                        let _ = write!(s, " fill={}", f.to_record());
                    }
                }
                if *even_odd {
                    s.push_str(" rule=eo");
                }
                if let Some(st) = stroke {
                    let _ = write!(s, " stroke={} w={}", st.paint.to_record(), num(st.width));
                    match st.cap {
                        Cap::Butt => {}
                        Cap::Round => s.push_str(" cap=round"),
                        Cap::Square => s.push_str(" cap=square"),
                    }
                    match st.join {
                        Join::Miter => {}
                        Join::Round => s.push_str(" join=round"),
                        Join::Bevel => s.push_str(" join=bevel"),
                    }
                    if !st.dash.is_empty() {
                        let d: Vec<String> = st.dash.iter().map(|v| num(*v)).collect();
                        let _ = write!(s, " dash={}", d.join(","));
                    }
                }
                if let Some(t) = t {
                    let _ = write!(s, " t={}", t.to_record());
                }
            }
            Element::Symbol { d } => {
                let _ = write!(s, "s id={id} d={}", d.to_record());
            }
            Element::Use { group, symbol, t, fill } => {
                let _ = write!(s, "u id={id}");
                if *group != 0 {
                    let _ = write!(s, " g={group}");
                }
                let _ = write!(s, " s={symbol}");
                if let Some(t) = t {
                    let _ = write!(s, " t={}", t.to_record());
                }
                let _ = write!(s, " fill={}", fill.to_record());
            }
            Element::Ink { group, pts, fill } => {
                let _ = write!(s, "k id={id}");
                if *group != 0 {
                    let _ = write!(s, " g={group}");
                }
                let _ = write!(s, " pts={}", pts_record(pts));
                let _ = write!(s, " fill={}", fill.to_record());
            }
            Element::Linear { x1, y1, x2, y2, stops } => {
                let _ = write!(
                    s,
                    "l id={id} x1={} y1={} x2={} y2={} stops={}",
                    num(*x1),
                    num(*y1),
                    num(*x2),
                    num(*y2),
                    stops_record(stops)
                );
            }
            Element::Radial { cx, cy, r, fx, fy, stops } => {
                let _ = write!(
                    s,
                    "r id={id} cx={} cy={} r={} fx={} fy={} stops={}",
                    num(*cx),
                    num(*cy),
                    num(*r),
                    num(*fx),
                    num(*fy),
                    stops_record(stops)
                );
            }
        }
        s
    }
}

pub fn pts_record(pts: &[[f32; 3]]) -> String {
    pts.iter()
        .map(|p| format!("{},{},{}", num(p[0]), num(p[1]), num(p[2])))
        .collect::<Vec<_>>()
        .join(",")
}

fn stops_record(stops: &[(f32, Color)]) -> String {
    stops
        .iter()
        .map(|(o, c)| format!("{},{}", num(*o), c.to_record()))
        .collect::<Vec<_>>()
        .join(",")
}

/// A change to a scene, which is also one line of a patch.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Set(u32, Element),
    Delete(u32),
    DeleteGroup(u32),
    AppendPts(u32, Vec<[f32; 3]>),
}

impl Edit {
    pub fn record(&self) -> String {
        match self {
            Edit::Set(id, e) => e.record(*id),
            Edit::Delete(id) => format!("x id={id}"),
            Edit::DeleteGroup(id) => format!("x g={id}"),
            Edit::AppendPts(id, pts) => format!("a id={id} pts={}", pts_record(pts)),
        }
    }

    /// Whether Silkie must redraw the whole image for this edit (see the
    /// vector specification's partial redraw rules).
    pub fn redraws_everything(&self, scene: &Scene) -> bool {
        match self {
            Edit::Set(_, Element::Path { .. } | Element::Use { .. } | Element::Ink { .. }) => false,
            Edit::Set(..) | Edit::DeleteGroup(_) => true,
            Edit::Delete(id) => !matches!(
                scene.get(*id),
                Some(Element::Path { .. } | Element::Use { .. } | Element::Ink { .. }) | None
            ),
            Edit::AppendPts(..) => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    elems: BTreeMap<u32, (u64, Element)>,
    seq: u64,
}

impl Scene {
    pub fn new(width: f32, height: f32) -> Self {
        Scene {
            width,
            height,
            elems: BTreeMap::new(),
            seq: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }

    pub fn get(&self, id: u32) -> Option<&Element> {
        self.elems.get(&id).map(|(_, e)| e)
    }

    /// Adds an element or replaces one, keeping its place in drawing order.
    pub fn set(&mut self, id: u32, e: Element) {
        match self.elems.get_mut(&id) {
            Some(slot) => slot.1 = e,
            None => {
                self.seq += 1;
                self.elems.insert(id, (self.seq, e));
            }
        }
    }

    pub fn delete(&mut self, id: u32) {
        self.elems.remove(&id);
    }

    /// Deletes a group and everything whose parent chain leads to it.
    pub fn delete_group(&mut self, gid: u32) {
        let mut doomed = vec![gid];
        loop {
            let more: Vec<u32> = self
                .elems
                .iter()
                .filter(|(id, (_, e))| !doomed.contains(id) && e.parent().is_some_and(|p| doomed.contains(&p)))
                .map(|(id, _)| *id)
                .collect();
            if more.is_empty() {
                break;
            }
            doomed.extend(more);
        }
        for id in doomed {
            self.elems.remove(&id);
        }
    }

    pub fn append_pts(&mut self, id: u32, more: &[[f32; 3]]) {
        if let Some((_, Element::Ink { pts, .. })) = self.elems.get_mut(&id) {
            pts.extend_from_slice(more);
        }
    }

    pub fn apply(&mut self, edit: &Edit) {
        match edit {
            Edit::Set(id, e) => self.set(*id, e.clone()),
            Edit::Delete(id) => self.delete(*id),
            Edit::DeleteGroup(id) => self.delete_group(*id),
            Edit::AppendPts(id, pts) => self.append_pts(*id, pts),
        }
    }

    /// Every element as records, in creation order (so groups precede
    /// their members), for a full transmission.
    pub fn records(&self) -> String {
        let mut v: Vec<(&u64, &u32, &Element)> = self.elems.iter().map(|(id, (s, e))| (s, id, e)).collect();
        v.sort_by_key(|(s, _, _)| **s);
        let mut out = String::new();
        for (_, id, e) in v {
            out.push_str(&e.record(*id));
            out.push('\n');
        }
        out
    }

    fn children(&self) -> BTreeMap<u32, Vec<(u64, u32)>> {
        let mut map: BTreeMap<u32, Vec<(u64, u32)>> = BTreeMap::new();
        for (id, (seq, e)) in &self.elems {
            if *id == 0 || !e.is_drawable() {
                continue;
            }
            if let Some(p) = e.parent() {
                map.entry(p).or_default().push((*seq, *id));
            }
        }
        for v in map.values_mut() {
            v.sort();
        }
        map
    }

    /// Rasterizes the scene into `pixmap`; `base` maps user units to pixels.
    pub fn render(&self, pixmap: &mut Pixmap, base: Transform) {
        let children = self.children();
        let root_t = match self.get(0) {
            Some(Element::Group { t: Some(t), .. }) => base.pre_concat(t.skia()),
            _ => base,
        };
        self.render_group(0, pixmap, root_t, &children, 0);
    }

    fn render_group(&self, gid: u32, target: &mut Pixmap, t: Transform, children: &BTreeMap<u32, Vec<(u64, u32)>>, depth: usize) {
        // A cycle in parent chains would recurse forever; the format rejects
        // them, so stop at an impossible depth instead.
        if depth > 64 {
            return;
        }
        let Some(kids) = children.get(&gid) else {
            return;
        };
        for &(_, id) in kids {
            let Some(e) = self.get(id) else { continue };
            match e {
                Element::Group {
                    t: gt,
                    opacity,
                    visible,
                    multiply,
                    ..
                } => {
                    if !visible {
                        continue;
                    }
                    let ct = match gt {
                        Some(g) => t.pre_concat(g.skia()),
                        None => t,
                    };
                    if *opacity >= 100.0 && !multiply {
                        self.render_group(id, target, ct, children, depth + 1);
                    } else if let Some(mut layer) = Pixmap::new(target.width(), target.height()) {
                        self.render_group(id, &mut layer, ct, children, depth + 1);
                        let paint = PixmapPaint {
                            opacity: (*opacity / 100.0).clamp(0.0, 1.0),
                            blend_mode: if *multiply { BlendMode::Multiply } else { BlendMode::SourceOver },
                            ..Default::default()
                        };
                        target.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
                    }
                }
                Element::Path {
                    d,
                    fill,
                    even_odd,
                    stroke,
                    t: pt,
                    ..
                } => {
                    let ct = match pt {
                        Some(p) => t.pre_concat(p.skia()),
                        None => t,
                    };
                    let Some(path) = d.to_skia() else { continue };
                    // Records always carry the fill key, so `fill=none` is
                    // explicit and nothing is filled.
                    if let Some(paint) = self.paint_for(fill) {
                        let rule = if *even_odd { FillRule::EvenOdd } else { FillRule::Winding };
                        target.fill_path(&path, &paint, rule, ct, None);
                    }
                    if let Some(st) = stroke {
                        if let Some(paint) = self.paint_for(&st.paint) {
                            let mut s = Stroke {
                                width: st.width,
                                line_cap: match st.cap {
                                    Cap::Butt => LineCap::Butt,
                                    Cap::Round => LineCap::Round,
                                    Cap::Square => LineCap::Square,
                                },
                                line_join: match st.join {
                                    Join::Miter => LineJoin::Miter,
                                    Join::Round => LineJoin::Round,
                                    Join::Bevel => LineJoin::Bevel,
                                },
                                ..Default::default()
                            };
                            if st.dash.len() >= 2 {
                                s.dash = StrokeDash::new(st.dash.clone(), 0.0);
                            }
                            target.stroke_path(&path, &paint, &s, ct, None);
                        }
                    }
                }
                Element::Use { symbol, t: ut, fill, .. } => {
                    let Some(Element::Symbol { d }) = self.get(*symbol) else { continue };
                    let ct = match ut {
                        Some(u) => t.pre_concat(u.skia()),
                        None => t,
                    };
                    if let (Some(path), Some(paint)) = (d.to_skia(), self.paint_for(fill)) {
                        target.fill_path(&path, &paint, FillRule::Winding, ct, None);
                    }
                }
                Element::Ink { pts, fill, .. } => {
                    if let Some(paint) = self.paint_for(fill) {
                        draw_ribbon(target, pts, &paint, t);
                    }
                }
                _ => {}
            }
        }
    }

    fn paint_for(&self, fill: &Fill) -> Option<Paint<'static>> {
        let shader = match fill {
            Fill::None => return None,
            Fill::Solid(c) => Shader::SolidColor(c.skia()),
            Fill::Gradient(id) => match self.get(*id)? {
                Element::Linear { x1, y1, x2, y2, stops } => LinearGradient::new(
                    (*x1, *y1).into(),
                    (*x2, *y2).into(),
                    stops.iter().map(|(o, c)| GradientStop::new(*o, c.skia())).collect(),
                    SpreadMode::Pad,
                    Transform::identity(),
                )?,
                Element::Radial { cx, cy, r, fx, fy, stops } => RadialGradient::new(
                    (*fx, *fy).into(),
                    0.0,
                    (*cx, *cy).into(),
                    *r,
                    stops.iter().map(|(o, c)| GradientStop::new(*o, c.skia())).collect(),
                    SpreadMode::Pad,
                    Transform::identity(),
                )?,
                _ => return None,
            },
        };
        Some(Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        })
    }
}

/// Draws a variable-width ribbon: quads between consecutive points and
/// discs at each point, which gives round joins and caps.
pub fn draw_ribbon(target: &mut Pixmap, pts: &[[f32; 3]], paint: &Paint, t: Transform) {
    let mut pb = PathBuilder::new();
    for (i, p) in pts.iter().enumerate() {
        let r = (p[2] / 2.0).max(0.01);
        if let Some(c) = tiny_skia::Rect::from_xywh(p[0] - r, p[1] - r, r * 2.0, r * 2.0) {
            pb.push_oval(c);
        }
        if i + 1 < pts.len() {
            let q = pts[i + 1];
            let dx = q[0] - p[0];
            let dy = q[1] - p[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1e-4 {
                let (nx, ny) = (-dy / len, dx / len);
                let (r0, r1) = (p[2] / 2.0, q[2] / 2.0);
                pb.move_to(p[0] + nx * r0, p[1] + ny * r0);
                pb.line_to(q[0] + nx * r1, q[1] + ny * r1);
                pb.line_to(q[0] - nx * r1, q[1] - ny * r1);
                pb.line_to(p[0] - nx * r0, p[1] - ny * r0);
                pb.close();
            }
        }
    }
    if let Some(path) = pb.finish() {
        target.fill_path(&path, paint, FillRule::Winding, t, None);
    }
}

/// A mask for clipping, used by the art painter.
pub fn mask_from(path: &PathData, w: u32, h: u32, t: Transform) -> Option<Mask> {
    let mut m = Mask::new(w, h)?;
    m.fill_path(&path.to_skia()?, FillRule::Winding, true, t);
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_match_the_specification_examples() {
        let mut s = Scene::new(200.0, 100.0);
        s.set(1, Element::fill_path(0, PathData::rect(0.0, 0.0, 200.0, 100.0), Color::hex(0xffffff)));
        s.set(2, Element::fill_path(0, PathData::rect(20.0, 20.0, 60.0, 60.0), Color::hex(0xff0000)));
        assert_eq!(
            s.records(),
            "p id=1 d=M0,0L200,0L200,100L0,100Z fill=#ffffff\np id=2 d=M20,20L80,20L80,80L20,80Z fill=#ff0000\n"
        );
        let mut g = Element::group(0);
        if let Element::Group { t, .. } = &mut g {
            *t = Some(Affine([2.0, 0.0, 0.0, 2.0, -100.0, -50.0]));
        }
        assert_eq!(g.record(0), "g id=0 t=2,0,0,2,-100,-50");
    }

    #[test]
    fn replace_keeps_drawing_order() {
        let mut s = Scene::new(10.0, 10.0);
        s.set(5, Element::fill_path(0, PathData::rect(0.0, 0.0, 1.0, 1.0), Color::hex(0x00ff00)));
        s.set(3, Element::fill_path(0, PathData::rect(0.0, 0.0, 1.0, 1.0), Color::hex(0x0000ff)));
        s.set(5, Element::fill_path(0, PathData::rect(0.0, 0.0, 2.0, 2.0), Color::hex(0xff0000)));
        let r = s.records();
        assert!(r.find("id=5").unwrap() < r.find("id=3").unwrap());
    }

    #[test]
    fn rasterizes_like_the_terminal_would() {
        let mut s = Scene::new(10.0, 10.0);
        s.set(1, Element::fill_path(0, PathData::rect(0.0, 0.0, 10.0, 10.0), Color::hex(0xffffff)));
        s.set(2, Element::fill_path(0, PathData::rect(0.0, 0.0, 5.0, 10.0), Color::hex(0xff0000)));
        let mut px = Pixmap::new(20, 20).unwrap();
        s.render(&mut px, Transform::from_scale(2.0, 2.0));
        let left = px.pixel(2, 10).unwrap();
        let right = px.pixel(17, 10).unwrap();
        assert_eq!((left.red(), left.green()), (255, 0));
        assert_eq!((right.red(), right.green()), (255, 255));
    }

    #[test]
    fn group_deletion_is_recursive() {
        let mut s = Scene::new(10.0, 10.0);
        s.set(10, Element::group(0));
        s.set(11, Element::group(10));
        s.set(12, Element::fill_path(11, PathData::rect(0.0, 0.0, 1.0, 1.0), Color::hex(0)));
        s.set(13, Element::fill_path(0, PathData::rect(0.0, 0.0, 1.0, 1.0), Color::hex(0)));
        s.delete_group(10);
        assert_eq!(s.len(), 1);
        assert!(s.get(13).is_some());
    }

    #[test]
    fn edits_classify_redraw_cost() {
        let s = Scene::new(1.0, 1.0);
        let p = Edit::Set(1, Element::fill_path(0, PathData::rect(0.0, 0.0, 1.0, 1.0), Color::hex(0)));
        assert!(!p.redraws_everything(&s));
        assert!(Edit::Set(2, Element::group(0)).redraws_everything(&s));
        assert!(!Edit::AppendPts(3, vec![[0.0, 0.0, 1.0]]).redraws_everything(&s));
        assert_eq!(Edit::AppendPts(3, vec![[1.5, 2.0, 0.25]]).record(), "a id=3 pts=1.5,2,0.25");
    }

    #[test]
    fn colors_carry_alpha_and_brightness() {
        assert_eq!(Color::rgba(255, 136, 0, 128).to_record(), "#ff880080");
        assert_eq!(Color::hex(0xff8800).with_bright(250).to_record(), "#ff8800*250");
    }
}
