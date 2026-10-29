//! Presentation: what the simulation looks like.
//!
//! Turns the game state into sprites (positions interpolated between ticks,
//! animations chosen from state, speed and distance walked), and keeps the
//! short-lived effects that events set off: emotes over heads, feathers,
//! dust, sparkles and score pop-ups. Effects use their own random numbers,
//! so the simulation stays deterministic however things are drawn.

use crate::render::{GateView, Layer, SheetId, Sprite};
use crate::sim::chick::ChickState;
use crate::sim::hawk::HawkState;
use crate::sim::items::ItemKind;
use crate::sim::predator::{PredState, PredatorKind};
use crate::sim::{Emote, Game, Phase, SimEvent};
use crate::gfx::scenery;
use crate::world::{Dir, TilePos};
use glam::Vec2;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct Particle {
    id: u32,
    anim: &'static str,
    pos: Vec2,
    vel: Vec2,
    gravity: f32,
    age: f32,
    life: f32,
    frame: usize,
}

#[derive(Clone, Debug)]
struct Popup {
    id: u32,
    text: String,
    pos: Vec2,
    age: f32,
}

pub struct Show {
    /// Effects are skipped on displays where animation is costly (e-ink).
    pub effects: bool,
    rng: ChaCha8Rng,
    pub time: f32,
    particles: Vec<Particle>,
    emotes: BTreeMap<u32, (Emote, f32)>,
    popups: Vec<Popup>,
    next_id: u32,
    sides: BTreeMap<u32, Dir>,
    last_facing: BTreeMap<u32, (Dir, f32)>,
    /// Remaining time of the hen's startled jump.
    startle: f32,
    /// When the hen last moved, for her idle fluff-up.
    hen_still_since: f32,
}

const FX_BASE: u32 = 1_000_000;
const POP_BASE: u32 = 2_000_000;

fn side_name(d: Dir) -> &'static str {
    match d {
        Dir::Left => "left",
        _ => "right",
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Up => "up",
        Dir::Down => "down",
        Dir::Left => "left",
        Dir::Right => "right",
    }
}

impl Default for Show {
    fn default() -> Self {
        Self::new()
    }
}

impl Show {
    pub fn new() -> Self {
        Show {
            effects: true,
            rng: ChaCha8Rng::seed_from_u64(0x5ca1_ab1e),
            time: 0.0,
            particles: Vec::new(),
            emotes: BTreeMap::new(),
            popups: Vec::new(),
            next_id: 0,
            sides: BTreeMap::new(),
            last_facing: BTreeMap::new(),
            startle: 0.0,
            hen_still_since: 0.0,
        }
    }

    /// A fox has just spotted the hen nearby.
    pub fn startle(&mut self) {
        self.startle = 0.5;
    }

    pub fn clear(&mut self) {
        self.particles.clear();
        self.emotes.clear();
        self.popups.clear();
        self.startle = 0.0;
        self.hen_still_since = self.time;
    }

