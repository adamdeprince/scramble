//! The game simulation: a fixed-timestep, deterministic model of the yard.
//!
//! Everything that decides the outcome of a game lives here and runs the
//! same whichever terminal displays it. Rendering reads the state (and the
//! previous tick's positions, for interpolation) and the events, and never
//! changes them. Given the same level, seed and recorded inputs, two runs
//! produce identical states, which is what replays and tests rely on.

pub mod autopilot;
pub mod chick;
pub mod fence;
pub mod gate;
pub mod hawk;
pub mod items;
pub mod mover;
pub mod predator;
pub mod replay;
pub mod rules;
pub mod steer;

use crate::world::level::Level;
use crate::world::nav::{DistField, Regions};
use crate::world::{Dir, Mover, TilePos, Topology};
use chick::{Chick, ChickState};
use fence::Fence;
use gate::Gate;
use glam::Vec2;
use hawk::Hawk;
use items::Item;
use mover::LaneMover;
use predator::{Predator, PredatorKind};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rules::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A completed fence stroke drawn with a pointer, in world (tile) units.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FenceStroke {
    /// Correlates the stroke with what the renderer drew while it was being
    /// made (an ink element, a trail of dots).
    pub id: u64,
    pub points: Vec<[f32; 2]>,
}

/// Player input for one tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TickInput {
    /// The direction the player is asking for (held key, or last pressed).
    pub dir: Option<Dir>,
    /// Space: open/close a latch gate when next to one, otherwise drop a
    /// fence behind the hen.
    pub action: bool,
    pub strokes: Vec<FenceStroke>,
}

impl TickInput {
    pub fn is_idle(&self) -> bool {
        self.dir.is_none() && !self.action && self.strokes.is_empty()
    }
}

/// Little symbols that pop over a creature's head.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emote {
    Alert,
    Puzzled,
    Heart,
    Fright,
    Note,
    Sweat,
    Angry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FenceReject {
    Occupied,
    NoSupply,
    WouldTrapBrood,
    WouldPenPredator,
    NotGround,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SimEvent {
    Emote { entity: u32, emote: Emote },
    ChickJoined { chick: u32, pos: Vec2 },
    ChickHomed { chick: u32, pos: Vec2 },
    BatchScored { count: u32, points: u32, pos: Vec2 },
    ChickGrabbed { chick: u32, by: u32, pos: Vec2 },
    ChickRescued { chick: u32, from: u32, pos: Vec2, points: u32 },
    ChickLost { chick: u32, pos: Vec2 },
    ChickStrayed { chick: u32, pos: Vec2 },
    ChickScared { chick: u32, pos: Vec2 },
    ChickFoundBug { chick: u32, pos: Vec2 },
    ItemSpawned { item: u32, pos: Vec2 },
    ItemCollected { item: u32, points: u32, pos: Vec2, by_chick: bool },
    ItemExpired { item: u32 },
    HenCaught { by: u32, pos: Vec2 },
    HenStunned { pos: Vec2 },
    HenRespawned { pos: Vec2 },
    FenceBuilt { fence: u32, stroke: Option<u64>, tiles: Vec<TilePos>, rejected: Vec<(TilePos, FenceReject)> },
    FenceRefused { stroke: Option<u64>, rejected: Vec<(TilePos, FenceReject)> },
    FenceDamaged { fence: u32, tile: TilePos },
    FenceExpired { fence: u32 },
    GateMoved { gate: u32, closed: bool },
    GateWarning { gate: u32 },
    HawkCircling { pos: Vec2 },
    HawkStrike { pos: Vec2, took: Option<u32> },
    ExtraLife,
    LevelCleared { delivered: u32, lost: u32, time_bonus: u32, perfect_bonus: u32 },
    LevelFailed { delivered: u32, needed: u32 },
    GameOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    /// Countdown before play starts.
    Ready { t: f32 },
    Playing,
    /// The hen was caught: a short pause, then respawn or game over.
    Caught { t: f32 },
    /// Every chick is home or gone and enough made it.
    Cleared { t: f32 },
    /// Every chick is home or gone and too few made it.
    Failed { t: f32 },
    GameOver,
}

/// What the app should do once a level's closing animation is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    NextLevel,
    Retry,
    GameOver,
}

