//! The predators: a lanky fox with sooty socks and a white-tipped brush,
//! a slender weasel that bounds along, and a hawk seen from below, with
//! the shadow it casts.

use super::SheetBuilder;
use super::paint::{self, Painter, mirror, rot_about};
use crate::gfx::sprites::SheetSpec;
use crate::gfx::vector::{Affine, Color, PathData};
use tiny_skia::Pixmap;

const FUR: Color = Color::hex(0xdf7430);
const FUR_LIGHT: Color = Color::hex(0xf39c55);
const FUR_DARK: Color = Color::hex(0xb3561f);
const CREAM: Color = Color::hex(0xfff3e2);
const SOCK: Color = Color::hex(0x3e2a22);
const FOX_INK: Color = Color::hex(0x4a2817);
const EYE: Color = Color::hex(0x1c1410);

fn leg(p: &mut Painter, hip: (f32, f32), len: f32, angle: f32, fur: Color, sock: Color, w: f32) {
    let a = angle.to_radians();
    let foot = (hip.0 + a.sin() * len, hip.1 + a.cos() * len);
    let knee = (hip.0 + a.sin() * len * 0.5, hip.1 + a.cos() * len * 0.5);
    let mut upper = PathData::new();
    upper.move_to(hip.0, hip.1).line_to(knee.0, knee.1);
    let mut lower = PathData::new();
    lower.move_to(knee.0, knee.1).line_to(foot.0, foot.1);
    p.stroke(&upper, FOX_INK, w + 2.4);
    p.stroke(&lower, FOX_INK, w + 2.4);
    p.stroke(&upper, fur, w);
    p.stroke(&lower, sock, w);
    p.fill(&PathData::ellipse(foot.0 + 1.5, foot.1 - 0.5, w * 0.75, w * 0.45), sock);
}

/// A bushy tail along a spine of points, with a white tip.
fn tail(p: &mut Painter, spine: &[(f32, f32)], widths: &[f32], ink: Color, fur: Color, tip: Color) {
    let n = spine.len();
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..n {
        let (x, y) = spine[i];
        let (px, py) = spine[i.saturating_sub(1)];
        let (nx, ny) = spine[(i + 1).min(n - 1)];
        let (dx, dy) = (nx - px, ny - py);
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ox, oy) = (-dy / l * widths[i], dx / l * widths[i]);
        left.push((x + ox, y + oy));
        right.push((x - ox, y - oy));
    }
    let end = spine[n - 1];
    let prev = spine[n - 2];
    let dir = ((end.0 - prev.0), (end.1 - prev.1));
    let dl = (dir.0 * dir.0 + dir.1 * dir.1).sqrt().max(1e-3);
    let tipp = (end.0 + dir.0 / dl * widths[n - 1] * 0.9, end.1 + dir.1 / dl * widths[n - 1] * 0.9);
    let mut pts = left.clone();
    pts.push(tipp);
    pts.extend(right.iter().rev());
    let shape = PathData::smooth_closed(&pts, 0.9);
    let tip_c = (end.0 + dir.0 / dl * 2.0, end.1 + dir.1 / dl * 2.0);
    let tip_shape = PathData::ellipse(tip_c.0, tip_c.1, widths[n - 1] * 0.95, widths[n - 1] * 0.95);
    // Ink both shapes first, then fill both, so the white tip joins the
    // tail without a seam.
    p.stroke(&shape, ink, 2.6);
    p.stroke(&tip_shape, ink, 2.6);
    p.fill(&shape, fur);
    // Fur strokes along the tail.
    let mut strands = PathData::new();
    for i in 1..n - 1 {
        let (x, y) = spine[i];
        strands.move_to(left[i].0 * 0.6 + x * 0.4, left[i].1 * 0.6 + y * 0.4).quad_to(x, y, spine[i + 1].0, spine[i + 1].1);
    }
    p.stroke(&strands, FUR_DARK.with_alpha(120), 1.2);
    p.fill(&tip_shape, tip);
}

#[derive(Clone, Copy)]
struct FoxPose {
    legs: [f32; 4], // far back, far front, near back, near front (degrees)
    bob: f32,
    stretch: f32,
    head_dip: f32,
    ears_back: f32,
    tail_lift: f32,
    eye_open: f32,
    alert: bool,
}

