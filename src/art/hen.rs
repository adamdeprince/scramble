//! The Silkie hen: a near-spherical pom-pom of hair-like white feathers,
//! a powder-puff crest that tumbles over her eyes, a dark mulberry face,
//! turquoise earlobes, a slate-blue beak, and feathered feet that all but
//! vanish beneath her.

use super::SheetBuilder;
use super::paint::{self, Cloud, Painter, mirror, rot_about, scale_about, wisps};
use crate::gfx::sprites::SheetSpec;
use crate::gfx::vector::{Affine, Color, PathData};
use tiny_skia::Pixmap;

pub const FEATHER: Color = Color::hex(0xfffdf7);
pub const FEATHER_SHADE: Color = Color::hex(0xebe4d6);
pub const FEATHER_DEEP: Color = Color::hex(0xd6cdbb);
pub const WISP: Color = Color::hex(0xd2c8b6);
pub const INK: Color = Color::hex(0x6f6359);
const SKIN: Color = Color::hex(0x4a3858);
const BEAK: Color = Color::hex(0x8191a8);
const COMB: Color = Color::hex(0x7b2d5d);
const EARLOBE: Color = Color::hex(0x4cc6ca);
const EYE: Color = Color::hex(0x1d1520);

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Side,
    Front,
    Back,
    Turn,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Eyes {
    Open,
    Shut,
    Happy,
    Wide,
    Dizzy,
}

#[derive(Clone, Copy)]
pub struct Pose {
    pub view: View,
    pub bob: f32,
    pub squash: f32,
    pub step: f32,
    pub head_dip: f32,
    pub puff: f32,
    pub wings: f32,
    pub jump: f32,
    pub eyes: Eyes,
}

impl Pose {
    pub fn new(view: View) -> Self {
        Pose {
            view,
            bob: 0.0,
            squash: 1.0,
            step: 0.0,
            head_dip: 0.0,
            puff: 0.0,
            wings: 0.0,
            jump: 0.0,
            eyes: Eyes::Open,
        }
    }
}

const LIT: [(f32, Color); 3] = [(0.0, FEATHER), (0.5, FEATHER), (1.0, FEATHER_SHADE)];

fn feet(p: &mut Painter, xs: [f32; 2], step: f32) {
    for (i, &x) in xs.iter().enumerate() {
        let lift = if i == 0 { step.max(0.0) } else { (-step).max(0.0) } * 3.5;
        let c = Cloud::new(x, -4.5 - lift, 8.0, 5.5, 5, 0.42, 31 + i as u64);
        p.cloud(&c, INK, 1.2, FEATHER_SHADE);
    }
}

fn draw_eyes(p: &mut Painter, pts: &[(f32, f32)], eyes: Eyes) {
    for &(x, y) in pts {
        match eyes {
            Eyes::Open => paint::eye(p, x, y, 2.6, 1.0, EYE),
            Eyes::Shut => paint::eye(p, x, y, 2.6, 0.0, Color::hex(0xb59ac4)),
            Eyes::Happy => paint::happy_eye(p, x, y, 2.6, Color::hex(0xd8c3e6)),
            Eyes::Wide => {
                p.fill(&PathData::circle(x, y, 3.6), Color::rgb(255, 255, 255));
                p.fill(&PathData::circle(x, y, 1.7), EYE);
            }
            Eyes::Dizzy => {
                let mut d = PathData::new();
                d.move_to(x - 2.2, y - 2.2).line_to(x + 2.2, y + 2.2).move_to(x + 2.2, y - 2.2).line_to(x - 2.2, y + 2.2);
                p.stroke(&d, Color::hex(0xd8c3e6), 1.3);
            }
        }
    }
}

fn texture(p: &mut Painter, cx: f32, cy: f32, rx: f32, ry: f32, n: usize, seed: u64) {
    p.stroke(&wisps(cx, cy, rx, ry, n, 5.0, seed), WISP.with_alpha(190), 1.0);
}