/// Score and lives carried from level to level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub score: u32,
    pub lives: u32,
    pub next_extra_life: u32,
    pub levels_cleared: u32,
    pub chicks_saved: u32,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            score: 0,
            lives: START_LIVES,
            next_extra_life: EXTRA_LIFE_EVERY,
            levels_cleared: 0,
            chicks_saved: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hen {
    pub id: u32,
    pub mover: LaneMover,
    pub stun: f32,
    pub invulnerable: f32,
    pub alive: bool,
    /// Seconds since the hen last did something worth animating.
    pub peck: f32,
    pub celebrate: f32,
}

impl Hen {
    pub fn pos(&self) -> Vec2 {
        self.mover.pos
    }

    pub fn velocity(&self) -> Vec2 {
        (self.mover.pos - self.mover.prev_pos) / DT
    }
}

/// An open delivery: chicks arriving within a short window score together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub count: u32,
    pub idle: f32,
    pub pos: Vec2,
}

/// Measured cost of simulation work, for diagnostics. Not part of the
/// deterministic state.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickStats {
    pub last_tick: Duration,
    pub last_ai: Duration,
    pub last_flock: Duration,
    pub last_nav: Duration,
    pub max_tick: Duration,
}

/// Cached flow fields, refreshed when what they depend on changes.
#[derive(Clone, Debug)]
pub struct NavCache {
    pub hen_field: DistField,
    hen_key: (TilePos, u64),
    pub door_field: DistField,
    door_key: u64,
    pub cover_field: DistField,
    cover_key: u64,
    pub fox_regions: Regions,
    pub weasel_regions: Regions,
    pub recomputes: u64,
}

impl NavCache {
    fn new(topo: &Topology, hen: TilePos, doors: &[TilePos]) -> Self {
        let cover: Vec<TilePos> = topo.grid.positions().filter(|&p| topo.grid.sheltered(p)).collect();
        NavCache {
            hen_field: DistField::compute(topo, &[hen], Mover::Chick),
            hen_key: (hen, topo.version),
            door_field: DistField::compute(topo, doors, Mover::Chick),
            door_key: topo.version,
            cover_field: DistField::compute(topo, &cover, Mover::Chick),
            cover_key: topo.version,
            fox_regions: Regions::compute(topo, Mover::Fox),
            weasel_regions: Regions::compute(topo, Mover::Weasel),
            recomputes: 5,
        }
    }

    fn refresh(&mut self, topo: &Topology, hen: TilePos, doors: &[TilePos]) {
        if self.hen_key != (hen, topo.version) {
            self.hen_field = DistField::compute(topo, &[hen], Mover::Chick);
            self.hen_key = (hen, topo.version);
            self.recomputes += 1;
        }
        if self.door_key != topo.version {
            self.door_field = DistField::compute(topo, doors, Mover::Chick);
            self.door_key = topo.version;
            self.recomputes += 1;
        }
        if self.cover_key != topo.version {
            let cover: Vec<TilePos> = topo
                .grid
                .positions()
                .filter(|&p| topo.grid.sheltered(p) && topo.passable(p, Mover::Chick))
                .collect();
            self.cover_field = DistField::compute(topo, &cover, Mover::Chick);
            self.cover_key = topo.version;
            self.recomputes += 1;
        }
        if self.fox_regions.version != topo.version {
            self.fox_regions = Regions::compute(topo, Mover::Fox);
            self.weasel_regions = Regions::compute(topo, Mover::Weasel);
            self.recomputes += 2;
        }
    }
}

pub struct Game {
    pub level: Level,
    pub level_index: usize,
    pub topo: Topology,
    pub rng: ChaCha8Rng,
    pub seed: u64,
    pub tick: u64,
    /// Seconds spent in play on this level.
    pub time: f32,
    pub phase: Phase,
    pub hen: Hen,
    pub chicks: Vec<Chick>,
    pub predators: Vec<Predator>,
    pub hawk: Option<Hawk>,
    pub fences: Vec<Fence>,
    pub gates: Vec<Gate>,
    pub items: Vec<Item>,
    pub session: Session,
    pub fence_supply: f32,
    pub batch: Option<Batch>,
    pub events: Vec<SimEvent>,
    pub stats: TickStats,
    pub nav: NavCache,
    pub delivered: u32,
    pub lost: u32,
    pub(crate) next_id: u32,
    pub(crate) item_timer: f32,
    pub(crate) panic: Vec<(Vec2, Vec2)>,
}