impl FoxPose {
    fn stand() -> Self {
        FoxPose {
            legs: [0.0; 4],
            bob: 0.0,
            stretch: 1.0,
            head_dip: 0.0,
            ears_back: 0.0,
            tail_lift: 0.0,
            eye_open: 0.7,
            alert: false,
        }
    }
}

fn fox_side(p: &mut Painter, f: &FoxPose) {
    paint::shadow(p, 34.0, 6.5);
    let b = f.bob;
    p.push(Affine::scale(f.stretch, 1.0));
    leg(p, (-17.0, -15.0 + b), 15.0, f.legs[0], FUR_DARK, SOCK.shade(-0.2), 5.5);
    leg(p, (16.0, -15.0 + b), 15.0, f.legs[1], FUR_DARK, SOCK.shade(-0.2), 5.5);
    // Tail.
    let lift = f.tail_lift;
    tail(
        p,
        &[
            (-22.0, -22.0 + b),
            (-34.0, -26.0 + b - lift * 4.0),
            (-45.0, -32.0 + b - lift * 9.0),
            (-53.0, -40.0 + b - lift * 14.0),
        ],
        &[4.5, 8.5, 9.0, 7.0],
        FOX_INK,
        FUR,
        CREAM,
    );
    // Body.
    let body = PathData::smooth_closed(
        &[
            (-27.0, -21.0 + b),
            (-15.0, -31.0 + b),
            (0.0, -33.0 + b),
            (14.0, -31.5 + b),
            (24.0, -25.0 + b),
            (19.0, -14.0 + b),
            (0.0, -12.0 + b),
            (-19.0, -13.0 + b),
        ],
        1.0,
    );
    p.stroke(&body, FOX_INK, 2.6);
    p.radial(&body, 2.0, -34.0 + b, 30.0, &[(0.0, FUR_LIGHT), (0.6, FUR), (1.0, FUR_DARK)]);
    p.fill(&PathData::ellipse(3.0, -15.5 + b, 15.0, 3.6), CREAM.with_alpha(230));
    leg(p, (-14.0, -15.0 + b), 15.0, f.legs[2], FUR, SOCK, 6.0);
    leg(p, (19.0, -15.0 + b), 15.0, f.legs[3], FUR, SOCK, 6.0);
    // Chest ruff.
    let ruff = PathData::smooth_closed(&[(17.0, -31.0 + b), (27.0, -28.0 + b), (27.0, -18.0 + b), (20.0, -15.0 + b), (15.0, -22.0 + b)], 1.0);
    p.stroke(&ruff, FOX_INK, 2.0);
    p.fill(&ruff, CREAM);
    p.pop();
    // Head.
    let hx = 23.0 * (f.stretch - 1.0);
    p.push(Affine::translate(hx, 0.0).then(rot_about(32.0 * f.head_dip, 24.0, -30.0 + b)));
    let earb = f.ears_back * 10.0;
    let far_ear = PathData::polygon(&[(24.0, -40.0 + b), (25.0 - earb, -55.0 + b + earb * 0.3), (32.0, -43.0 + b)]);
    p.stroke(&far_ear, FOX_INK, 2.4);
    p.fill(&far_ear, SOCK);
    let head = PathData::smooth_closed(
        &[
            (22.0, -38.0 + b),
            (29.0, -45.0 + b),
            (37.0, -43.0 + b),
            (44.0, -37.0 + b),
            (52.0, -32.5 + b),
            (46.0, -28.5 + b),
            (35.0, -26.5 + b),
            (25.0, -28.5 + b),
        ],
        1.0,
    );
    p.stroke(&head, FOX_INK, 2.6);
    p.radial(&head, 34.0, -42.0 + b, 18.0, &[(0.0, FUR_LIGHT), (1.0, FUR)]);
    let muzzle = PathData::smooth_closed(
        &[(33.0, -32.5 + b), (43.0, -33.5 + b), (51.0, -31.0 + b), (45.0, -28.5 + b), (35.0, -27.2 + b), (29.0, -29.5 + b)],
        1.0,
    );
    p.fill(&muzzle, CREAM);
    p.fill(&PathData::ellipse(51.0, -32.4 + b, 2.7, 2.1), EYE);
    let mut smile = PathData::new();
    smile.move_to(45.0, -29.0 + b).quad_to(42.0, -27.5 + b, 39.0, -28.5 + b);
    p.stroke(&smile, FOX_INK, 1.1);
    let ear = PathData::polygon(&[(29.0, -42.0 + b), (33.0 - earb, -57.0 + b + earb * 0.3), (39.0, -41.0 + b)]);
    p.stroke(&ear, FOX_INK, 2.4);
    p.fill(&ear, FUR);
    p.fill(&PathData::polygon(&[(31.0, -43.0 + b), (33.5 - earb, -53.0 + b + earb * 0.3), (36.5, -42.5 + b)]), CREAM.with_alpha(200));
    let tip = PathData::polygon(&[(31.5 - earb * 0.6, -51.0 + b), (33.0 - earb, -57.0 + b + earb * 0.3), (35.5 - earb * 0.6, -50.5 + b)]);
    p.fill(&tip, SOCK);
    // Eye: sly half-lidded, or round when alert.
    if f.alert {
        p.fill(&PathData::circle(37.0, -38.0 + b, 3.0), Color::rgb(255, 255, 255));
        p.fill(&PathData::circle(37.6, -38.0 + b, 1.9), EYE);
    } else {
        paint::eye(p, 37.0, -38.0 + b, 2.5, f.eye_open, EYE);
        let mut lid = PathData::new();
        lid.move_to(33.8, -39.6 + b).quad_to(37.0, -41.2 + b, 40.4, -39.0 + b);
        p.stroke(&lid, FOX_INK, 1.4);
    }
    p.pop();
}

