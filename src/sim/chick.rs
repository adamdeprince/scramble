//! The chicks: autonomous little agents with a few moods.
//!
//! Each chick decides for itself every tick. A chick in the brood follows the
//! creature ahead of it in the procession (the hen, or the next chick
//! closer to her) with arrival steering, keeps its distance from its
//! siblings, and falls back on the hen's flow field when walls hide whoever
//! it is following. A fright sends it under the hen's feathers, into tall
//! grass, or simply away; once things are quiet it creeps back. Now and then
//! a beetle catches its eye. None of this replays the hen's path: the
//! procession is whatever the chicks make of it.

use super::rules::*;
use super::steer;
use super::{Emote, Game, SimEvent, items};
use crate::world::nav::{self, DistField};
use crate::world::{Dir, Mover, TilePos, Topology};
use glam::Vec2;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Number of plumage colours the art provides.
pub const CHICK_COLORS: u8 = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChickTraits {
    /// Above 1 is braver: predators must come closer before it panics.
    pub bravery: f32,
    /// How easily beetles and worms distract it.
    pub curiosity: f32,
    /// Walking speed multiplier.
    pub pace: f32,
    pub color: u8,
    /// Personal phase for idle animation, so the brood does not bob in step.
    pub phase: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FleeMode {
    /// Run to the hen and hide under her.
    Huddle,
    /// Make for tall grass or a tree's shade.
    Cover,
    /// Just get away.
    Run,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChickState {
    /// Not yet gathered: potters about near home.
    Wild { target: Vec2, idle: f32 },
    /// Part of the procession behind the hen.
    Following,
    Frightened { threat: Vec2, calm: f32, mode: FleeMode },
    /// Crouched in cover until the danger has passed.
    Hiding { calm: f32, time: f32 },
    /// Heading back to the hen after a fright or a distraction.
    Rejoining,
    /// Chasing a beetle (no item) or pecking at an item.
    Distracted { target: Vec2, item: Option<u32>, peck: f32, time: f32 },
    /// Running in through the coop door.
    Homing,
    Delivered,
    /// In a predator's mouth.
    Carried { by: u32 },
    Lost,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chick {
    pub id: u32,
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub vel: Vec2,
    pub state: ChickState,
    /// Belongs to the hen's brood (as opposed to still wild).
    pub rescued: bool,
    pub home: Vec2,
    pub traits: ChickTraits,
    pub facing: Dir,
    /// Remaining time of a peck animation.
    pub pecking: f32,
    /// Seconds spent too far from the hen.
    pub stray: f32,
    pub stray_warned: bool,
    pub distract_cd: f32,
    /// Remaining time of a happy hop.
    pub joy: f32,
    pub odometer: f32,
}

impl Chick {
    pub fn new(id: u32, spawn: TilePos, rng: &mut ChaCha8Rng) -> Self {
        let jitter = Vec2::new(rng.random_range(-0.25..0.25), rng.random_range(-0.25..0.25));
        let pos = spawn.center() + jitter;
        Chick {
            id,
            pos,
            prev_pos: pos,
            vel: Vec2::ZERO,
            state: ChickState::Wild {
                target: pos,
                idle: rng.random_range(0.2..1.5),
            },
            rescued: false,
            home: spawn.center(),
            traits: ChickTraits {
                bravery: rng.random_range(0.78..1.3),
                curiosity: rng.random_range(0.4..1.7),
                pace: rng.random_range(0.93..1.07),
                color: rng.random_range(0..CHICK_COLORS),
                phase: rng.random_range(0.0..std::f32::consts::TAU),
            },
            facing: Dir::Down,
            pecking: 0.0,
            stray: 0.0,
            stray_warned: false,
            distract_cd: rng.random_range(3.0..DISTRACT_COOLDOWN),
            joy: 0.0,
            odometer: 0.0,
        }
    }

    /// Still out in the yard (or in a mouth): not home and not lost.
    pub fn is_active(&self) -> bool {
        !matches!(self.state, ChickState::Delivered | ChickState::Lost)
    }

    pub fn is_on_ground(&self) -> bool {
        self.is_active() && !matches!(self.state, ChickState::Carried { .. })
    }

    pub fn is_wild(&self) -> bool {
        !self.rescued && self.is_on_ground()
    }

    pub fn in_brood(&self) -> bool {
        self.rescued && self.is_on_ground()
    }

    pub fn is_hidden(&self, topo: &Topology) -> bool {
        matches!(self.state, ChickState::Hiding { .. }) && topo.grid.sheltered(TilePos::of(self.pos))
    }

    pub fn join_brood(&mut self) {
        self.rescued = true;
        self.state = ChickState::Following;
        self.stray = 0.0;
        self.stray_warned = false;
        self.joy = 0.8;
    }

    /// Sends the chick running from `from`; it forgets the hen until she
    /// gathers it again.
    pub fn scatter(&mut self, from: Vec2) {
        self.rescued = false;
        self.state = ChickState::Frightened {
            threat: from,
            calm: 0.0,
            mode: FleeMode::Run,
        };
    }

    /// Dropped by a predator the hen has driven off: it is hers again.
    pub fn drop_rescued(&mut self, at: Vec2, threat: Vec2) {
        self.pos = at;
        self.prev_pos = at;
        self.vel = Vec2::ZERO;
        self.rescued = true;
        self.state = ChickState::Frightened {
            threat,
            calm: 0.0,
            mode: FleeMode::Huddle,
        };
    }
}

/// A predator or hawk shadow, as a chick perceives it.
#[derive(Clone, Copy, Debug)]
struct Threat {
    pos: Vec2,
    /// Hawk shadows only frighten chicks out in the open.
    from_above: bool,
    radius_scale: f32,
}

enum Leader {
    Hen,
    Chick(usize),
}

/// Updates every chick. `live` is false during the countdown, when nothing
/// can frighten them.
pub fn update_all(game: &mut Game, live: bool) {
    let mut chicks = std::mem::take(&mut game.chicks);
    let mut events = Vec::new();
    let mut homed = Vec::new();
    let mut pecked_items: Vec<(u32, usize)> = Vec::new();
    let mut new_panic = Vec::new();

    let topo = &game.topo;
    let hen_pos = game.hen.pos();
    let hen_vel = game.hen.velocity();
    let hen_alive = game.hen.alive;
    let hen_speed = game.hen_speed();
    let door = game.door_center();
    let hen_near_door = hen_alive && hen_pos.distance(door) < HOMING_HEN_DIST;
    let fields = Fields {
        hen: &game.nav.hen_field,
        door: &game.nav.door_field,
        cover: &game.nav.cover_field,
    };

    let threats: Vec<Threat> = if live {
        let mut t: Vec<Threat> = game
            .predators
            .iter()
            .filter(|p| p.is_menacing())
            .map(|p| Threat {
                pos: p.mover.pos,
                from_above: false,
                radius_scale: p.fear_scale(),
            })
            .collect();
        if let Some(h) = &game.hawk {
            if let Some(shadow) = h.shadow() {
                t.push(Threat {
                    pos: shadow,
                    from_above: true,
                    radius_scale: 1.0,
                });
            }
        }
        t
    } else {
        Vec::new()
    };
    let items: Vec<(u32, Vec2)> = game.items.iter().map(|i| (i.id, i.pos)).collect();

    let snap: Vec<(Vec2, Vec2, bool)> = chicks.iter().map(|c| (c.pos, c.vel, c.is_on_ground())).collect();

    // Panic spreads one hop per tick from chicks that were frightened last
    // tick to calm neighbours.
    let panic_sources = std::mem::take(&mut game.panic);
    if live {
        for c in chicks.iter_mut() {
            if !c.is_on_ground() || !calm_state(&c.state) {
                continue;
            }
            for &(at, threat) in &panic_sources {
                if at.distance(c.pos) < PANIC_SPREAD_RADIUS && at.distance(c.pos) > 1e-3 && game.rng.random_bool(PANIC_SPREAD_CHANCE) {
                    let mode = choose_flee(topo, &fields, c.pos, threat, hen_pos, hen_alive, false);
                    c.state = ChickState::Frightened { threat, calm: 0.0, mode };
                    events.push(SimEvent::ChickScared { chick: c.id, pos: c.pos });
                    break;
                }
            }
        }
    }

    // The procession: brood chicks ordered by how far they are from the hen
    // along the paths. Each follows the one ahead of it.
    let mut order: Vec<usize> = (0..chicks.len())
        .filter(|&i| chicks[i].state == ChickState::Following)
        .collect();
    let hen_tile = TilePos::of(hen_pos);
    let key = |p: Vec2| {
        if TilePos::of(p) == hen_tile {
            p.distance(hen_pos) * 0.5
        } else {
            fields.hen.at_point(p)
        }
    };
    order.sort_by(|&a, &b| key(snap[a].0).total_cmp(&key(snap[b].0)).then(chicks[a].id.cmp(&chicks[b].id)));
    let mut leader: Vec<Leader> = (0..chicks.len()).map(|_| Leader::Hen).collect();
    for k in 1..order.len() {
        leader[order[k]] = Leader::Chick(order[k - 1]);
    }
    let mut distracted = chicks
        .iter()
        .filter(|c| matches!(c.state, ChickState::Distracted { .. }))
        .count();

    for i in 0..chicks.len() {
        let c = &mut chicks[i];
        if !c.is_on_ground() {
            continue;
        }
        c.pecking = (c.pecking - DT).max(0.0);
        c.joy = (c.joy - DT).max(0.0);
        c.distract_cd -= DT;

        let sheltered = topo.grid.sheltered(TilePos::of(c.pos));
        let near_hen = hen_alive && c.pos.distance(hen_pos) < 1.5;
        let mut seen: Option<(Vec2, f32)> = None;
        for t in &threats {
            if t.from_above && sheltered {
                continue;
            }
            let mut r = if t.from_above { HAWK_FEAR_RADIUS } else { FEAR_RADIUS / c.traits.bravery };
            r *= t.radius_scale;
            if c.rescued && near_hen && !t.from_above {
                r *= 0.75;
            }
            let d = t.pos.distance(c.pos);
            if d < r && (t.from_above || nav::line_of_sight(c.pos, t.pos, |p| topo.blocks_sight(p))) && seen.is_none_or(|(_, sd)| d < sd) {
                seen = Some((t.pos, d));
            }
        }
        let hawk_threat = seen.is_some_and(|(p, _)| threats.iter().any(|t| t.from_above && t.pos == p));

        // React to danger.
        if let Some((threat, dist)) = seen {
            match &mut c.state {
                ChickState::Wild { .. } | ChickState::Following | ChickState::Rejoining | ChickState::Distracted { .. } => {
                    let mode = choose_flee(topo, &fields, c.pos, threat, hen_pos, hen_alive, hawk_threat);
                    if matches!(c.state, ChickState::Distracted { .. }) {
                        distracted = distracted.saturating_sub(1);
                    }
                    c.state = ChickState::Frightened { threat, calm: 0.0, mode };
                    events.push(SimEvent::ChickScared { chick: c.id, pos: c.pos });
                    events.push(SimEvent::Emote {
                        entity: c.id,
                        emote: Emote::Fright,
                    });
                    new_panic.push((c.pos, threat));
                }
                ChickState::Frightened { threat: t, calm, .. } => {
                    *t = threat;
                    *calm = 0.0;
                }
                ChickState::Hiding { calm, .. } => {
                    if dist < 1.0 && !hawk_threat {
                        c.state = ChickState::Frightened {
                            threat,
                            calm: 0.0,
                            mode: FleeMode::Run,
                        };
                    } else {
                        *calm = 0.0;
                    }
                }
                ChickState::Homing if dist < 1.0 && !hawk_threat => {
                    c.state = ChickState::Frightened {
                        threat,
                        calm: 0.0,
                        mode: FleeMode::Run,
                    };
                }
                _ => {}
            }
        }

        let follow_speed = (hen_speed * CHICK_SPEED_FACTOR).min(CHICK_MAX_SPEED) * c.traits.pace;
        let mut desired = Vec2::ZERO;
        let mut max_speed = follow_speed;
        let mut sep_radius = SEPARATION_RADIUS;
        let mut flock_forces = false;

        match c.state.clone() {
            ChickState::Wild { mut target, mut idle } => {
                max_speed = CHICK_WANDER_SPEED;
                if idle > 0.0 {
                    idle -= DT;
                    if c.pecking <= 0.0 && game.rng.random_bool(0.012) {
                        c.pecking = 0.45;
                    }
                } else if c.pos.distance(target) < 0.12 {
                    idle = game.rng.random_range(0.6..2.6);
                    target = pick_wander(topo, c.pos, c.home, &mut game.rng);
                } else {
                    desired = steer::arrive(c.pos, target, max_speed, 0.4, 0.05);
                }
                c.state = ChickState::Wild { target, idle };
            }
            ChickState::Following => {
                flock_forces = true;
                let (lpos, lvel, gap) = match leader[i] {
                    Leader::Hen => (hen_pos, hen_vel, HEN_GAP),
                    Leader::Chick(j) => (snap[j].0, snap[j].1, FOLLOW_GAP),
                };
                let d = c.pos.distance(lpos);
                let visible = d < 3.5 && nav::clear_walk(topo, c.pos, lpos, CHICK_RADIUS * 0.8, Mover::Chick);
                if visible {
                    let lspeed = lvel.length();
                    let behind = if lspeed > 0.3 { lpos - lvel / lspeed * gap } else { lpos };
                    let (target, stop) = if lspeed > 0.3 && nav::clear_walk(topo, c.pos, behind, CHICK_RADIUS * 0.8, Mover::Chick) {
                        (behind, 0.0)
                    } else {
                        (lpos, gap)
                    };
                    desired = steer::pursue(c.pos, target, lvel, max_speed, stop, 3.5);
                    if d < gap * 0.85 {
                        desired *= 0.25;
                    }
                } else {
                    let target = steer::flow_target(topo, fields.hen, c.pos, CHICK_RADIUS, 4, Mover::Chick).unwrap_or(hen_pos);
                    desired = steer::seek(c.pos, target, max_speed);
                }
                if hen_near_door && fields.door.get(TilePos::of(c.pos)) <= HOMING_CHICK_PATH {
                    c.state = ChickState::Homing;
                } else if live
                    && seen.is_none()
                    && distracted < MAX_DISTRACTED
                    && c.distract_cd <= 0.0
                    && !hen_near_door
                    && game.rng.random_bool((DISTRACT_CHANCE_PER_SEC * c.traits.curiosity as f64 * DT as f64).min(1.0))
                {
                    if let Some((target, item)) = find_distraction(topo, c.pos, &items, &mut game.rng) {
                        c.state = ChickState::Distracted {
                            target,
                            item,
                            peck: 0.0,
                            time: 0.0,
                        };
                        c.distract_cd = DISTRACT_COOLDOWN;
                        distracted += 1;
                        events.push(SimEvent::Emote {
                            entity: c.id,
                            emote: Emote::Note,
                        });
                        if item.is_none() {
                            events.push(SimEvent::ChickFoundBug { chick: c.id, pos: target });
                        }
                    } else {
                        c.distract_cd = 2.0;
                    }
                }
                track_stray(c, fields.hen, &mut events);
            }
            ChickState::Frightened { threat, mut calm, mode } => {
                max_speed = CHICK_FLEE_SPEED * c.traits.pace;
                if seen.is_none() {
                    calm += DT;
                }
                let mut mode = mode;
                match mode {
                    FleeMode::Huddle => {
                        if !hen_alive {
                            mode = FleeMode::Run;
                        } else {
                            let angle = c.id as f32 * 2.399;
                            let spot = hen_pos + Vec2::new(angle.cos(), angle.sin()) * 0.32;
                            desired = steer::arrive(c.pos, spot, max_speed, 0.5, 0.05);
                            sep_radius = 0.22;
                        }
                    }
                    FleeMode::Cover => {
                        let tile = TilePos::of(c.pos);
                        if topo.grid.sheltered(tile) && c.pos.distance(tile.center()) < 0.4 {
                            c.state = ChickState::Hiding { calm, time: 0.0 };
                            c.vel *= 0.3;
                        } else {
                            match steer::flow_target(topo, fields.cover, c.pos, CHICK_RADIUS, 3, Mover::Chick) {
                                Some(t) => desired = steer::seek(c.pos, t, max_speed),
                                None if topo.grid.sheltered(tile) => {
                                    desired = steer::arrive(c.pos, tile.center(), max_speed, 0.4, 0.0);
                                }
                                None => mode = FleeMode::Run,
                            }
                        }
                    }
                    FleeMode::Run => {
                        desired = flee_direction(topo, c.pos, c.vel, threat) * max_speed;
                    }
                }
                if let ChickState::Frightened { .. } = c.state {
                    if calm >= CALM_TIME {
                        c.state = if c.rescued {
                            ChickState::Rejoining
                        } else {
                            ChickState::Wild {
                                target: c.pos,
                                idle: 1.0,
                            }
                        };
                        if !c.rescued {
                            c.home = TilePos::of(c.pos).center();
                        }
                    } else {
                        c.state = ChickState::Frightened { threat, calm, mode };
                    }
                }
            }
            ChickState::Hiding { mut calm, mut time } => {
                time += DT;
                if seen.is_none() {
                    calm += DT;
                }
                max_speed = 1.0;
                sep_radius = 0.0;
                let tile = TilePos::of(c.pos);
                desired = steer::arrive(c.pos, tile.center(), 0.8, 0.4, 0.1);
                if calm >= CALM_TIME && time >= HIDE_MIN_TIME {
                    c.state = if c.rescued {
                        ChickState::Rejoining
                    } else {
                        c.home = tile.center();
                        ChickState::Wild {
                            target: c.pos,
                            idle: 1.5,
                        }
                    };
                } else {
                    c.state = ChickState::Hiding { calm, time };
                }
            }
            ChickState::Rejoining => {
                max_speed = follow_speed * 0.9;
                flock_forces = true;
                let target = if c.pos.distance(hen_pos) < 3.0 && nav::clear_walk(topo, c.pos, hen_pos, CHICK_RADIUS, Mover::Chick) {
                    hen_pos
                } else {
                    steer::flow_target(topo, fields.hen, c.pos, CHICK_RADIUS, 4, Mover::Chick).unwrap_or(hen_pos)
                };
                desired = steer::arrive(c.pos, target, max_speed, 0.6, 0.4);
                let close_to_following = order.iter().any(|&j| j != i && snap[j].0.distance(c.pos) < 0.7);
                if hen_alive && (c.pos.distance(hen_pos) < 1.3 || close_to_following) {
                    c.state = ChickState::Following;
                }
                track_stray(c, fields.hen, &mut events);
            }
            ChickState::Distracted {
                target,
                item,
                mut peck,
                mut time,
            } => {
                time += DT;
                max_speed = 2.6 * c.traits.pace;
                let item_gone = item.is_some_and(|id| !items.iter().any(|(iid, _)| *iid == id));
                if c.pos.distance(target) < 0.22 || peck > 0.0 {
                    peck += DT;
                    desired = Vec2::ZERO;
                    if c.pecking <= 0.0 {
                        c.pecking = 0.4;
                    }
                    if let Some(id) = item {
                        if !item_gone && peck > 0.3 {
                            pecked_items.push((id, i));
                        }
                    }
                } else {
                    desired = steer::arrive(c.pos, target, max_speed, 0.3, 0.1);
                }
                if peck >= DISTRACT_PECK_TIME || time >= DISTRACT_GIVE_UP || (item_gone && peck <= 0.0) {
                    c.state = ChickState::Rejoining;
                    distracted = distracted.saturating_sub(1);
                } else {
                    c.state = ChickState::Distracted { target, item, peck, time };
                }
            }
            ChickState::Homing => {
                max_speed = follow_speed;
                let target = if nav::clear_walk(topo, c.pos, door, CHICK_RADIUS, Mover::Chick) {
                    door
                } else {
                    steer::flow_target(topo, fields.door, c.pos, CHICK_RADIUS, 4, Mover::Chick).unwrap_or(door)
                };
                desired = steer::seek(c.pos, target, max_speed);
                sep_radius = 0.2;
                if c.pos.distance(door) < DELIVER_RADIUS {
                    homed.push(i);
                }
            }
            ChickState::Delivered | ChickState::Carried { .. } | ChickState::Lost => continue,
        }

        // Local forces: personal space, the hen's feathers, walls, and (in
        // the procession) a little alignment and cohesion with neighbours.
        let mut force = Vec2::ZERO;
        if sep_radius > 0.0 {
            let others = snap
                .iter()
                .enumerate()
                .filter(|&(j, s)| j != i && s.2)
                .map(|(_, s)| s.0);
            force += steer::separation(c.pos, others, sep_radius) * 6.0;
            if hen_alive && !matches!(c.state, ChickState::Frightened { mode: FleeMode::Huddle, .. }) {
                force += steer::separation(c.pos, std::iter::once(hen_pos), 0.48) * 7.0;
            }
        }
        if flock_forces {
            let near = |s: &&(Vec2, Vec2, bool)| s.2 && s.0.distance(c.pos) < 1.0 && s.0 != c.pos;
            force += steer::alignment(c.vel, snap.iter().filter(near).map(|s| s.1)) * 0.25;
            force += steer::cohesion(c.pos, snap.iter().filter(near).map(|s| s.0)) * 0.6;
        }
        force += steer::wall_avoid(topo, c.pos, CHICK_RADIUS, 0.18, Mover::Chick) * 3.0;
        // A little personal wobble so the brood never marches in lockstep.
        let wobble = (game.tick as f32 * 0.11 + c.traits.phase).sin();
        force += Vec2::new(-desired.y, desired.x) * 0.08 * wobble;

        c.vel = steer::integrate(c.vel, desired, force, CHICK_ACCEL, max_speed, DT);
        let next = c.pos + c.vel * DT;
        let (resolved, normal) = topo.resolve_circle(next, CHICK_RADIUS, Mover::Chick);
        if normal.length_squared() > 1e-10 {
            let n = normal.normalize();
            let into = c.vel.dot(n);
            if into < 0.0 {
                c.vel -= n * into;
            }
        }
        c.odometer += resolved.distance(c.pos);
        c.pos = resolved;
        if c.vel.length() > 0.35 {
            if let Some(d) = Dir::from_vec(c.vel) {
                // Hysteresis: only turn the sprite when the new direction
                // clearly dominates.
                let keep = c.facing.vec().dot(c.vel.normalize());
                if keep < 0.55 {
                    c.facing = d;
                }
            }
        }
    }

    game.chicks = chicks;
    game.events.extend(events);
    game.panic = new_panic;
    for i in homed {
        game.chick_home(i);
    }
    for (item, chick) in pecked_items {
        items::collect_by_chick(game, item, chick);
    }
}

struct Fields<'a> {
    hen: &'a DistField,
    door: &'a DistField,
    cover: &'a DistField,
}

fn calm_state(s: &ChickState) -> bool {
    matches!(
        s,
        ChickState::Wild { .. } | ChickState::Following | ChickState::Rejoining | ChickState::Distracted { .. }
    )
}

fn choose_flee(topo: &Topology, f: &Fields, pos: Vec2, threat: Vec2, hen: Vec2, hen_alive: bool, from_above: bool) -> FleeMode {
    if !from_above && hen_alive && pos.distance(hen) < 2.4 && hen.distance(threat) > 1.0 && nav::clear_walk(topo, pos, hen, CHICK_RADIUS, Mover::Chick) {
        return FleeMode::Huddle;
    }
    let tile = TilePos::of(pos);
    let cd = f.cover.get(tile);
    if cd <= 6 {
        if cd == 0 {
            return FleeMode::Cover;
        }
        if let Some(next) = f.cover.downhill(tile) {
            let step = (next.center() - pos).normalize_or_zero();
            let toward = (threat - pos).normalize_or_zero();
            if step.dot(toward) < 0.5 {
                return FleeMode::Cover;
            }
        }
    }
    FleeMode::Run
}

/// The best unit direction away from a threat that does not run into a
/// wall, preferring to keep going the way the chick is already going.
fn flee_direction(topo: &Topology, pos: Vec2, vel: Vec2, threat: Vec2) -> Vec2 {
    let away = (pos - threat).normalize_or_zero();
    let heading = vel.normalize_or_zero();
    let mut best = (f32::NEG_INFINITY, Vec2::ZERO);
    for k in 0..16 {
        let a = k as f32 * std::f32::consts::TAU / 16.0;
        let d = Vec2::new(a.cos(), a.sin());
        let probe = pos + d * 0.55;
        if topo.circle_blocked(probe, CHICK_RADIUS, Mover::Chick) || !nav::clear_walk(topo, pos, probe, CHICK_RADIUS, Mover::Chick) {
            continue;
        }
        let score = d.dot(away) * 1.0 + d.dot(heading) * 0.35 + (probe.distance(threat) - pos.distance(threat)) * 0.5;
        if score > best.0 {
            best = (score, d);
        }
    }
    best.1
}

fn pick_wander(topo: &Topology, pos: Vec2, home: Vec2, rng: &mut ChaCha8Rng) -> Vec2 {
    for _ in 0..8 {
        let off = Vec2::new(rng.random_range(-2.0..2.0), rng.random_range(-2.0..2.0));
        let t = home + off;
        if !topo.circle_blocked(t, CHICK_RADIUS + 0.05, Mover::Chick) && nav::clear_walk(topo, pos, t, CHICK_RADIUS, Mover::Chick) {
            return t;
        }
    }
    home
}

fn find_distraction(topo: &Topology, pos: Vec2, items: &[(u32, Vec2)], rng: &mut ChaCha8Rng) -> Option<(Vec2, Option<u32>)> {
    let mut best: Option<(f32, u32, Vec2)> = None;
    for &(id, ipos) in items {
        let d = ipos.distance(pos);
        if d < DISTRACT_RANGE && nav::clear_walk(topo, pos, ipos, CHICK_RADIUS, Mover::Chick) && best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, id, ipos));
        }
    }
    if let Some((_, id, p)) = best {
        return Some((p, Some(id)));
    }
    for _ in 0..6 {
        let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
        let r: f32 = rng.random_range(0.8..1.7);
        let t = pos + Vec2::new(a.cos(), a.sin()) * r;
        if !topo.circle_blocked(t, CHICK_RADIUS + 0.04, Mover::Chick) && nav::clear_walk(topo, pos, t, CHICK_RADIUS, Mover::Chick) {
            return Some((t, None));
        }
    }
    None
}

fn track_stray(c: &mut Chick, hen_field: &DistField, events: &mut Vec<SimEvent>) {
    let d = hen_field.get(TilePos::of(c.pos));
    if d > STRAY_DIST {
        c.stray += DT;
        if c.stray > STRAY_TIME * 0.5 && !c.stray_warned {
            c.stray_warned = true;
            events.push(SimEvent::Emote {
                entity: c.id,
                emote: Emote::Puzzled,
            });
        }
        if c.stray > STRAY_TIME {
            c.rescued = false;
            c.home = TilePos::of(c.pos).center();
            c.state = ChickState::Wild { target: c.pos, idle: 1.0 };
            c.stray = 0.0;
            c.stray_warned = false;
            events.push(SimEvent::ChickStrayed { chick: c.id, pos: c.pos });
        }
    } else {
        c.stray = (c.stray - DT).max(0.0);
        if c.stray == 0.0 {
            c.stray_warned = false;
        }
    }
}
