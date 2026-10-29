//! Temporary fences, drawn with a pointer or dropped behind the hen.
//!
//! A stroke is turned into the chain of tiles it crosses, and each tile is
//! accepted in order only if the yard stays fair:
//!
//! * it is open ground (path, lawn or tall grass) with nobody standing on it,
//! * the hen can still reach the coop and every chick she has yet to bring
//!   home,
//! * every predator keeps at least [`FENCE_MIN_PREDATOR_REGION`] tiles of
//!   yard (or all it had, if it had less): fences redirect predators, they
//!   never pen them.
//!
//! Fences cost supply, fall apart after [`FENCE_LIFETIME`] seconds, and a
//! fox that has been cut off from its quarry chews through them.

use super::rules::*;
use super::{FenceReject, Game, SimEvent};
use crate::world::nav::{self, DistField};
use crate::world::{Mover, TilePos};
use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fence {
    pub id: u32,
    pub tiles: Vec<TilePos>,
    pub age: f32,
    pub lifetime: f32,
}

impl Fence {
    /// 0 when new, 1 when about to fall apart.
    pub fn wear(&self) -> f32 {
        (self.age / self.lifetime).clamp(0.0, 1.0)
    }
}

/// The tiles a polyline passes through, in drawing order, without repeats.
pub fn stroke_tiles(points: &[Vec2]) -> Vec<TilePos> {
    let mut out: Vec<TilePos> = Vec::new();
    let push = |t: TilePos, out: &mut Vec<TilePos>| {
        if !out.contains(&t) {
            out.push(t);
        }
    };
    match points {
        [] => {}
        [p] => push(TilePos::of(*p), &mut out),
        _ => {
            for w in points.windows(2) {
                for t in nav::tiles_on_segment(w[0], w[1]) {
                    push(t, &mut out);
                }
            }
        }
    }
    out
}

/// Decides which candidate tiles may be fenced. Returns the accepted tiles
/// and the reasons the others were refused. Tiles that are not ground at all
/// (hedges, the pond) are skipped silently: strokes cross scenery all the
/// time.
pub fn plan(game: &Game, candidates: &[TilePos]) -> (Vec<TilePos>, Vec<(TilePos, FenceReject)>) {
    let topo = &game.topo;
    let occupants = game.occupants();
    let supply = game.fence_supply.floor().max(0.0) as usize;
    let hen_tile = game.hen.mover.tile();

    // What the hen must still be able to reach: the coop, and every chick
    // still out in the yard, as far as she can reach them now.
    let baseline = DistField::compute(topo, &[hen_tile], Mover::Hen);
    let mut required: Vec<TilePos> = game.level.doors.iter().copied().filter(|&d| baseline.reachable(d)).collect();
    for c in &game.chicks {
        if c.is_on_ground() {
            let t = TilePos::of(c.pos);
            if baseline.reachable(t) && !required.contains(&t) {
                required.push(t);
            }
        }
    }
    let predators: Vec<(TilePos, Mover, usize)> = game
        .predators
        .iter()
        .map(|p| {
            let t = p.mover.tile();
            (t, p.mover.who, nav::region_size(topo, t, p.mover.who, &[]))
        })
        .collect();

    let mut accepted: Vec<TilePos> = Vec::new();
    let mut rejected = Vec::new();
    for &t in candidates {
        if accepted.contains(&t) {
            continue;
        }
        if !topo.fenceable(t) {
            if topo.grid.get(t).is_ground() && topo.dyn_at(t).fence.is_none() {
                rejected.push((t, FenceReject::NotGround));
            }
            continue;
        }
        let c = t.center();
        if occupants.iter().any(|o| o.distance(c) < FENCE_AGENT_CLEARANCE + 0.15) {
            rejected.push((t, FenceReject::Occupied));
            continue;
        }
        if accepted.len() >= supply {
            rejected.push((t, FenceReject::NoSupply));
            continue;
        }
        accepted.push(t);
        let field = DistField::compute_with(topo, &[hen_tile], |p| topo.passable(p, Mover::Hen) && !accepted.contains(&p));
        if required.iter().any(|&r| !field.reachable(r)) {
            accepted.pop();
            rejected.push((t, FenceReject::WouldTrapBrood));
            continue;
        }
        let pens = predators.iter().any(|&(pt, who, base)| {
            let need = base.min(FENCE_MIN_PREDATOR_REGION);
            nav::region_size(topo, pt, who, &accepted) < need
        });
        if pens {
            accepted.pop();
            rejected.push((t, FenceReject::WouldPenPredator));
        }
    }
    (accepted, rejected)
}

fn build(game: &mut Game, candidates: &[TilePos], stroke: Option<u64>) {
    let (tiles, rejected) = plan(game, candidates);
    if tiles.is_empty() {
        game.emit(SimEvent::FenceRefused { stroke, rejected });
        return;
    }
    let id = game.alloc_id();
    for &t in &tiles {
        game.topo.set_fence(t, Some(id));
    }
    game.fence_supply -= tiles.len() as f32;
    game.fences.push(Fence {
        id,
        tiles: tiles.clone(),
        age: 0.0,
        lifetime: FENCE_LIFETIME,
    });
    game.emit(SimEvent::FenceBuilt {
        fence: id,
        stroke,
        tiles,
        rejected,
    });
}

pub fn build_from_stroke(game: &mut Game, points: &[Vec2], stroke: Option<u64>) {
    let tiles = stroke_tiles(points);
    build(game, &tiles, stroke);
}

/// How far back along her trail the hen looks for room for a hurdle.
const DROP_REACH: usize = 4;

/// Keyboard fencing: one panel across the path behind the hen, on the first
/// free tile behind her, which with a brood in tow is behind the brood: it
/// shuts the way behind them.
pub fn drop_behind_hen(game: &mut Game) {
    let m = &game.hen.mover;
    let back = m.dir.unwrap_or(m.facing).opposite();
    let here = m.tile();
    let mut first_refusal = None;
    let mut t = here;
    for _ in 0..DROP_REACH {
        t = t.step(back);
        if !game.topo.passable(t, Mover::Hen) {
            break;
        }
        let (ok, rejected) = plan(game, &[t]);
        if !ok.is_empty() {
            build(game, &[t], None);
            return;
        }
        if first_refusal.is_none() {
            first_refusal = Some(rejected);
        }
    }
    game.emit(SimEvent::FenceRefused {
        stroke: None,
        rejected: first_refusal.unwrap_or_default(),
    });
}

pub fn update(game: &mut Game) {
    let mut expired = Vec::new();
    for f in &mut game.fences {
        f.age += DT;
        if f.age >= f.lifetime {
            expired.push(f.id);
        }
    }
    for id in expired {
        if let Some(i) = game.fences.iter().position(|f| f.id == id) {
            let f = game.fences.remove(i);
            for t in f.tiles {
                game.topo.set_fence(t, None);
            }
            game.emit(SimEvent::FenceExpired { fence: id });
        }
    }
}

/// A predator has chewed through one panel.
pub fn damage(game: &mut Game, tile: TilePos, _by: Option<u32>) {
    let Some(fid) = game.topo.dyn_at(tile).fence else {
        return;
    };
    game.topo.set_fence(tile, None);
    if let Some(i) = game.fences.iter().position(|f| f.id == fid) {
        game.fences[i].tiles.retain(|&t| t != tile);
        game.emit(SimEvent::FenceDamaged { fence: fid, tile });
        if game.fences[i].tiles.is_empty() {
            game.fences.remove(i);
            game.emit(SimEvent::FenceExpired { fence: fid });
        }
    }
}