fn fox_front(p: &mut Painter, step: f32, bob: f32, eyes_dizzy: bool, sit: bool) {
    paint::shadow(p, 20.0, 6.0);
    let b = bob;
    // Tail curled round to one side.
    tail(
        p,
        &[(8.0, -10.0 + b), (18.0, -12.0 + b), (26.0, -18.0 + b), (28.0, -27.0 + b)],
        &[4.0, 7.0, 7.5, 6.0],
        FOX_INK,
        FUR,
        CREAM,
    );
    let body = PathData::ellipse(0.0, -18.0 + b, 14.0, 12.5);
    p.stroke(&body, FOX_INK, 2.6);
    p.radial(&body, 0.0, -24.0 + b, 16.0, &[(0.0, FUR_LIGHT), (1.0, FUR)]);
    let chest = PathData::ellipse(0.0, -17.0 + b, 7.5, 9.0);
    p.fill(&chest, CREAM);
    if !sit {
        for (i, x) in [-6.0f32, 6.0].into_iter().enumerate() {
            let lift = if i == 0 { step.max(0.0) } else { (-step).max(0.0) } * 3.0;
            let mut l = PathData::new();
            l.move_to(x, -12.0 + b).line_to(x, -1.0 - lift);
            p.stroke(&l, FOX_INK, 8.2);
            p.stroke(&l, SOCK, 5.8);
        }
    } else {
        for x in [-6.0f32, 6.0] {
            p.fill(&PathData::ellipse(x, -2.0, 4.5, 3.0), SOCK);
        }
    }
    // Head.
    let ears = |p: &mut Painter, s: f32| {
        let e = PathData::polygon(&[(s * 13.5, -38.0 + b), (s * 16.0, -55.0 + b), (s * 4.0, -44.0 + b)]);
        p.stroke(&e, FOX_INK, 2.4);
        p.fill(&e, FUR);
        p.fill(&PathData::polygon(&[(s * 12.0, -40.0 + b), (s * 14.6, -51.0 + b), (s * 6.0, -43.5 + b)]), CREAM.with_alpha(200));
        p.fill(&PathData::polygon(&[(s * 13.6, -49.0 + b), (s * 16.0, -55.0 + b), (s * 11.4, -50.6 + b)]), SOCK);
    };
    ears(p, -1.0);
    ears(p, 1.0);
    let head = PathData::smooth_closed(
        &[
            (-16.0, -30.0 + b),
            (-12.0, -41.0 + b),
            (0.0, -46.0 + b),
            (12.0, -41.0 + b),
            (16.0, -30.0 + b),
            (8.0, -25.0 + b),
            (0.0, -21.5 + b),
            (-8.0, -25.0 + b),
        ],
        1.0,
    );
    p.stroke(&head, FOX_INK, 2.6);
    p.radial(&head, 0.0, -42.0 + b, 18.0, &[(0.0, FUR_LIGHT), (1.0, FUR)]);
    let cheeks = PathData::smooth_closed(
        &[(-15.0, -30.0 + b), (-6.0, -32.0 + b), (0.0, -29.0 + b), (6.0, -32.0 + b), (15.0, -30.0 + b), (6.0, -24.5 + b), (0.0, -22.0 + b), (-6.0, -24.5 + b)],
        1.0,
    );
    p.fill(&cheeks, CREAM);
    p.fill(&PathData::ellipse(0.0, -27.5 + b, 2.9, 2.2), EYE);
    for x in [-5.8f32, 5.8] {
        if eyes_dizzy {
            let mut d = PathData::new();
            d.move_to(x - 2.2, -37.5 + b).line_to(x + 2.2, -33.5 + b).move_to(x + 2.2, -37.5 + b).line_to(x - 2.2, -33.5 + b);
            p.stroke(&d, EYE, 1.3);
        } else {
            paint::eye(p, x, -35.5 + b, 2.5, 0.75, EYE);
            let mut lid = PathData::new();
            lid.move_to(x - 3.2, -37.3 + b).quad_to(x, -38.8 + b, x + 3.2, -37.3 + b);
            p.stroke(&lid, FOX_INK, 1.3);
        }
    }
}