impl Game {
    pub fn new(level: Level, level_index: usize, session: Session, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (level_index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut topo = Topology::new(level.grid.clone());
        let mut next_id = 1;
        let mut alloc = || {
            let id = next_id;
            next_id += 1;
            id
        };
        let hen = Hen {
            id: alloc(),
            mover: LaneMover::new(level.hen_start, Mover::Hen),
            stun: 0.0,
            invulnerable: 0.0,
            alive: true,
            peck: 10.0,
            celebrate: 0.0,
        };
        let mut chicks = Vec::new();
        for i in 0..level.chick_count {
            let spawn = level.chick_spawns[i as usize % level.chick_spawns.len()];
            chicks.push(Chick::new(alloc(), spawn, &mut rng));
        }
        let mut predators = Vec::new();
        for i in 0..level.fox_count {
            let den = level.fox_dens[i as usize % level.fox_dens.len()];
            predators.push(Predator::new(alloc(), PredatorKind::Fox, den, i));
        }
        for i in 0..level.weasel_count {
            let b = level.weasel_burrows[i as usize % level.weasel_burrows.len()];
            predators.push(Predator::new(alloc(), PredatorKind::Weasel, b, i));
        }
        let mut gates = Vec::new();
        for p in level.grid.positions() {
            let t = level.grid.get(p);
            if t.is_gate() {
                let g = Gate::new(alloc(), p, t, gates.len(), level.gate_period);
                topo.set_gate(p, g.closed);
                gates.push(g);
            }
        }
        let hawk = level.hawk.then(|| Hawk::new(alloc()));
        let nav = NavCache::new(&topo, level.hen_start, &level.doors);
        let fence_supply = level.fence_supply as f32;
        Game {
            level,
            level_index,
            topo,
            rng,
            seed,
            tick: 0,
            time: 0.0,
            phase: Phase::Ready { t: READY_TIME },
            hen,
            chicks,
            predators,
            hawk,
            fences: Vec::new(),
            gates,
            items: Vec::new(),
            session,
            fence_supply,
            batch: None,
            events: Vec::new(),
            stats: TickStats::default(),
            nav,
            delivered: 0,
            lost: 0,
            next_id,
            item_timer: ITEM_FIRST_DELAY,
            panic: Vec::new(),
        }
    }

    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn emit(&mut self, e: SimEvent) {
        self.events.push(e);
    }

    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn brood_size(&self) -> usize {
        self.chicks.iter().filter(|c| c.in_brood()).count()
    }

    pub fn remaining(&self) -> usize {
        self.chicks.iter().filter(|c| c.is_active()).count()
    }

    pub fn needed(&self) -> u32 {
        (self.level.quota * self.level.chick_count as f32).ceil() as u32
    }

    pub fn hen_speed(&self) -> f32 {
        let factor = (1.0 - HEN_BROOD_SLOWDOWN * self.brood_size() as f32).max(HEN_MIN_SPEED_FACTOR);
        HEN_SPEED * factor
    }

    pub fn door_center(&self) -> Vec2 {
        self.nearest_door(self.hen.pos()).center()
    }

    pub fn nearest_door(&self, p: Vec2) -> TilePos {
        *self
            .level
            .doors
            .iter()
            .min_by(|a, b| a.center().distance_squared(p).total_cmp(&b.center().distance_squared(p)))
            .expect("levels always have a door")
    }

    pub fn outcome(&self) -> Option<Outcome> {
        match self.phase {
            Phase::Cleared { t } if t <= 0.0 => Some(Outcome::NextLevel),
            Phase::Failed { t } if t <= 0.0 => Some(if self.session.lives == 0 {
                Outcome::GameOver
            } else {
                Outcome::Retry
            }),
            Phase::GameOver => Some(Outcome::GameOver),
            _ => None,
        }
    }

    pub fn add_score(&mut self, points: u32) {
        self.session.score = self.session.score.saturating_add(points);
        while self.session.score >= self.session.next_extra_life {
            self.session.lives += 1;
            self.session.next_extra_life = self.session.next_extra_life.saturating_add(EXTRA_LIFE_EVERY);
            self.emit(SimEvent::ExtraLife);
        }
    }

    /// Advances the simulation by one fixed tick.
    pub fn step(&mut self, input: &TickInput) {
        let started = Instant::now();
        self.tick += 1;
        self.hen.mover.prev_pos = self.hen.mover.pos;
        for c in &mut self.chicks {
            c.prev_pos = c.pos;
        }
        for p in &mut self.predators {
            p.mover.prev_pos = p.mover.pos;
        }
        if let Some(h) = &mut self.hawk {
            h.prev_pos = h.pos;
        }

        match self.phase {
            Phase::Ready { t } => {
                let t = t - DT;
                self.phase = if t <= 0.0 { Phase::Playing } else { Phase::Ready { t } };
                self.update_flock(false);
            }
            Phase::Playing => self.update_playing(input),
            Phase::Caught { t } => {
                let t = t - DT;
                self.update_flock(true);
                if t <= 0.0 {
                    self.after_caught();
                } else {
                    self.phase = Phase::Caught { t };
                }
            }
            Phase::Cleared { t } => {
                self.hen.celebrate += DT;
                self.phase = Phase::Cleared { t: t - DT };
            }
            Phase::Failed { t } => self.phase = Phase::Failed { t: t - DT },
            Phase::GameOver => {}
        }
        let elapsed = started.elapsed();
        self.stats.last_tick = elapsed;
        self.stats.max_tick = self.stats.max_tick.max(elapsed);
    }

    fn update_playing(&mut self, input: &TickInput) {
        self.time += DT;

        for stroke in &input.strokes {
            let pts: Vec<Vec2> = stroke.points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
            fence::build_from_stroke(self, &pts, Some(stroke.id));
        }
        if input.action && !gate::try_toggle_latch(self) {
            fence::drop_behind_hen(self);
        }

        // The hen.
        self.hen.invulnerable = (self.hen.invulnerable - DT).max(0.0);
        self.hen.peck += DT;
        if self.hen.stun > 0.0 {
            self.hen.stun -= DT;
        } else {
            let speed = self.hen_speed();
            let moved = self.hen.mover.advance(&self.topo, input.dir, speed * DT, TURN_WINDOW);
            if moved.blocked && input.dir.is_some() {
                self.hen.peck = 0.0;
            }
        }

        gate::update(self);
        fence::update(self);

        let t_nav = Instant::now();
        let hen_tile = self.hen.mover.tile();
        let doors = self.level.doors.clone();
        self.nav.refresh(&self.topo, hen_tile, &doors);
        self.stats.last_nav = t_nav.elapsed();

        self.update_flock(true);

        let t_ai = Instant::now();
        predator::update_all(self);
        hawk::update(self);
        self.stats.last_ai = t_ai.elapsed();

        self.interactions();
        items::update(self);

        if self.fence_supply < self.level.fence_supply as f32 {
            self.fence_supply = (self.fence_supply + DT / FENCE_REGEN_TIME).min(self.level.fence_supply as f32);
        }

        self.update_batch();
        self.check_level_end();
    }

    fn update_flock(&mut self, live: bool) {
        let t = Instant::now();
        chick::update_all(self, live);
        self.stats.last_flock = t.elapsed();
    }

    fn update_batch(&mut self) {
        if let Some(b) = &mut self.batch {
            b.idle += DT;
            if b.idle >= BATCH_WINDOW {
                let b = self.batch.take().expect("checked above");
                let mult = 1.0 + BATCH_BONUS * (b.count.saturating_sub(1)) as f32;
                let points = (POINTS_DELIVER as f32 * b.count as f32 * mult).round() as u32;
                self.add_score(points);
                self.emit(SimEvent::BatchScored {
                    count: b.count,
                    points,
                    pos: b.pos,
                });
            }
        }
    }

    /// Called by the flock when a chick reaches the coop.
    pub(crate) fn chick_home(&mut self, index: usize) {
        let c = &mut self.chicks[index];
        c.state = ChickState::Delivered;
        c.vel = Vec2::ZERO;
        let pos = c.pos;
        let id = c.id;
        self.delivered += 1;
        self.session.chicks_saved += 1;
        let door = self.door_center();
        match &mut self.batch {
            Some(b) => {
                b.count += 1;
                b.idle = 0.0;
            }
            None => {
                self.batch = Some(Batch {
                    count: 1,
                    idle: 0.0,
                    pos: door,
                })
            }
        }
        self.emit(SimEvent::ChickHomed { chick: id, pos });
    }

    fn interactions(&mut self) {
        let hen_pos = self.hen.pos();
        // The hen gathers wild chicks by walking up to them.
        if self.hen.alive && self.hen.stun <= 0.0 {
            for i in 0..self.chicks.len() {
                let c = &self.chicks[i];
                if c.is_wild() && c.pos.distance(hen_pos) < JOIN_DIST {
                    let id = c.id;
                    let pos = c.pos;
                    self.chicks[i].join_brood();
                    self.add_score(POINTS_JOIN);
                    self.emit(SimEvent::ChickJoined { chick: id, pos });
                    self.emit(SimEvent::Emote {
                        entity: id,
                        emote: Emote::Heart,
                    });
                }
            }
        }
        predator::interactions(self);
    }

    fn check_level_end(&mut self) {
        if self.remaining() > 0 || self.batch.is_some() {
            return;
        }
        let needed = self.needed();
        if self.delivered >= needed {
            let time_bonus = TIME_BONUS_MAX.saturating_sub(TIME_BONUS_PER_SEC * self.time as u32);
            let perfect_bonus = if self.lost == 0 { POINTS_PERFECT } else { 0 };
            self.add_score(time_bonus + perfect_bonus);
            self.session.levels_cleared += 1;
            self.emit(SimEvent::LevelCleared {
                delivered: self.delivered,
                lost: self.lost,
                time_bonus,
                perfect_bonus,
            });
            self.phase = Phase::Cleared { t: CLEARED_TIME };
            self.hen.celebrate = 0.0;
        } else {
            self.session.lives = self.session.lives.saturating_sub(1);
            self.emit(SimEvent::LevelFailed {
                delivered: self.delivered,
                needed,
            });
            self.phase = Phase::Failed { t: CLEARED_TIME };
        }
    }

    /// The hen has been caught by a predator.
    pub(crate) fn hen_caught(&mut self, by: u32) {
        let pos = self.hen.pos();
        self.hen.alive = false;
        self.hen.mover.dir = None;
        self.emit(SimEvent::HenCaught { by, pos });
        // The brood scatters and has to be gathered again.
        for i in 0..self.chicks.len() {
            if self.chicks[i].in_brood() {
                self.chicks[i].scatter(pos);
            }
        }
        self.batch = None;
        self.phase = Phase::Caught { t: CAUGHT_TIME };
    }

    fn after_caught(&mut self) {
        self.session.lives = self.session.lives.saturating_sub(1);
        if self.session.lives == 0 {
            self.phase = Phase::GameOver;
            self.emit(SimEvent::GameOver);
            return;
        }
        self.hen.mover.place(self.level.hen_start);
        self.hen.alive = true;
        self.hen.stun = 0.0;
        self.hen.invulnerable = RESPAWN_INVULNERABLE;
        for p in &mut self.predators {
            p.send_home(&self.topo);
        }
        let pos = self.hen.pos();
        self.emit(SimEvent::HenRespawned { pos });
        self.phase = Phase::Playing;
    }

    /// Positions of creatures that block fence placement.
    pub fn occupants(&self) -> Vec<Vec2> {
        let mut v = vec![self.hen.pos()];
        v.extend(self.chicks.iter().filter(|c| c.is_on_ground()).map(|c| c.pos));
        v.extend(self.predators.iter().map(|p| p.mover.pos));
        v
    }

    /// A hash of the deterministic state, for replay verification.
    pub fn state_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.tick.hash(&mut h);
        self.session.score.hash(&mut h);
        self.session.lives.hash(&mut h);
        let mut f = |v: Vec2| {
            v.x.to_bits().hash(&mut h);
            v.y.to_bits().hash(&mut h);
        };
        f(self.hen.pos());
        for c in &self.chicks {
            f(c.pos);
            f(c.vel);
        }
        for p in &self.predators {
            f(p.mover.pos);
        }
        if let Some(hk) = &self.hawk {
            f(hk.pos);
        }
        self.fences.len().hash(&mut h);
        self.topo.version.hash(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests;
