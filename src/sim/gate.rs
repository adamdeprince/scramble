//! Garden gates. Timed gates swing shut and open on their own, with a
//! warning before they close; latched gates are opened and closed by the
//! hen. A gate never closes on somebody standing in it.

use super::rules::*;
use super::{Game, SimEvent};
use crate::world::{Tile, TilePos};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GateKind {
    Timed,
    Latch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gate {
    pub id: u32,
    pub tile: TilePos,
    pub kind: GateKind,
    pub closed: bool,
    pub timer: f32,
    pub period: f32,
    pub warning: bool,
    /// 0 fully open to 1 fully shut, animated.
    pub swing: f32,
}

impl Gate {
    pub fn new(id: u32, tile: TilePos, t: Tile, index: usize, period: f32) -> Self {
        let kind = if t == Tile::GateLatch { GateKind::Latch } else { GateKind::Timed };
        let closed = kind == GateKind::Timed && index % 2 == 1;
        Gate {
            id,
            tile,
            kind,
            closed,
            timer: period * (0.6 + 0.4 * (index % 3) as f32 / 2.0),
            period,
            warning: false,
            swing: if closed { 1.0 } else { 0.0 },
        }
    }
}

fn occupied(game: &Game, tile: TilePos) -> bool {
    let c = tile.center();
    game.occupants().iter().any(|o| o.distance(c) < 0.75)
}

pub fn update(game: &mut Game) {
    for i in 0..game.gates.len() {
        let g = &mut game.gates[i];
        let goal = if g.closed { 1.0 } else { 0.0 };
        g.swing += (goal - g.swing).clamp(-DT * 4.0, DT * 4.0);
        if g.kind != GateKind::Timed {
            continue;
        }
        g.timer -= DT;
        if !g.closed && !g.warning && g.timer < GATE_WARNING {
            g.warning = true;
            let id = g.id;
            game.emit(SimEvent::GateWarning { gate: id });
        }
        let g = &game.gates[i];
        if g.timer <= 0.0 {
            let closing = !g.closed;
            let tile = g.tile;
            if closing && occupied(game, tile) {
                game.gates[i].timer = 0.15;
                continue;
            }
            let g = &mut game.gates[i];
            g.closed = closing;
            g.timer = g.period;
            g.warning = false;
            let id = g.id;
            game.topo.set_gate(tile, closing);
            game.emit(SimEvent::GateMoved { gate: id, closed: closing });
        }
    }
}

/// The latched gate the hen can reach, if any (the one Space works).
pub fn latch_in_reach(game: &Game) -> Option<usize> {
    let hen = game.hen.pos();
    game.gates
        .iter()
        .position(|g| g.kind == GateKind::Latch && g.tile.center().distance(hen) < LATCH_REACH)
}

/// Space next to a latched gate swings it. Returns whether a gate was in
/// reach (so the key does not also drop a fence).
pub fn try_toggle_latch(game: &mut Game) -> bool {
    let Some(i) = latch_in_reach(game) else {
        return false;
    };
    let tile = game.gates[i].tile;
    let closing = !game.gates[i].closed;
    if closing && occupied(game, tile) {
        return true;
    }
    game.gates[i].closed = closing;
    game.topo.set_gate(tile, closing);
    let id = game.gates[i].id;
    game.emit(SimEvent::GateMoved { gate: id, closed: closing });
    true
}