fn fox_back(p: &mut Painter, step: f32, bob: f32) {
    paint::shadow(p, 20.0, 6.0);
    let b = bob;
    for (i, x) in [-6.0f32, 6.0].into_iter().enumerate() {
        let lift = if i == 0 { step.max(0.0) } else { (-step).max(0.0) } * 3.0;
        let mut l = PathData::new();
        l.move_to(x, -12.0 + b).line_to(x, -1.0 - lift);
        p.stroke(&l, FOX_INK, 8.2);
        p.stroke(&l, SOCK, 5.8);
    }
    let body = PathData::ellipse(0.0, -19.0 + b, 14.5, 13.0);
    p.stroke(&body, FOX_INK, 2.6);
    p.radial(&body, 0.0, -26.0 + b, 16.0, &[(0.0, FUR_LIGHT), (1.0, FUR)]);
    for s in [-1.0f32, 1.0] {
        let e = PathData::polygon(&[(s * 12.5, -38.0 + b), (s * 15.0, -55.0 + b), (s * 3.5, -44.0 + b)]);
        p.stroke(&e, FOX_INK, 2.4);
        p.fill(&e, SOCK.shade(0.15));
    }
    let head = PathData::ellipse(0.0, -36.0 + b, 13.5, 11.0);
    p.stroke(&head, FOX_INK, 2.6);
    p.radial(&head, 0.0, -42.0 + b, 14.0, &[(0.0, FUR_LIGHT), (1.0, FUR)]);
    // Tail swinging towards the viewer.
    let sway = step * 4.0;
    tail(
        p,
        &[(0.0, -14.0 + b), (sway * 0.5, -8.0 + b), (sway, -1.0), (sway * 1.2, 4.0)],
        &[4.0, 6.5, 7.0, 5.5],
        FOX_INK,
        FUR,
        CREAM,
    );
}

