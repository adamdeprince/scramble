//! Foxes and weasels.
//!
//! Predators only know what they perceive: what is inside their field of
//! view and not hidden by a hedge, what is right under their nose, and the
//! peeping of a brood (louder the bigger it is) or of a panicking chick.
//! A fox that spots something freezes for a moment with a "!" before giving
//! chase, which is the player's warning. While chasing it aims where the hen
//! is going rather than where she is, the more so the more cunning it is,
//! and a second fox may lie in wait on the way to the coop when the hen is
//! bringing a big brood home. It remembers where it last saw its quarry for
//! a while, then goes to sniff about there.
//!
//! A predator with a chick in its mouth heads for home and cannot bite; the
//! hen can make it drop the chick by catching up with it. Weasels never
//! threaten the hen, only her chicks, use the drain pipes, and bolt when she
//! comes at them.

use super::chick::ChickState;
use super::mover::LaneMover;
use super::rules::*;
use super::{Emote, Game, SimEvent, fence};
use crate::world::nav;
use crate::world::{Dir, Mover, Tile, TilePos, Topology};
use glam::Vec2;
use rand::RngExt;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredatorKind {
    Fox,
    Weasel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Hen,
    Chick(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PredState {
    /// Wandering the yard, now and then stopping to look about.
    Prowl { goal: TilePos, pause: f32 },
    /// Just spotted something: a short freeze before the chase.
    Alert { t: f32, target: Target },
    Chase { target: Target },
    /// Going to where something was heard or last seen, then sniffing.
    Investigate { spot: TilePos, sniff: f32 },
    /// Taking a chick home.
    Carry,
    /// Pecked by the hen.
    Stunned { t: f32 },
    /// Going home for a rest.
    Retreat { t: f32 },
    /// Chewing through a fence that is in the way.
    Gnaw { tile: TilePos, t: f32, target: Target },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub pos: Vec2,
    pub target: Target,
    pub age: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Predator {
    pub id: u32,
    pub kind: PredatorKind,
    pub mover: LaneMover,
    pub home: TilePos,
    pub state: PredState,
    pub carrying: Option<u32>,
    pub memory: Option<Memory>,
    /// Index among predators of its kind: the second fox may flank.
    pub role: u32,
    /// Where it is heading, for diagnostics overlays.
    pub goal: Option<TilePos>,
    /// The last spot it went to sniff, and how long ago: it will not be
    /// lured back there by the same noise straight away.
    pub last_spot: Option<(TilePos, f32)>,
    route: Vec<TilePos>,
    route_key: (TilePos, TilePos, u64),
}

impl Predator {
    pub fn new(id: u32, kind: PredatorKind, home: TilePos, role: u32) -> Self {
        let who = match kind {
            PredatorKind::Fox => Mover::Fox,
            PredatorKind::Weasel => Mover::Weasel,
        };
        Predator {
            id,
            kind,
            mover: LaneMover::new(home, who),
            home,
            state: PredState::Retreat { t: 1.0 + role as f32 * 1.5 },
            carrying: None,
            memory: None,
            role,
            goal: None,
            last_spot: None,
            route: Vec::new(),
            route_key: (home, home, u64::MAX),
        }
    }

    /// Whether chicks should be afraid of it.
    pub fn is_menacing(&self) -> bool {
        !matches!(self.state, PredState::Stunned { .. })
    }

    pub fn fear_scale(&self) -> f32 {
        match (self.kind, self.carrying.is_some()) {
            (_, true) => 0.6,
            (PredatorKind::Fox, false) => 1.0,
            (PredatorKind::Weasel, false) => 0.85,
        }
    }

    pub fn can_bite_hen(&self) -> bool {
        self.kind == PredatorKind::Fox
            && self.carrying.is_none()
            && !matches!(self.state, PredState::Stunned { .. } | PredState::Retreat { .. } | PredState::Carry)
    }

    pub fn can_grab(&self) -> bool {
        self.carrying.is_none() && !matches!(self.state, PredState::Stunned { .. } | PredState::Retreat { .. })
    }

    /// After the hen has been caught: back to the den to start over.
    pub fn send_home(&mut self, _topo: &Topology) {
        if self.carrying.is_some() {
            return;
        }
        self.mover.place(self.home);
        self.state = PredState::Retreat { t: 1.5 + self.role as f32 };
        self.memory = None;
        self.route.clear();
        self.route_key.2 = u64::MAX;
    }

    fn mover_kind(&self) -> Mover {
        self.mover.who
    }

    /// Direction of the next step towards `goal`, using a cached route.
    fn step_towards(&mut self, topo: &Topology, goal: TilePos) -> Option<Dir> {
        let here = self.mover.tile();
        self.goal = Some(goal);
        if here == goal {
            // Settle on the tile centre.
            let to = goal.center() - self.mover.pos;
            return if to.length() > 0.05 { Dir::from_vec(to) } else { None };
        }
        let key = (here, goal, topo.version);
        if self.route_key != key {
            self.route = nav::route(topo, here, goal, self.mover_kind()).unwrap_or_default();
            self.route_key = key;
        }
        let next = *self.route.first()?;
        dir_between(here, next)
    }
}

fn dir_between(a: TilePos, b: TilePos) -> Option<Dir> {
    Dir::ALL.into_iter().find(|&d| a.step(d) == b)
}

struct Seen {
    target: Target,
    pos: Vec2,
    utility: f32,
}

fn sees(topo: &Topology, from: Vec2, facing: Dir, to: Vec2, range: f32, cone_cos: f32) -> bool {
    let d = to - from;
    let dist = d.length();
    if dist > range {
        return false;
    }
    if dist < FOX_CLOSE_SENSE {
        return true;
    }
    (d / dist.max(1e-5)).dot(facing.vec()) >= cone_cos
        && nav::line_of_sight(from, to, |p| topo.blocks_sight(p))
}

/// The tile the hen will reach after `k` more tiles straight ahead.
fn predict_hen(topo: &Topology, tile: TilePos, dir: Dir, k: u32) -> TilePos {
    let mut t = tile;
    for _ in 0..k {
        let n = t.step(dir);
        if topo.passable(n, Mover::Fox) {
            t = n;
        } else {
            break;
        }
    }
    t
}

pub fn update_all(game: &mut Game) {
    let mut preds = std::mem::take(&mut game.predators);
    let mut events = Vec::new();
    let mut gnawed = Vec::new();
    let mut delivered_home = Vec::new();
    let cunning = game.level.cunning as f32;
    let brood = game.brood_size();
    {
        let Game {
            ref topo,
            ref chicks,
            ref hen,
            ref mut rng,
            ref nav,
            ref fences,
            ..
        } = *game;
        let hen_pos = hen.pos();
        let hen_tile = hen.mover.tile();
        let hen_dir = hen.mover.dir.unwrap_or(hen.mover.facing);
        let hen_targetable = hen.alive && hen.invulnerable <= 0.0;
        let range = FOX_VISION + FOX_VISION_PER_CUNNING * cunning;
        let cone_cos = (75.0f32 + 5.0 * cunning).to_radians().cos();
        let memory_time = FOX_MEMORY + FOX_MEMORY_PER_CUNNING * cunning;
        let alert_time = (FOX_ALERT_TIME - FOX_ALERT_PER_CUNNING * cunning).max(0.25);

        // What could be heard: the brood around the hen, and panicking chicks.
        let mut noises: Vec<(Vec2, f32)> = Vec::new();
        if hen.alive && brood > 0 {
            noises.push((hen_pos, NOISE_BASE + NOISE_PER_CHICK * brood as f32));
        }
        for c in chicks.iter() {
            if matches!(c.state, ChickState::Frightened { .. }) {
                noises.push((c.pos, NOISE_PANIC));
            }
        }

        for p in preds.iter_mut() {
            let pos = p.mover.pos;
            let regions = match p.kind {
                PredatorKind::Fox => &nav.fox_regions,
                PredatorKind::Weasel => &nav.weasel_regions,
            };
            let here = p.mover.tile();
            // Quarry out of reach (in the coop doorway, behind a closed
            // gate) is not worth chasing: it would only make the predator
            // camp beside it.
            let reachable = |at: Vec2| regions.connected(here, TilePos::of(at));
            // Perception.
            let mut best: Option<Seen> = None;
            let mut hen_visible = false;
            if hen_targetable && sees(topo, pos, p.mover.facing, hen_pos, range, cone_cos) && reachable(hen_pos) {
                hen_visible = true;
                if p.kind == PredatorKind::Fox {
                    let u = 1.0 / (1.0 + 0.3 * pos.distance(hen_pos));
                    best = Some(Seen {
                        target: Target::Hen,
                        pos: hen_pos,
                        utility: u,
                    });
                }
            }
            for c in chicks.iter() {
                if !c.is_on_ground() || c.is_hidden(topo) || !reachable(c.pos) {
                    continue;
                }
                if !sees(topo, pos, p.mover.facing, c.pos, range, cone_cos) {
                    continue;
                }
                let protected = hen.alive && c.pos.distance(hen_pos) < 1.6;
                let value = match (p.kind, protected) {
                    (PredatorKind::Fox, true) => 0.55,
                    (PredatorKind::Fox, false) => 1.25,
                    (PredatorKind::Weasel, true) => 0.25,
                    (PredatorKind::Weasel, false) => 1.4,
                };
                let u = value / (1.0 + 0.3 * pos.distance(c.pos));
                if best.as_ref().is_none_or(|b| u > b.utility) {
                    best = Some(Seen {
                        target: Target::Chick(c.id),
                        pos: c.pos,
                        utility: u,
                    });
                }
            }
            let target_pos = |t: Target| -> Option<Vec2> {
                match t {
                    Target::Hen => hen_targetable.then_some(hen_pos),
                    Target::Chick(id) => chicks.iter().find(|c| c.id == id && c.is_on_ground()).map(|c| c.pos),
                }
            };
            let visible = |t: Target| -> Option<Vec2> {
                let tp = target_pos(t)?;
                (sees(topo, pos, p.mover.facing, tp, range, cone_cos) && reachable(tp)).then_some(tp)
            };

            // A weasel bolts when the hen comes at it.
            if p.kind == PredatorKind::Weasel
                && p.carrying.is_none()
                && hen.alive
                && !matches!(p.state, PredState::Retreat { .. })
                && (pos.distance(hen_pos) < 1.4 || (hen_visible && pos.distance(hen_pos) < WEASEL_SCARE_DIST))
            {
                p.state = PredState::Retreat { t: WEASEL_LURK_TIME };
                events.push(SimEvent::Emote {
                    entity: p.id,
                    emote: Emote::Fright,
                });
            }

            if let Some((_, age)) = &mut p.last_spot {
                *age += DT;
            }
            let mut want = None;
            let mut speed;
            let chase_speed = match p.kind {
                PredatorKind::Fox => FOX_CHASE_SPEED + FOX_SPEED_PER_CUNNING * cunning,
                PredatorKind::Weasel => WEASEL_SPEED,
            };
            let prowl_speed = match p.kind {
                PredatorKind::Fox => FOX_PATROL_SPEED + FOX_SPEED_PER_CUNNING * cunning * 0.5,
                PredatorKind::Weasel => WEASEL_SPEED * 0.7,
            };

            match p.state {
                PredState::Stunned { t } => {
                    speed = 0.0;
                    p.state = if t - DT <= 0.0 {
                        PredState::Retreat { t: 2.0 }
                    } else {
                        PredState::Stunned { t: t - DT }
                    };
                }
                PredState::Retreat { t } => {
                    speed = chase_speed * 0.9;
                    want = p.step_towards(topo, p.home);
                    let t = t - DT;
                    if t <= 0.0 && p.mover.tile() == p.home {
                        let goal = pick_prowl_goal(topo, p, chicks, cunning, rng);
                        p.state = PredState::Prowl { goal, pause: 0.5 };
                    } else {
                        p.state = PredState::Retreat { t };
                    }
                }
                PredState::Carry => {
                    speed = chase_speed * FOX_CARRY_FACTOR;
                    want = p.step_towards(topo, p.home);
                    if p.mover.tile() == p.home {
                        if let Some(c) = p.carrying.take() {
                            delivered_home.push((c, pos));
                        }
                        p.state = PredState::Retreat { t: 2.0 };
                    }
                }
                PredState::Gnaw { tile, t, target } => {
                    speed = 0.0;
                    p.goal = Some(tile);
                    if topo.dyn_at(tile).fence.is_none() {
                        p.state = PredState::Chase { target };
                    } else if t - DT <= 0.0 {
                        gnawed.push((tile, p.id));
                        p.state = PredState::Chase { target };
                    } else {
                        p.state = PredState::Gnaw { tile, t: t - DT, target };
                        if let Some(d) = Dir::from_vec(tile.center() - pos) {
                            p.mover.facing = d;
                        }
                    }
                }
                PredState::Alert { t, target } => {
                    speed = 0.0;
                    if let Some(tp) = target_pos(target) {
                        if let Some(d) = Dir::from_vec(tp - pos) {
                            p.mover.facing = d;
                        }
                    }
                    p.state = if t - DT <= 0.0 {
                        PredState::Chase { target }
                    } else {
                        PredState::Alert { t: t - DT, target }
                    };
                }
                PredState::Chase { target } => {
                    speed = chase_speed;
                    // Switch to a juicier target in plain view.
                    let mut target = target;
                    if let Some(b) = &best {
                        if b.target != target && b.utility > 1.4 * utility_of(p, target, &target_pos, hen_pos, hen.alive) {
                            target = b.target;
                        }
                    }
                    match visible(target) {
                        Some(tp) => p.memory = Some(Memory { pos: tp, target, age: 0.0 }),
                        None => {
                            if let Some(m) = &mut p.memory {
                                m.age += DT;
                            }
                        }
                    }
                    let memory = p.memory.filter(|m| m.target == target && m.age <= memory_time);
                    match memory {
                        None => {
                            let spot = p.memory.map_or(p.mover.tile(), |m| TilePos::of(m.pos));
                            let spot = reachable_near(topo, regions, here, spot, p.mover.who);
                            p.last_spot = Some((spot, 0.0));
                            p.state = PredState::Investigate {
                                spot,
                                sniff: FOX_SNIFF_TIME,
                            };
                            events.push(SimEvent::Emote {
                                entity: p.id,
                                emote: Emote::Puzzled,
                            });
                        }
                        Some(m) => {
                            let fresh = m.age < 0.05;
                            let goal = if fresh {
                                chase_goal(topo, p, target, m.pos, hen_tile, hen_dir, brood, cunning, nav)
                            } else {
                                TilePos::of(m.pos)
                            };
                            p.state = PredState::Chase { target };
                            if p.mover.tile() == TilePos::of(m.pos) || pos.distance(m.pos) < 0.9 {
                                // Close enough to lunge along the lane.
                                want = lane_lunge(topo, p, m.pos);
                                p.goal = Some(TilePos::of(m.pos));
                            } else {
                                want = p.step_towards(topo, goal);
                                if want.is_none() && goal != TilePos::of(m.pos) {
                                    want = p.step_towards(topo, TilePos::of(m.pos));
                                }
                                if want.is_none() && !fences.is_empty() {
                                    if let Some((d, gnaw)) = fence_detour(topo, p, TilePos::of(m.pos)) {
                                        want = d;
                                        if let Some(tile) = gnaw {
                                            p.state = PredState::Gnaw {
                                                tile,
                                                t: FOX_GNAW_TIME,
                                                target,
                                            };
                                            events.push(SimEvent::Emote {
                                                entity: p.id,
                                                emote: Emote::Angry,
                                            });
                                            want = None;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PredState::Investigate { spot, sniff } => {
                    speed = prowl_speed * 1.15;
                    if let Some(b) = &best {
                        p.state = PredState::Alert {
                            t: alert_time,
                            target: b.target,
                        };
                        p.memory = Some(Memory {
                            pos: b.pos,
                            target: b.target,
                            age: 0.0,
                        });
                        events.push(SimEvent::Emote {
                            entity: p.id,
                            emote: Emote::Alert,
                        });
                    } else if p.mover.tile() == spot || p.step_towards(topo, spot).is_none() {
                        speed = 0.0;
                        if sniff - DT <= 0.0 {
                            let goal = pick_prowl_goal(topo, p, chicks, cunning, rng);
                            p.state = PredState::Prowl { goal, pause: 0.0 };
                        } else {
                            p.state = PredState::Investigate { spot, sniff: sniff - DT };
                        }
                    } else {
                        want = p.step_towards(topo, spot);
                    }
                }
                PredState::Prowl { goal, pause } => {
                    speed = prowl_speed;
                    if let Some(b) = &best {
                        p.state = PredState::Alert {
                            t: alert_time,
                            target: b.target,
                        };
                        p.memory = Some(Memory {
                            pos: b.pos,
                            target: b.target,
                            age: 0.0,
                        });
                        events.push(SimEvent::Emote {
                            entity: p.id,
                            emote: Emote::Alert,
                        });
                    } else if let Some((at, _)) = noises
                        .iter()
                        .filter(|(at, r)| at.distance(pos) < *r && reachable(*at))
                        .filter(|(at, _)| {
                            p.last_spot
                                .is_none_or(|(spot, age)| age > 8.0 || spot.center().distance(*at) > 2.5)
                        })
                        .min_by(|a, b| a.0.distance(pos).total_cmp(&b.0.distance(pos)))
                    {
                        let spot = reachable_near(topo, regions, here, TilePos::of(*at), p.mover.who);
                        p.last_spot = Some((spot, 0.0));
                        p.state = PredState::Investigate {
                            spot,
                            sniff: FOX_SNIFF_TIME * 0.6,
                        };
                        events.push(SimEvent::Emote {
                            entity: p.id,
                            emote: Emote::Puzzled,
                        });
                    } else if pause > 0.0 {
                        speed = 0.0;
                        p.state = PredState::Prowl { goal, pause: pause - DT };
                    } else if p.mover.tile() == goal || p.step_towards(topo, goal).is_none() {
                        let goal = pick_prowl_goal(topo, p, chicks, cunning, rng);
                        let pause = if rng.random_bool(0.35) { rng.random_range(0.4..1.4) } else { 0.0 };
                        p.state = PredState::Prowl { goal, pause };
                    } else {
                        want = p.step_towards(topo, goal);
                    }
                }
            }

            if p.kind == PredatorKind::Weasel && topo.grid.get(p.mover.tile()) == Tile::Pipe {
                speed = speed.max(WEASEL_PIPE_SPEED * if speed > 0.0 { 1.0 } else { 0.0 });
            }
            if speed > 0.0 {
                p.mover.advance(topo, want, speed * DT, 0.45);
            } else if let Some(d) = want {
                p.mover.facing = d;
            }
        }
    }

    // Carried chicks dangle from mouths.
    for p in &preds {
        if let Some(cid) = p.carrying {
            if let Some(c) = game.chicks.iter_mut().find(|c| c.id == cid) {
                c.pos = p.mover.pos + p.mover.facing.vec() * 0.32 + Vec2::new(0.0, -0.05);
                c.facing = p.mover.facing;
            }
        }
    }
    game.predators = preds;
    game.events.extend(events);
    for (tile, by) in gnawed {
        fence::damage(game, tile, Some(by));
    }
    for (cid, pos) in delivered_home {
        if let Some(c) = game.chicks.iter_mut().find(|c| c.id == cid) {
            c.state = ChickState::Lost;
            game.lost += 1;
            game.events.push(SimEvent::ChickLost { chick: cid, pos });
        }
    }
}

fn utility_of(p: &Predator, t: Target, target_pos: &impl Fn(Target) -> Option<Vec2>, hen_pos: Vec2, hen_alive: bool) -> f32 {
    let Some(tp) = target_pos(t) else {
        return 0.0;
    };
    let d = p.mover.pos.distance(tp);
    let value = match t {
        Target::Hen => 1.0,
        Target::Chick(_) => {
            if hen_alive && tp.distance(hen_pos) < 1.6 {
                0.55
            } else {
                1.25
            }
        }
    };
    value / (1.0 + 0.3 * d)
}

/// Where to run while chasing: the target itself up close, otherwise ahead
/// of the hen by an amount that grows with cunning; a second fox may cut her
/// off from the coop when she is bringing a brood home.
#[allow(clippy::too_many_arguments)]
fn chase_goal(
    topo: &Topology,
    p: &Predator,
    target: Target,
    seen_at: Vec2,
    hen_tile: TilePos,
    hen_dir: Dir,
    brood: usize,
    cunning: f32,
    nav_cache: &super::NavCache,
) -> TilePos {
    let at = TilePos::of(seen_at);
    let Target::Hen = target else {
        return nearest_open(topo, at, p.mover.who).unwrap_or(at);
    };
    let dist = p.mover.pos.distance(seen_at);
    if dist < 3.0 {
        return hen_tile;
    }
    let mut k = match cunning as u32 {
        0 | 1 => 0,
        2 => 2,
        3 => 3,
        4 => 4,
        _ => 5,
    };
    if p.role >= 1 && cunning >= 3.0 {
        if brood >= 3 {
            // Lie in wait on her way home.
            let field = &nav_cache.door_field;
            let mut t = hen_tile;
            let total = field.get(t);
            if total != nav::UNREACHABLE {
                let steps = (total / 2).min(6);
                for _ in 0..steps {
                    match field.downhill(t) {
                        Some(n) if topo.passable(n, Mover::Fox) => t = n,
                        _ => break,
                    }
                }
                if topo.passable(t, Mover::Fox) {
                    return t;
                }
            }
        }
        k += 4;
    }
    let predicted = predict_hen(topo, hen_tile, hen_dir, k);
    if nav::route(topo, p.mover.tile(), predicted, p.mover.who).is_some() {
        predicted
    } else {
        hen_tile
    }
}

/// Move straight at a target in the same or the next tile.
fn lane_lunge(topo: &Topology, p: &Predator, at: Vec2) -> Option<Dir> {
    let to = at - p.mover.pos;
    let primary = Dir::from_vec(to)?;
    if topo.passable(p.mover.tile().step(primary), p.mover.who) || to.dot(primary.vec()) < 0.5 {
        return Some(primary);
    }
    // Try the other axis.
    let other = if primary.is_horizontal() {
        Dir::from_vec(Vec2::new(0.0, to.y))
    } else {
        Dir::from_vec(Vec2::new(to.x, 0.0))
    };
    other.or(Some(primary))
}

/// When fences cut a predator off from its quarry: the first step of the
/// route that ignores fences, and the fence tile to chew when it is next to
/// one.
fn fence_detour(topo: &Topology, p: &Predator, goal: TilePos) -> Option<(Option<Dir>, Option<TilePos>)> {
    let here = p.mover.tile();
    let who = p.mover.who;
    let (path, _) = pathfinding::prelude::astar(
        &here,
        |&t| {
            Dir::ALL
                .into_iter()
                .map(move |d| t.step(d))
                .filter(|&q| {
                    q == goal || topo.passable(q, who) || (topo.dyn_at(q).fence.is_some() && topo.grid.get(q).is_ground())
                })
                .map(|q| (q, if topo.dyn_at(q).fence.is_some() { 6u32 } else { 1 }))
                .collect::<Vec<_>>()
        },
        |&t| t.manhattan(goal),
        |&t| t == goal,
    )?;
    let next = *path.get(1)?;
    if topo.dyn_at(next).fence.is_some() {
        return Some((None, Some(next)));
    }
    Some((dir_between(here, next), None))
}

/// The tile nearest `t` that the predator can actually get to.
fn reachable_near(topo: &Topology, regions: &nav::Regions, here: TilePos, t: TilePos, who: Mover) -> TilePos {
    if regions.connected(here, t) {
        return t;
    }
    let mut best: Option<(u32, TilePos)> = None;
    for r in 1..5 {
        for dy in -r..=r {
            for dx in -r..=r {
                let q = TilePos::new(t.x + dx, t.y + dy);
                if topo.passable(q, who) && regions.connected(here, q) {
                    let d = q.manhattan(t);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, q));
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map_or(here, |(_, q)| q)
}

fn nearest_open(topo: &Topology, t: TilePos, who: Mover) -> Option<TilePos> {
    if topo.passable(t, who) {
        return Some(t);
    }
    let mut best = None;
    for r in 1..4 {
        for dy in -r..=r {
            for dx in -r..=r {
                let q = TilePos::new(t.x + dx, t.y + dy);
                if topo.passable(q, who) {
                    let d = q.manhattan(t);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, q));
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, q)| q)
}

fn pick_prowl_goal(topo: &Topology, p: &Predator, chicks: &[super::chick::Chick], cunning: f32, rng: &mut ChaCha8Rng) -> TilePos {
    let who = p.mover.who;
    // Cunning predators prowl where chicks are likely to be.
    if cunning >= 2.0 && rng.random_bool(0.45) {
        let wild: Vec<_> = chicks.iter().filter(|c| c.is_wild()).collect();
        if !wild.is_empty() {
            let c = wild[rng.random_range(0..wild.len())];
            if let Some(t) = nearest_open(topo, TilePos::of(c.home), who) {
                return t;
            }
        }
    }
    let here = p.mover.tile();
    let open: Vec<TilePos> = topo
        .grid
        .positions()
        .filter(|&t| topo.passable(t, who) && t.manhattan(here) >= 6)
        .filter(|&t| p.kind == PredatorKind::Weasel || topo.grid.get(t) != Tile::Den || t == p.home)
        .collect();
    if open.is_empty() {
        return p.home;
    }
    open[rng.random_range(0..open.len())]
}

/// Catches, grabs and rescues after everyone has moved.
pub fn interactions(game: &mut Game) {
    let hen_pos = game.hen.pos();
    // A fox catches the hen.
    if game.hen.alive && game.hen.invulnerable <= 0.0 {
        if let Some(p) = game
            .predators
            .iter()
            .find(|p| p.can_bite_hen() && p.mover.pos.distance(hen_pos) < CATCH_DIST)
        {
            let by = p.id;
            game.hen_caught(by);
            return;
        }
    }
    // The hen makes a predator drop the chick it is carrying.
    if game.hen.alive {
        for pi in 0..game.predators.len() {
            let p = &game.predators[pi];
            let Some(cid) = p.carrying else { continue };
            if p.mover.pos.distance(hen_pos) < RESCUE_DIST {
                let ppos = p.mover.pos;
                let pid = p.id;
                game.predators[pi].carrying = None;
                game.predators[pi].state = PredState::Stunned { t: FOX_STUN_TIME };
                if let Some(c) = game.chicks.iter_mut().find(|c| c.id == cid) {
                    let at = ppos + (hen_pos - ppos).normalize_or_zero() * 0.3;
                    let (at, _) = game.topo.resolve_circle(at, CHICK_RADIUS, Mover::Chick);
                    c.drop_rescued(at, ppos);
                }
                game.add_score(POINTS_RESCUE);
                game.emit(SimEvent::ChickRescued {
                    chick: cid,
                    from: pid,
                    pos: ppos,
                    points: POINTS_RESCUE,
                });
                game.emit(SimEvent::Emote {
                    entity: pid,
                    emote: Emote::Angry,
                });
            }
        }
    }
    // Predators grab chicks.
    for pi in 0..game.predators.len() {
        if !game.predators[pi].can_grab() {
            continue;
        }
        let ppos = game.predators[pi].mover.pos;
        let topo = &game.topo;
        let grab = game
            .chicks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_on_ground() && !matches!(c.state, ChickState::Homing))
            .filter(|(_, c)| {
                let reach = if c.is_hidden(topo) { GRAB_DIST * 0.5 } else { GRAB_DIST };
                c.pos.distance(ppos) < reach
            })
            .min_by(|a, b| a.1.pos.distance(ppos).total_cmp(&b.1.pos.distance(ppos)))
            .map(|(i, _)| i);
        if let Some(ci) = grab {
            let pid = game.predators[pi].id;
            let cid = game.chicks[ci].id;
            let cpos = game.chicks[ci].pos;
            game.chicks[ci].state = ChickState::Carried { by: pid };
            game.chicks[ci].vel = Vec2::ZERO;
            game.predators[pi].carrying = Some(cid);
            game.predators[pi].state = PredState::Carry;
            game.emit(SimEvent::ChickGrabbed {
                chick: cid,
                by: pid,
                pos: cpos,
            });
            game.emit(SimEvent::Emote {
                entity: cid,
                emote: Emote::Fright,
            });
        }
    }
}