    fn id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1) % 900_000;
        self.next_id
    }

    fn burst(&mut self, anim: &'static str, at: Vec2, n: usize, speed: f32, gravity: f32, life: f32) {
        if !self.effects {
            return;
        }
        for _ in 0..n {
            let a: f32 = self.rng.random_range(0.0..std::f32::consts::TAU);
            let s: f32 = self.rng.random_range(0.4..1.0) * speed;
            let id = self.id();
            let frame = self.rng.random_range(0..3);
            self.particles.push(Particle {
                id,
                anim,
                pos: at,
                vel: Vec2::new(a.cos() * s, a.sin() * s - speed * 0.4),
                gravity,
                age: 0.0,
                life: life * self.rng.random_range(0.7..1.2),
                frame,
            });
        }
    }

    fn one(&mut self, anim: &'static str, at: Vec2, vel: Vec2, life: f32) {
        let id = self.id();
        self.particles.push(Particle {
            id,
            anim,
            pos: at,
            vel,
            gravity: 0.0,
            age: 0.0,
            life,
            frame: 0,
        });
    }

    fn popup(&mut self, text: String, at: Vec2) {
        let id = self.id();
        self.popups.push(Popup {
            id,
            text,
            pos: at,
            age: 0.0,
        });
    }

    pub fn on_event(&mut self, e: &SimEvent) {
        match e {
            SimEvent::Emote { entity, emote } => {
                self.emotes.insert(*entity, (*emote, 1.4));
            }
            SimEvent::ChickJoined { pos, .. } => {
                self.one("heart", *pos - Vec2::new(0.0, 0.6), Vec2::new(0.0, -0.9), 0.9);
                self.burst("sparkle", *pos - Vec2::new(0.0, 0.3), 3, 1.5, 0.0, 0.5);
            }
            SimEvent::ChickHomed { pos, .. } => self.burst("sparkle", *pos, 4, 1.8, 0.0, 0.6),
            SimEvent::BatchScored { count, points, pos } => {
                self.popup(format!("+{points}"), *pos - Vec2::new(0.0, 1.2));
                if *count > 1 {
                    self.popup(format!("x{count}"), *pos - Vec2::new(0.0, 0.4));
                }
                self.burst("sparkle", *pos - Vec2::new(0.0, 0.5), 6 + *count as usize * 2, 3.0, 0.0, 0.9);
            }
            SimEvent::ChickGrabbed { pos, .. } => self.burst("feather", *pos, 3, 2.0, 1.5, 0.8),
            SimEvent::ChickRescued { pos, points, .. } => {
                self.popup(format!("+{points}"), *pos - Vec2::new(0.0, 1.0));
                self.burst("star", *pos - Vec2::new(0.0, 0.5), 4, 2.2, 0.0, 0.8);
            }
            SimEvent::ChickLost { pos, .. } => self.burst("feather", *pos, 4, 1.6, 1.2, 1.0),
            SimEvent::ItemCollected { points, pos, .. } => {
                self.popup(format!("+{points}"), *pos - Vec2::new(0.0, 0.8));
                self.burst("sparkle", *pos, 3, 1.5, 0.0, 0.5);
            }
            SimEvent::HenCaught { pos, .. } => self.burst("feather", *pos - Vec2::new(0.0, 0.4), 14, 3.5, 2.0, 1.6),
            SimEvent::HenStunned { pos } => self.burst("star", *pos - Vec2::new(0.0, 1.0), 4, 1.2, 0.0, 1.2),
            SimEvent::HenRespawned { pos } => self.burst("sparkle", *pos, 6, 2.0, 0.0, 0.7),
            SimEvent::FenceBuilt { tiles, rejected, .. } => {
                for t in tiles {
                    self.burst("dust", t.center(), 2, 1.0, 0.0, 0.5);
                }
                for (t, _) in rejected {
                    self.one("reject", t.center(), Vec2::ZERO, 0.8);
                }
            }
            SimEvent::FenceRefused { rejected, .. } => {
                for (t, _) in rejected {
                    self.one("reject", t.center(), Vec2::ZERO, 0.8);
                }
            }
            SimEvent::FenceDamaged { tile, .. } => self.burst("dust", tile.center(), 4, 1.5, 0.0, 0.6),
            SimEvent::HawkStrike { pos, took } => {
                self.burst("dust", *pos, 5, 2.0, 0.0, 0.6);
                if took.is_some() {
                    self.burst("feather", *pos, 5, 2.2, 1.0, 1.0);
                }
            }
            SimEvent::LevelCleared { .. } => {}
            _ => {}
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.startle = (self.startle - dt).max(0.0);
        for p in &mut self.particles {
            p.age += dt;
            p.vel.y += p.gravity * dt;
            p.vel *= 1.0 - 1.5 * dt;
            p.pos += p.vel * dt;
        }
        self.particles.retain(|p| p.age < p.life);
        for (_, t) in self.emotes.values_mut() {
            *t -= dt;
        }
        self.emotes.retain(|_, (_, t)| *t > 0.0);
        for p in &mut self.popups {
            p.age += dt;
        }
        self.popups.retain(|p| p.age < 1.3);
    }

    /// A celebration shower, for the level-cleared screen.
    pub fn confetti(&mut self, area: Vec2) {
        if self.rng.random_bool(0.35) {
            let at = Vec2::new(self.rng.random_range(1.0..area.x - 1.0), self.rng.random_range(1.0..area.y - 1.0));
            self.burst("sparkle", at, 3, 2.0, 0.0, 0.8);
        }
    }

    /// Everything that moves, with positions interpolated by `alpha`
    /// between the previous tick and the current one.
    pub fn sprites(&mut self, g: &Game, alpha: f32) -> Vec<Sprite> {
        let mut v = Vec::with_capacity(64);
        let lerp = |a: Vec2, b: Vec2| a.lerp(b, alpha.clamp(0.0, 1.0));
        let t = self.time;
        let mut heads: BTreeMap<u32, Vec2> = BTreeMap::new();

        // Items.
        for it in &g.items {
            let anim = match it.kind {
                ItemKind::Worm => "worm",
                ItemKind::Corn => "corn",
                ItemKind::Snail => "snail",
                ItemKind::GoldenEgg => "golden_egg",
            };
            v.push(Sprite {
                key: it.id,
                sheet: SheetId::Items,
                anim: anim.into(),
                frame: (t * 3.0) as usize,
                pos: it.pos,
                layer: Layer::Body,
                z_bias: -2,
            });
        }

        // The hen.
        let h = &g.hen;
        let hpos = lerp(h.mover.prev_pos, h.mover.pos);
        let moving = h.mover.pos.distance(h.mover.prev_pos) > 1e-4;
        let facing = h.mover.facing;
        let side = *self.sides.entry(h.id).or_insert(Dir::Right);
        if facing.is_horizontal() {
            self.sides.insert(h.id, facing);
        }
        let turned = {
            let e = self.last_facing.entry(h.id).or_insert((facing, -1.0));
            if e.0 != facing {
                *e = (facing, t);
            }
            t - e.1 < 0.09 && facing == Dir::Down
        };
        let blink_hide = h.invulnerable > 0.0 && ((t * 10.0) as i32) % 2 == 0;
        if moving || !h.alive {
            self.hen_still_since = t;
        }
        // Standing about, she fluffs herself up every few seconds.
        let still = t - self.hen_still_since;
        let fluffing = still > 3.0 && (still - 3.0) % 5.0 < 0.75;
        let (anim, frame): (String, usize) = if !h.alive {
            ("dizzy".into(), 0)
        } else if matches!(g.phase, Phase::Cleared { .. }) {
            ("celebrate".into(), (t * 8.0) as usize)
        } else if h.stun > 0.0 {
            ("dizzy".into(), 0)
        } else if self.startle > 0.0 {
            // A jump, then landing with her feathers still up.
            ("startled".into(), usize::from(self.startle < 0.3))
        } else if fluffing {
            // Swell, then settle again.
            let k = ((still - 3.0) % 5.0 / 0.15) as usize;
            ("fluff".into(), [0usize, 1, 2, 1, 0][k.min(4)])
        } else if h.peck < 0.36 {
            let d = if facing == Dir::Up { Dir::Down } else { facing };
            (format!("peck_{}", dir_name(d)), ((h.peck / 0.12) as usize).min(2))
        } else if turned {
            (format!("turn_{}", side_name(side)), 0)
        } else if moving {
            (format!("walk_{}", dir_name(facing)), (h.mover.odometer / 0.2) as usize)
        } else if facing != Dir::Up && ((t + 0.37) % 3.3) < 0.13 {
            (format!("blink_{}", dir_name(facing)), 0)
        } else {
            (format!("idle_{}", dir_name(facing)), (t * 1.6) as usize)
        };
        if !blink_hide {
            v.push(Sprite {
                key: h.id,
                sheet: SheetId::Hen,
                anim,
                frame,
                pos: hpos,
                layer: Layer::Body,
                z_bias: 0,
            });
        }
        heads.insert(h.id, hpos - Vec2::new(0.0, 1.5));

        // Chicks.
        for c in &g.chicks {
            if !c.is_active() {
                continue;
            }
            let pos = lerp(c.prev_pos, c.pos);
            let speed = c.vel.length();
            let col = c.traits.color;
            let side = *self.sides.entry(c.id).or_insert(Dir::Right);
            if c.facing.is_horizontal() {
                self.sides.insert(c.id, c.facing);
            }
            let phase = c.traits.phase;
            let (anim, frame, z_bias) = match &c.state {
                ChickState::Carried { .. } => ("carried".to_string(), 0, 4000),
                ChickState::Hiding { .. } => ("hide".into(), 0, 0),
                ChickState::Frightened { .. } if speed < 1.5 => ("scared".into(), (t * 10.0) as usize, 0),
                _ if c.joy > 0.0 => ("happy".into(), (t * 7.0) as usize, 0),
                _ if c.pecking > 0.0 => (format!("peck_{}", side_name(side)), ((0.45 - c.pecking) / 0.2) as usize % 2, 0),
                _ if speed > 0.25 => (
                    format!("walk_{}", dir_name(c.facing)),
                    (c.odometer / 0.11) as usize,
                    0,
                ),
                _ => ("idle".into(), (t * 1.5 + phase) as usize, 0),
            };
            v.push(Sprite {
                key: c.id,
                sheet: SheetId::Chick,
                anim: format!("{col}:{anim}"),
                frame,
                pos,
                layer: Layer::Body,
                z_bias,
            });
            heads.insert(c.id, pos - Vec2::new(0.0, 0.85));
            // Whatever caught its eye.
            if let ChickState::Distracted { target, item: None, .. } = &c.state {
                let butterfly = c.id % 2 == 0;
                let flutter = if butterfly { (t * 5.0 + phase).sin() * 0.12 } else { 0.0 };
                v.push(Sprite {
                    key: FX_BASE + 950_000 + c.id % 50_000,
                    sheet: SheetId::Items,
                    anim: if butterfly { "butterfly" } else { "beetle" }.into(),
                    frame: (t * if butterfly { 9.0 } else { 6.0 }) as usize,
                    pos: *target + Vec2::new(0.0, flutter),
                    layer: Layer::Body,
                    z_bias: -1,
                });
            }
        }

        // Predators.
        for p in &g.predators {
            let pos = lerp(p.mover.prev_pos, p.mover.pos);
            let moving = p.mover.pos.distance(p.mover.prev_pos) > 1e-4;
            let speed = p.mover.pos.distance(p.mover.prev_pos) / crate::sim::rules::DT;
            let f = p.mover.facing;
            if f.is_horizontal() {
                self.sides.insert(p.id, f);
            }
            let side = side_name(*self.sides.get(&p.id).unwrap_or(&Dir::Right));
            let stride = (p.mover.odometer / 0.24) as usize;
            let (sheet, anim, frame) = match p.kind {
                PredatorKind::Fox => {
                    let (a, fr) = match p.state {
                        PredState::Stunned { .. } => ("stunned".to_string(), (t * 4.0) as usize),
                        PredState::Alert { .. } => {
                            if f.is_horizontal() {
                                (format!("alert_{side}"), 0)
                            } else {
                                ("idle_down".into(), 0)
                            }
                        }
                        PredState::Gnaw { .. } => (format!("sniff_{side}"), (t * 6.0) as usize),
                        _ if !moving => {
                            if matches!(p.state, PredState::Investigate { .. }) {
                                (format!("sniff_{side}"), (t * 3.0) as usize)
                            } else if f.is_horizontal() {
                                (format!("walk_{side}"), 0)
                            } else {
                                ("idle_down".into(), (t * 1.2) as usize)
                            }
                        }
                        _ if f.is_horizontal() && speed > 3.0 => (format!("run_{side}"), stride),
                        _ => (format!("walk_{}", dir_name(f)), stride),
                    };
                    (SheetId::Fox, a, fr)
                }
                PredatorKind::Weasel => {
                    let a = match (p.state, f) {
                        (PredState::Retreat { .. }, d) if d.is_horizontal() => format!("scared_{side}"),
                        (_, Dir::Up) => "run_up".into(),
                        (_, Dir::Down) => "run_down".into(),
                        _ => format!("run_{side}"),
                    };
                    (SheetId::Weasel, a, if moving { (p.mover.odometer / 0.2) as usize } else { 0 })
                }
            };
            v.push(Sprite {
                key: p.id,
                sheet,
                anim,
                frame,
                pos,
                layer: Layer::Body,
                z_bias: 0,
            });
            heads.insert(p.id, pos - Vec2::new(0.0, if p.kind == PredatorKind::Fox { 1.3 } else { 0.8 }));
            if matches!(p.state, PredState::Stunned { .. }) {
                for k in 0..3 {
                    let a = t * 3.0 + k as f32 * 2.1;
                    v.push(Sprite {
                        key: FX_BASE + 900_000 + p.id * 4 + k,
                        sheet: SheetId::Fx,
                        anim: "star".into(),
                        frame: 0,
                        pos: pos + Vec2::new(a.cos() * 0.45, -1.1 + a.sin() * 0.15),
                        layer: Layer::Over,
                        z_bias: 0,
                    });
                }
            }
        }

        // The hawk: a shadow sliding over its prey, then the stoop.
        if let Some(hk) = &g.hawk {
            let pos = lerp(hk.prev_pos, hk.pos);
            match hk.state {
                HawkState::Circling { .. } | HawkState::Diving { .. } => {
                    v.push(Sprite {
                        key: hk.id,
                        sheet: SheetId::Hawk,
                        anim: "shadow".into(),
                        frame: (t * 3.0) as usize,
                        pos,
                        layer: Layer::Decal,
                        z_bias: 0,
                    });
                    if matches!(hk.state, HawkState::Diving { .. }) {
                        v.push(Sprite {
                            key: hk.id + 1,
                            sheet: SheetId::Hawk,
                            anim: "dive".into(),
                            frame: 0,
                            pos: pos - Vec2::new(0.0, 0.4),
                            layer: Layer::Over,
                            z_bias: 0,
                        });
                    }
                }
                HawkState::Leaving { .. } => {
                    v.push(Sprite {
                        key: hk.id + 1,
                        sheet: SheetId::Hawk,
                        anim: "fly".into(),
                        frame: (t * 8.0) as usize,
                        pos: pos - Vec2::new(0.0, 0.6),
                        layer: Layer::Over,
                        z_bias: 0,
                    });
                }
                HawkState::Away { .. } => {}
            }
        }

        // Emotes over heads.
        for (&entity, &(emote, left)) in &self.emotes {
            let Some(&head) = heads.get(&entity) else { continue };
            let anim = match emote {
                Emote::Alert => "emote_alert",
                Emote::Puzzled => "emote_puzzled",
                Emote::Heart => "emote_heart",
                Emote::Fright => "emote_fright",
                Emote::Note => "emote_note",
                Emote::Sweat => "emote_sweat",
                Emote::Angry => "emote_angry",
            };
            let pop = ((1.4 - left) * 10.0).min(1.0);
            v.push(Sprite {
                key: entity,
                sheet: SheetId::Fx,
                anim: anim.into(),
                frame: 0,
                pos: head - Vec2::new(0.0, 0.15 * pop),
                layer: Layer::Over,
                z_bias: 10,
            });
        }

        for p in &self.particles {
            v.push(Sprite {
                key: FX_BASE + p.id,
                sheet: SheetId::Fx,
                anim: p.anim.into(),
                frame: p.frame,
                pos: p.pos,
                layer: Layer::Over,
                z_bias: 5,
            });
        }
        for pop in &self.popups {
            let n = pop.text.chars().count() as f32;
            let rise = pop.pos - Vec2::new(0.0, pop.age * 0.9);
            let mut x = -(n - 1.0) * 0.18;
            for (i, ch) in pop.text.chars().enumerate() {
                v.push(Sprite {
                    key: POP_BASE + pop.id * 16 + i as u32,
                    sheet: SheetId::Glyphs,
                    anim: ch.to_string(),
                    frame: 0,
                    pos: rise + Vec2::new(x, 0.0),
                    layer: Layer::Over,
                    z_bias: 20,
                });
                x += 0.36 * crate::gfx::title::glyph_advance(ch) / 0.72;
            }
        }
        v
    }

    pub fn gates(g: &Game) -> Vec<GateView> {
        g.gates
            .iter()
            .enumerate()
            .map(|(index, gate)| GateView {
                index,
                gate: gate.clone(),
                stage: (gate.swing * 3.0).round().clamp(0.0, 3.0) as u8,
            })
            .collect()
    }

    pub fn fences(g: &Game) -> Vec<(TilePos, u8)> {
        let mut v = Vec::new();
        for f in &g.fences {
            let stage = scenery::wear_stage(f);
            for &t in &f.tiles {
                v.push((t, stage));
            }
        }
        v
    }
}