pub fn fox_sheet(tile_px: u32) -> (SheetSpec, Pixmap) {
    let mut sb = SheetBuilder::new("fox", (128.0, 96.0), (64.0, 80.0), tile_px);
    for (dir, left) in [("right", false), ("left", true)] {
        let with = |p: &mut Painter, f: &FoxPose| {
            if left {
                p.push(mirror());
            }
            fox_side(p, f);
            if left {
                p.pop();
            }
        };
        for k in 0..4 {
            let ph = k as f32 * std::f32::consts::FRAC_PI_2;
            let sw = 24.0 * ph.sin();
            let mut f = FoxPose::stand();
            f.legs = [sw, -sw, -sw, sw];
            f.bob = -1.5 * ph.cos().abs();
            f.tail_lift = 0.15 + 0.1 * ph.sin();
            sb.frame(&format!("walk_{dir}"), |p| with(p, &f));
        }
        for k in 0..4 {
            // A gallop: legs gathered, then flung out.
            let (fr, bk, bob, stretch) = [(35.0, -35.0, -3.0, 1.08), (10.0, 5.0, -1.0, 1.0), (-25.0, 30.0, 0.0, 0.94), (5.0, -10.0, -2.0, 1.02)][k];
            let mut f = FoxPose::stand();
            f.legs = [bk + 8.0, fr - 8.0, bk, fr];
            f.bob = bob;
            f.stretch = stretch;
            f.ears_back = 0.8;
            f.tail_lift = -0.6;
            f.eye_open = 1.0;
            sb.frame(&format!("run_{dir}"), |p| with(p, &f));
        }
        for k in 0..2 {
            let mut f = FoxPose::stand();
            f.head_dip = 0.8 + 0.2 * k as f32;
            f.tail_lift = 0.3;
            f.eye_open = 0.5;
            f.legs = [0.0, 5.0, 0.0, 8.0];
            sb.frame(&format!("sniff_{dir}"), |p| with(p, &f));
        }
        let mut f = FoxPose::stand();
        f.alert = true;
        f.tail_lift = 1.0;
        f.bob = -2.0;
        sb.frame(&format!("alert_{dir}"), |p| with(p, &f));
    }
    for k in 0..4 {
        let step = [0.0f32, 1.0, 0.0, -1.0][k];
        sb.frame("walk_down", |p| fox_front(p, step, -1.2 * step.abs(), false, false));
    }
    for k in 0..4 {
        let step = [0.0f32, 1.0, 0.0, -1.0][k];
        sb.frame("walk_up", |p| fox_back(p, step, -1.2 * step.abs()));
    }
    for k in 0..2 {
        sb.frame("idle_down", |p| fox_front(p, 0.0, -0.6 * k as f32, false, true));
    }
    for k in 0..2 {
        sb.frame("stunned", |p| {
            p.push(rot_about(if k == 0 { -6.0 } else { 6.0 }, 0.0, -20.0));
            fox_front(p, 0.0, 0.0, true, true);
            p.pop();
        });
    }
    sb.finish()
}

const WEASEL: Color = Color::hex(0x93603f);
const WEASEL_LIGHT: Color = Color::hex(0xb47c55);
const WEASEL_INK: Color = Color::hex(0x3d2618);

fn weasel_side(p: &mut Painter, arch: f32, legs: [f32; 2], scared: bool) {
    paint::shadow(p, 26.0, 4.5);
    let a = arch;
    let tailp = PathData::smooth_open(&[(-24.0, -11.0 + a * 0.3), (-36.0, -14.0), (-46.0, -19.0 - a * 0.4)], 1.0);
    p.stroke(&tailp, WEASEL_INK, 7.5);
    p.stroke(&tailp, WEASEL, 5.0);
    p.fill(&PathData::circle(-46.0, -19.0 - a * 0.4, 3.0), WEASEL_INK);
    for (x, ang) in [(-17.0f32, legs[0]), (12.0, legs[1])] {
        let r = ang.to_radians();
        let mut l = PathData::new();
        l.move_to(x, -9.0).line_to(x + r.sin() * 8.0, -9.0 + r.cos() * 8.0);
        p.stroke(&l, WEASEL_INK, 6.0);
        p.stroke(&l, WEASEL.shade(-0.2), 4.0);
    }
    let body = PathData::smooth_closed(
        &[
            (-28.0, -10.0),
            (-20.0, -17.0 - a * 0.5),
            (-2.0, -20.0 - a),
            (14.0, -18.0 - a * 0.4),
            (21.0, -13.0),
            (14.0, -7.0),
            (-2.0, -6.5 - a * 0.3),
            (-20.0, -6.0),
        ],
        1.0,
    );
    p.stroke(&body, WEASEL_INK, 2.4);
    p.radial(&body, 0.0, -20.0, 26.0, &[(0.0, WEASEL_LIGHT), (1.0, WEASEL)]);
    p.fill(&PathData::ellipse(0.0, -8.5 - a * 0.2, 16.0, 2.6), CREAM.with_alpha(220));
    let head = PathData::smooth_closed(&[(15.0, -20.0), (24.0, -23.5), (31.0, -19.5), (36.0, -15.0), (31.0, -11.5), (21.0, -11.0)], 1.0);
    p.stroke(&head, WEASEL_INK, 2.4);
    p.fill(&head, WEASEL_LIGHT);
    p.fill(&PathData::smooth_closed(&[(22.0, -15.0), (31.0, -14.5), (35.0, -14.5), (30.0, -11.8), (21.5, -11.5)], 1.0), CREAM);
    p.inked(&PathData::circle(20.0, -22.0, 3.2), WEASEL, WEASEL_INK, 1.0);
    p.fill(&PathData::circle(20.0, -22.0, 1.5), Color::hex(0xe5a5a0));
    p.fill(&PathData::circle(36.0, -15.2, 1.4), Color::hex(0x3a2222));
    if scared {
        p.fill(&PathData::circle(27.5, -18.0, 2.4), Color::rgb(255, 255, 255));
        p.fill(&PathData::circle(27.5, -18.0, 1.2), EYE);
    } else {
        paint::eye(p, 27.5, -18.0, 1.7, 1.0, EYE);
    }
}