/// Facing right; mirror for left.
fn side(p: &mut Painter, s: &Pose) {
    let b = s.bob;
    let pf = 1.0 + s.puff * 0.14;
    feet(p, [-8.0 - 4.0 * s.step, 8.0 + 4.0 * s.step], s.step);
    p.push(scale_about(pf / s.squash.sqrt(), pf * s.squash, 0.0, -2.0));
    let tail = Cloud::new(-25.0, -38.0 + b, 12.0, 11.0, 6, 0.45, 3);
    p.cloud_lit(&tail, INK, 1.4, -22.0, -46.0 + b, &LIT);
    let body = Cloud::new(0.0, -27.0 + b, 30.0, 23.5, 13, 0.3 + s.puff * 0.08, 1);
    p.cloud_lit(&body, INK, 1.4, 9.0, -42.0 + b, &LIT);
    texture(p, 0.0, -24.0 + b, 25.0, 17.0, 16, 5);
    p.push(rot_about(-45.0 * s.wings, 8.0, -30.0 + b));
    let wing = PathData::smooth_closed(
        &[(9.0, -33.0 + b), (-4.0, -35.0 + b), (-17.0, -29.0 + b), (-21.0, -22.0 + b), (-8.0, -19.0 + b), (5.0, -23.0 + b)],
        1.0,
    );
    p.fill(&wing, FEATHER_SHADE.with_alpha(170));
    p.stroke(&wing, INK.with_alpha(70), 1.0);
    let mut quills = PathData::new();
    for k in 0..3 {
        let y = -29.0 + b + k as f32 * 3.5;
        quills.move_to(2.0 - k as f32 * 2.0, y).quad_to(-8.0, y + 1.5, -15.0 + k as f32 * 2.0, y + 4.0);
    }
    p.stroke(&quills, WISP, 1.0);
    p.pop();
    // Head, dipping forward to peck.
    p.push(rot_about(55.0 * s.head_dip, 12.0, -36.0 + b).then(Affine::translate(4.0 * s.head_dip, 6.0 * s.head_dip)));
    p.fill(&PathData::ellipse(28.0, -45.0 + b, 8.6, 7.8), SKIN);
    let mut beak = PathData::new();
    beak.move_to(33.5, -46.5 + b)
        .quad_to(38.5, -46.0 + b, 41.0, -43.2 + b)
        .quad_to(38.0, -40.5 + b, 33.5, -41.0 + b)
        .close();
    p.inked(&beak, BEAK, BEAK.shade(-0.45), 0.7);
    p.fill(&PathData::circle(23.5, -41.0 + b, 2.4), EARLOBE);
    let muff = Cloud::new(25.5, -36.0 + b, 8.0, 5.5, 5, 0.42, 9);
    p.cloud(&muff, INK, 1.1, FEATHER);
    draw_eyes(p, &[(29.5, -46.2 + b)], s.eyes);
    p.fill(&PathData::ellipse(31.0, -53.0 + b, 5.0, 3.0), COMB);
    let crest = Cloud::new(15.5, -61.5 + b, 16.5, 12.5, 10, 0.36 + s.puff * 0.06, 11);
    p.cloud_lit(&crest, INK, 1.4, 19.0, -69.0 + b, &LIT);
    texture(p, 15.0, -61.0 + b, 12.0, 7.0, 8, 12);
    p.pop();
    p.pop();
}

fn front(p: &mut Painter, s: &Pose, face_shift: f32) {
    let b = s.bob;
    let pf = 1.0 + s.puff * 0.14;
    feet(p, [-11.0, 11.0], s.step);
    p.push(scale_about(pf / s.squash.sqrt(), pf * s.squash, 0.0, -2.0));
    // Wings at the sides, raised when celebrating.
    for (side, x) in [(-1.0f32, -24.0f32), (1.0, 24.0)] {
        p.push(rot_about(side * 55.0 * s.wings, x * 0.55, -30.0 + b));
        let w = Cloud::new(x, -28.0 + b, 9.0, 13.0, 5, 0.42, 20 + (side > 0.0) as u64);
        p.cloud(&w, INK, 1.2, FEATHER.mix(FEATHER_SHADE, 0.4));
        p.pop();
    }
    let body = Cloud::new(0.0, -27.0 + b, 30.5, 23.5, 14, 0.3 + s.puff * 0.08, 13);
    p.cloud_lit(&body, INK, 1.4, 5.0, -40.0 + b, &LIT);
    texture(p, 0.0, -22.0 + b, 25.0, 16.0, 18, 15);
    let fx = face_shift;
    p.fill(&PathData::ellipse(fx, -43.0 + b, 11.0, 8.4), SKIN);
    for x in [-10.6f32, 10.6] {
        if (fx > 0.0 && x < 0.0) || (fx < 0.0 && x > 0.0) {
            continue;
        }
        p.fill(&PathData::circle(x + fx, -40.0 + b, 2.3), EARLOBE);
    }
    let eyes: Vec<(f32, f32)> = if fx.abs() > 0.1 {
        vec![(fx + 4.0 * fx.signum(), -44.6 + b), (fx - 4.5 * fx.signum(), -44.8 + b)]
    } else {
        vec![(-5.0, -44.6 + b), (5.0, -44.6 + b)]
    };
    draw_eyes(p, &eyes, s.eyes);
    for x in [-6.5f32, 6.5] {
        let m = Cloud::new(x + fx, -35.0 + b, 6.5, 4.6, 5, 0.42, 16 + (x > 0.0) as u64);
        p.cloud(&m, INK, 1.0, FEATHER);
    }
    let mut beak = PathData::new();
    beak.move_to(fx - 3.0, -41.2 + b).quad_to(fx, -42.4 + b, fx + 3.0, -41.2 + b).line_to(fx, -36.2 + b).close();
    p.inked(&beak, BEAK, BEAK.shade(-0.45), 0.7);
    p.fill(&PathData::ellipse(fx, -51.5 + b, 5.0, 3.0), COMB);
    let crest = Cloud::new(fx * 0.5, -63.5 + b, 19.5, 12.0, 11, 0.36 + s.puff * 0.06, 19);
    p.cloud_lit(&crest, INK, 1.4, 5.0, -71.0 + b, &LIT);
    texture(p, fx * 0.5, -63.5 + b, 14.0, 7.0, 9, 21);
    p.pop();
}

