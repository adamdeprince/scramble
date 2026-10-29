//! Silkie chicks: fluffballs with a tuft on top, big dark eyes and feet
//! like tiny powder puffs, in four plumage colours.

use super::SheetBuilder;
use super::paint::{self, Cloud, Painter, mirror, rot_about, scale_about};
use crate::gfx::sprites::SheetSpec;
use crate::gfx::vector::{Affine, Color, PathData};
use tiny_skia::Pixmap;

/// (fill, shade, ink) per plumage colour: buttercup, white, buff, smoke.
pub const PLUMAGE: [(Color, Color, Color); 4] = [
    (Color::hex(0xffe896), Color::hex(0xf0c968), Color::hex(0x9a7838)),
    (Color::hex(0xfffaf0), Color::hex(0xe6ddcb), Color::hex(0x7c7165)),
    (Color::hex(0xedbb84), Color::hex(0xd0965c), Color::hex(0x7a522e)),
    (Color::hex(0x77707e), Color::hex(0x56505d), Color::hex(0x2b262f)),
];
const BEAK: Color = Color::hex(0x8a90a6);
const EYE: Color = Color::hex(0x1b1420);

#[derive(Clone, Copy, PartialEq)]
enum View {
    Side,
    Front,
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum Eyes {
    Open,
    Wide,
    Happy,
    Peek,
    Dizzy,
}

#[derive(Clone, Copy)]
struct Pose {
    view: View,
    bob: f32,
    step: f32,
    dip: f32,
    squash: f32,
    wings: f32,
    jump: f32,
    tilt: f32,
    eyes: Eyes,
}

impl Pose {
    fn new(view: View) -> Self {
        Pose {
            view,
            bob: 0.0,
            step: 0.0,
            dip: 0.0,
            squash: 1.0,
            wings: 0.0,
            jump: 0.0,
            tilt: 0.0,
            eyes: Eyes::Open,
        }
    }
}

fn eyes(p: &mut Painter, pts: &[(f32, f32)], e: Eyes, dark: bool) {
    let ring = if dark { Some(Color::hex(0xd9d2e0)) } else { None };
    for &(x, y) in pts {
        if let Some(r) = ring {
            if e != Eyes::Happy && e != Eyes::Dizzy {
                p.fill(&PathData::circle(x, y, 2.6), r);
            }
        }
        match e {
            Eyes::Open => paint::eye(p, x, y, 2.0, 1.0, EYE),
            Eyes::Peek => paint::eye(p, x, y, 2.0, 0.55, EYE),
            Eyes::Wide => {
                p.fill(&PathData::circle(x, y, 2.8), Color::rgb(255, 255, 255));
                p.fill(&PathData::circle(x, y, 1.3), EYE);
            }
            Eyes::Happy => paint::happy_eye(p, x, y, 2.0, if dark { Color::hex(0xe8e2ee) } else { EYE }),
            Eyes::Dizzy => {
                let mut d = PathData::new();
                d.move_to(x - 1.7, y - 1.7).line_to(x + 1.7, y + 1.7).move_to(x + 1.7, y - 1.7).line_to(x - 1.7, y + 1.7);
                p.stroke(&d, if dark { Color::hex(0xe8e2ee) } else { EYE }, 1.0);
            }
        }
    }
}

fn draw(p: &mut Painter, s: &Pose, color: usize, left: bool) {
    let (fill, shade, ink) = PLUMAGE[color];
    let dark = color == 3;
    let lit = [(0.0, fill.shade(0.18)), (0.55, fill), (1.0, shade)];
    paint::shadow(p, 12.0 - s.jump.abs() * 0.3, 3.5);
    p.push(Affine::translate(0.0, s.jump).then(rot_about(s.tilt, 0.0, -12.0)));
    if left {
        p.push(mirror());
    }
    let b = s.bob;
    p.push(scale_about(1.0 / s.squash.sqrt(), s.squash, 0.0, 0.0));
    // Feet.
    let feet_x: [f32; 2] = if s.view == View::Side {
        [-4.0 - 2.5 * s.step, 4.0 + 2.5 * s.step]
    } else {
        [-5.0, 5.0]
    };
    for (i, x) in feet_x.into_iter().enumerate() {
        let lift = if i == 0 { s.step.max(0.0) } else { (-s.step).max(0.0) } * 2.0;
        let c = Cloud::new(x, -2.5 - lift, 4.2, 2.9, 4, 0.45, 40 + i as u64);
        p.cloud(&c, ink, 0.9, shade);
    }
    match s.view {
        View::Side => {
            let body = Cloud::new(-1.0, -12.0 + b, 12.5, 10.5, 9, 0.36, 41);
            p.cloud_lit(&body, ink, 1.1, 2.0, -18.0 + b, &lit);
            // Wing nub, raised in a fright.
            p.push(rot_about(-50.0 * s.wings, 0.0, -14.0 + b));
            let wing = PathData::smooth_closed(&[(2.0, -15.0 + b), (-6.0, -15.0 + b), (-10.0, -10.0 + b), (-2.0, -9.0 + b)], 1.0);
            p.fill(&wing, shade);
            p.pop();
            p.push(rot_about(40.0 * s.dip, 3.0, -15.0 + b).then(Affine::translate(1.5 * s.dip, 3.0 * s.dip)));
            let head = Cloud::new(7.0, -21.0 + b, 9.0, 8.0, 7, 0.4, 42);
            p.cloud_lit(&head, ink, 1.1, 8.0, -26.0 + b, &lit);
            let mut beak = PathData::new();
            beak.move_to(14.5, -22.0 + b).line_to(18.5, -20.3 + b).line_to(14.5, -18.8 + b).close();
            p.fill(&beak, BEAK);
            eyes(p, &[(11.2, -22.3 + b)], s.eyes, dark);
            let tuft = Cloud::new(4.0, -28.5 + b, 5.0, 3.6, 5, 0.45, 43);
            p.cloud_lit(&tuft, ink, 1.0, 5.0, -31.0 + b, &lit);
            p.pop();
        }
        View::Front => {
            for (side, x) in [(-1.0f32, -11.5f32), (1.0, 11.5)] {
                p.push(rot_about(side * 60.0 * s.wings, x * 0.6, -14.0 + b));
                let w = Cloud::new(x, -12.0 + b, 4.5, 6.5, 4, 0.45, 44 + (side > 0.0) as u64);
                p.cloud(&w, ink, 0.9, shade.mix(fill, 0.4));
                p.pop();
            }
            let body = Cloud::new(0.0, -12.0 + b, 13.0, 10.5, 9, 0.36, 46);
            p.cloud_lit(&body, ink, 1.1, 2.0, -18.0 + b, &lit);
            let head = Cloud::new(0.0, -21.5 + b, 10.0, 8.2, 8, 0.4, 47);
            p.cloud_lit(&head, ink, 1.1, 2.0, -27.0 + b, &lit);
            eyes(p, &[(-3.9, -22.0 + b), (3.9, -22.0 + b)], s.eyes, dark);
            let mut beak = PathData::new();
            beak.move_to(-1.8, -19.6 + b).line_to(1.8, -19.6 + b).line_to(0.0, -16.6 + b).close();
            p.fill(&beak, BEAK);
            let tuft = Cloud::new(0.0, -29.5 + b, 5.2, 3.6, 5, 0.45, 48);
            p.cloud_lit(&tuft, ink, 1.0, 1.0, -32.0 + b, &lit);
        }
        View::Back => {
            let body = Cloud::new(0.0, -12.0 + b, 13.0, 10.5, 9, 0.36, 49);
            p.cloud_lit(&body, ink, 1.1, 1.0, -17.0 + b, &lit);
            let head = Cloud::new(0.0, -21.0 + b, 9.5, 8.0, 8, 0.4, 50);
            p.cloud_lit(&head, ink, 1.1, 1.0, -26.0 + b, &lit);
            let tuft = Cloud::new(0.0, -28.0 + b, 5.0, 3.5, 5, 0.45, 51);
            p.cloud_lit(&tuft, ink, 1.0, 1.0, -31.0 + b, &lit);
        }
    }
    p.pop();
    if left {
        p.pop();
    }
    p.pop();
}

pub fn sheet(tile_px: u32) -> (SheetSpec, Pixmap) {
    let mut sb = SheetBuilder::new("chick", (52.0, 52.0), (26.0, 44.0), tile_px);
    for color in 0..PLUMAGE.len() {
        let a = |name: &str| format!("{color}:{name}");
        for (name, view, left) in [("right", View::Side, false), ("left", View::Side, true)] {
            for k in 0..3 {
                let step = [0.0f32, 1.0, -1.0][k];
                sb.frame(&a(&format!("walk_{name}")), |p| {
                    let mut s = Pose::new(view);
                    s.step = step;
                    s.bob = -1.5 * step.abs();
                    draw(p, &s, color, left);
                });
            }
            for dip in [0.6f32, 1.0] {
                sb.frame(&a(&format!("peck_{name}")), |p| {
                    let mut s = Pose::new(view);
                    s.dip = dip;
                    draw(p, &s, color, left);
                });
            }
        }
        for (name, view) in [("down", View::Front), ("up", View::Back)] {
            for k in 0..2 {
                sb.frame(&a(&format!("walk_{name}")), |p| {
                    let mut s = Pose::new(view);
                    s.step = if k == 0 { 1.0 } else { -1.0 };
                    s.bob = -1.2;
                    draw(p, &s, color, false);
                });
            }
        }
        for k in 0..2 {
            sb.frame(&a("idle"), |p| {
                let mut s = Pose::new(View::Front);
                s.squash = 1.0 + 0.04 * k as f32;
                draw(p, &s, color, false);
            });
        }
        for k in 0..2 {
            sb.frame(&a("scared"), |p| {
                let mut s = Pose::new(View::Front);
                s.eyes = Eyes::Wide;
                s.wings = if k == 0 { 0.9 } else { 0.3 };
                s.jump = if k == 0 { -3.0 } else { 0.0 };
                draw(p, &s, color, false);
            });
        }
        sb.frame(&a("hide"), |p| {
            let mut s = Pose::new(View::Front);
            s.squash = 0.72;
            s.eyes = Eyes::Peek;
            draw(p, &s, color, false);
        });
        for k in 0..2 {
            sb.frame(&a("happy"), |p| {
                let mut s = Pose::new(View::Front);
                s.eyes = Eyes::Happy;
                s.jump = if k == 0 { -5.0 } else { 0.0 };
                s.wings = if k == 0 { 0.7 } else { 0.2 };
                draw(p, &s, color, false);
            });
        }
        sb.frame(&a("carried"), |p| {
            let mut s = Pose::new(View::Front);
            s.eyes = Eyes::Dizzy;
            s.tilt = 25.0;
            s.wings = 0.8;
            s.jump = -4.0;
            draw(p, &s, color, false);
        });
    }
    sb.finish()
}