fn weasel_front(p: &mut Painter, step: f32, up: bool) {
    paint::shadow(p, 12.0, 4.0);
    for (i, x) in [-5.0f32, 5.0].into_iter().enumerate() {
        let lift = if i == 0 { step.max(0.0) } else { (-step).max(0.0) } * 2.5;
        p.fill(&PathData::ellipse(x, -2.0 - lift, 3.0, 2.2), WEASEL_INK);
    }
    let body = PathData::ellipse(0.0, -11.0, 8.5, 9.5);
    p.stroke(&body, WEASEL_INK, 2.4);
    p.fill(&body, WEASEL);
    if up {
        let tl = PathData::smooth_open(&[(0.0, -6.0), (step * 3.0, 0.0), (step * 4.0, 5.0)], 1.0);
        p.stroke(&tl, WEASEL_INK, 7.0);
        p.stroke(&tl, WEASEL, 4.5);
        p.inked(&PathData::ellipse(0.0, -21.0, 7.5, 6.5), WEASEL, WEASEL_INK, 1.2);
        for s in [-1.0f32, 1.0] {
            p.inked(&PathData::circle(s * 6.0, -26.0, 3.0), WEASEL, WEASEL_INK, 1.0);
        }
    } else {
        p.fill(&PathData::ellipse(0.0, -9.0, 5.0, 6.5), CREAM);
        for s in [-1.0f32, 1.0] {
            p.inked(&PathData::circle(s * 6.0, -26.0, 3.0), WEASEL, WEASEL_INK, 1.0);
            p.fill(&PathData::circle(s * 6.0, -26.0, 1.4), Color::hex(0xe5a5a0));
        }
        let head = PathData::ellipse(0.0, -21.0, 7.5, 6.5);
        p.inked(&head, WEASEL_LIGHT, WEASEL_INK, 1.2);
        p.fill(&PathData::ellipse(0.0, -18.0, 4.5, 3.0), CREAM);
        for s in [-1.0f32, 1.0] {
            paint::eye(p, s * 3.0, -22.0, 1.5, 1.0, EYE);
        }
        p.fill(&PathData::circle(0.0, -18.5, 1.3), Color::hex(0x3a2222));
    }
}

pub fn weasel_sheet(tile_px: u32) -> (SheetSpec, Pixmap) {
    let mut sb = SheetBuilder::new("weasel", (104.0, 64.0), (52.0, 52.0), tile_px);
    for (dir, left) in [("right", false), ("left", true)] {
        for k in 0..4 {
            // Bounding: the back arches as the hind legs come forward.
            let (arch, l) = [(0.0, [20.0, -20.0]), (5.0, [-10.0, 10.0]), (2.0, [-25.0, 25.0]), (-1.0, [10.0, -5.0])][k];
            sb.frame(&format!("run_{dir}"), |p| {
                if left {
                    p.push(mirror());
                }
                weasel_side(p, arch, l, false);
                if left {
                    p.pop();
                }
            });
        }
        sb.frame(&format!("scared_{dir}"), |p| {
            if left {
                p.push(mirror());
            }
            weasel_side(p, 7.0, [-30.0, 30.0], true);
            if left {
                p.pop();
            }
        });
    }
    for k in 0..2 {
        let step = if k == 0 { 1.0 } else { -1.0 };
        sb.frame("run_down", |p| weasel_front(p, step, false));
    }
    for k in 0..2 {
        let step = if k == 0 { 1.0 } else { -1.0 };
        sb.frame("run_up", |p| weasel_front(p, step, true));
    }
    sb.finish()
}

const HAWK: Color = Color::hex(0x7a5232);
const HAWK_LIGHT: Color = Color::hex(0xc99a68);
const HAWK_PALE: Color = Color::hex(0xf0dcc0);
const HAWK_INK: Color = Color::hex(0x3a2412);