fn back(p: &mut Painter, s: &Pose) {
    let b = s.bob;
    feet(p, [-11.0, 11.0], s.step);
    p.push(scale_about(1.0 / s.squash.sqrt(), s.squash, 0.0, -2.0));
    let crest = Cloud::new(0.0, -55.0 + b, 20.0, 14.0, 11, 0.36, 22);
    p.cloud_lit(&crest, INK, 1.4, 3.0, -63.0 + b, &LIT);
    let body = Cloud::new(0.0, -27.0 + b, 30.5, 24.0, 14, 0.3, 23);
    p.cloud_lit(&body, INK, 1.4, 2.0, -38.0 + b, &LIT);
    texture(p, 0.0, -22.0 + b, 25.0, 16.0, 18, 24);
    let tail = Cloud::new(0.0, -39.0 + b, 12.5, 10.5, 6, 0.45, 25);
    p.cloud_lit(&tail, INK, 1.3, 2.0, -45.0 + b, &LIT);
    p.pop();
}

pub fn draw(p: &mut Painter, s: &Pose, left: bool) {
    paint::shadow(p, 25.0 - s.jump.abs() * 0.4, 6.5);
    p.push(Affine::translate(0.0, s.jump));
    if left {
        p.push(mirror());
    }
    match s.view {
        View::Side => side(p, s),
        View::Front => front(p, s, 0.0),
        View::Turn => front(p, s, 6.0),
        View::Back => back(p, s),
    }
    if left {
        p.pop();
    }
    p.pop();
}

pub fn sheet(tile_px: u32) -> (SheetSpec, Pixmap) {
    let mut sb = SheetBuilder::new("hen", (112.0, 112.0), (56.0, 96.0), tile_px);
    let walk = |k: usize| {
        let step = [0.0f32, 1.0, 0.0, -1.0][k];
        (step, -0.5 - 2.5 * step.abs(), 1.0 + 0.03 * step.abs())
    };
    for (name, view, left) in [
        ("right", View::Side, false),
        ("left", View::Side, true),
        ("down", View::Front, false),
        ("up", View::Back, false),
    ] {
        for k in 0..4 {
            let (step, bob, squash) = walk(k);
            sb.frame(&format!("walk_{name}"), |p| {
                let mut s = Pose::new(view);
                s.step = step;
                s.bob = bob;
                s.squash = squash;
                draw(p, &s, left);
            });
        }
        for k in 0..2 {
            sb.frame(&format!("idle_{name}"), |p| {
                let mut s = Pose::new(view);
                s.squash = 1.0 + 0.025 * k as f32;
                s.bob = -0.6 * k as f32;
                draw(p, &s, left);
            });
        }
        if view != View::Back {
            sb.frame(&format!("blink_{name}"), |p| {
                let mut s = Pose::new(view);
                s.eyes = Eyes::Shut;
                draw(p, &s, left);
            });
            for dip in [0.45f32, 0.95, 0.6] {
                sb.frame(&format!("peck_{name}"), |p| {
                    let mut s = Pose::new(view);
                    if view == View::Front {
                        s.bob = 4.0 * dip;
                        s.squash = 1.0 - 0.06 * dip;
                    } else {
                        s.head_dip = dip;
                    }
                    draw(p, &s, left);
                });
            }
        }
    }
    for puff in [0.4f32, 0.8, 1.0] {
        sb.frame("fluff", |p| {
            let mut s = Pose::new(View::Front);
            s.puff = puff;
            s.wings = puff * 0.3;
            draw(p, &s, false);
        });
    }
    for (jump, puff) in [(-6.0f32, 0.6f32), (-2.0, 0.85)] {
        sb.frame("startled", |p| {
            let mut s = Pose::new(View::Front);
            s.puff = puff;
            s.jump = jump;
            s.eyes = Eyes::Wide;
            s.wings = 0.4;
            draw(p, &s, false);
        });
    }
    for (jump, wings) in [(0.0f32, 0.3f32), (-8.0, 0.8), (-12.0, 1.0), (-5.0, 0.6)] {
        sb.frame("celebrate", |p| {
            let mut s = Pose::new(View::Front);
            s.jump = jump;
            s.wings = wings;
            s.eyes = Eyes::Happy;
            s.squash = if jump == 0.0 { 0.94 } else { 1.04 };
            draw(p, &s, false);
        });
    }
    sb.frame("turn_right", |p| draw(p, &Pose::new(View::Turn), false));
    sb.frame("turn_left", |p| draw(p, &Pose::new(View::Turn), true));
    sb.frame("dizzy", |p| {
        let mut s = Pose::new(View::Front);
        s.eyes = Eyes::Dizzy;
        s.squash = 0.86;
        s.puff = 0.5;
        draw(p, &s, false);
    });
    sb.finish()
}