/// A hawk seen from below, wings at `span` (1 fully spread).
fn hawk_shape(span: f32, sweep: f32) -> (PathData, PathData, PathData) {
    let mut wings = PathData::new();
    for s in [-1.0f32, 1.0] {
        let x = |v: f32| s * v * span;
        let sw = sweep * 14.0;
        wings.extend(&PathData::smooth_closed(
            &[
                (x(6.0), -10.0),
                (x(30.0), -16.0 + sw * 0.3),
                (x(58.0), -18.0 + sw),
                (x(80.0), -12.0 + sw * 1.5),
                (x(84.0), -4.0 + sw * 1.6),
                (x(76.0), 2.0 + sw * 1.5),
                (x(66.0), 6.0 + sw * 1.4),
                (x(48.0), 12.0 + sw),
                (x(26.0), 14.0 + sw * 0.4),
                (x(8.0), 12.0),
            ],
            0.8,
        ));
    }
    let body = PathData::smooth_closed(&[(0.0, -34.0), (7.0, -24.0), (9.0, -4.0), (7.0, 20.0), (0.0, 26.0), (-7.0, 20.0), (-9.0, -4.0), (-7.0, -24.0)], 1.0);
    let tail = PathData::smooth_closed(&[(-7.0, 18.0), (7.0, 18.0), (16.0, 40.0), (0.0, 46.0), (-16.0, 40.0)], 0.9);
    (wings, body, tail)
}

fn hawk(p: &mut Painter, span: f32, sweep: f32) {
    let (wings, body, tail) = hawk_shape(span, sweep);
    for d in [&wings, &tail, &body] {
        p.stroke(d, HAWK_INK, 3.0);
    }
    p.fill(&tail, HAWK_LIGHT);
    p.fill(&wings, HAWK_LIGHT);
    // Barring on the flight feathers.
    let mut bars = PathData::new();
    for s in [-1.0f32, 1.0] {
        for k in 0..4 {
            let x = s * (34.0 + k as f32 * 11.0) * span;
            bars.move_to(x, -12.0 + sweep * 5.0 * k as f32 * 0.3).line_to(x - s * 3.0, 8.0 + sweep * 9.0);
        }
    }
    for k in 0..3 {
        let y = 26.0 + k as f32 * 6.0;
        bars.move_to(-9.0 - k as f32 * 2.0, y).line_to(9.0 + k as f32 * 2.0, y);
    }
    p.stroke(&bars, HAWK.with_alpha(170), 2.4);
    p.fill(&body, HAWK_PALE);
    let mut streaks = PathData::new();
    for k in 0..4 {
        let y = -14.0 + k as f32 * 8.0;
        streaks.move_to(-4.0, y).line_to(-3.0, y + 4.0).move_to(4.0, y).line_to(3.0, y + 4.0);
    }
    p.stroke(&streaks, HAWK, 1.8);
    p.fill(&PathData::ellipse(0.0, -29.0, 6.0, 5.5), HAWK);
    p.fill(&PathData::polygon(&[(-2.5, -34.0), (2.5, -34.0), (0.0, -39.0)]), Color::hex(0xe8c547));
}

fn hawk_shadow(p: &mut Painter, span: f32) {
    let (wings, body, tail) = hawk_shape(span, 0.0);
    // Painted in a layer and faded as a whole, so the overlap of wing and
    // body is no darker than the rest; the faint outer rings soften the
    // edge.
    p.layer(0.42, |p| {
        for (grow, alpha) in [(7.0f32, 60u8), (3.5, 140), (0.0, 255)] {
            let c = Color::rgba(20, 35, 12, alpha);
            for d in [&wings, &body, &tail] {
                p.stroke(d, c, grow * 2.0 + 0.1);
                p.fill(d, c);
            }
        }
    });
}

pub fn hawk_sheet(tile_px: u32) -> (SheetSpec, Pixmap) {
    let mut sb = SheetBuilder::new("hawk", (192.0, 144.0), (96.0, 72.0), tile_px);
    for span in [1.0f32, 0.88, 0.74, 0.88] {
        sb.frame("fly", |p| hawk(p, span, 0.0));
    }
    sb.frame("dive", |p| hawk(p, 0.55, 1.0));
    for span in [1.0f32, 0.82] {
        sb.frame("shadow", |p| hawk_shadow(p, span));
    }
    sb.finish()
}
