//! A simple bot that plays the hen: it gathers the nearest wild chick,
//! takes the brood home when it is big enough, and runs from foxes it can
//! see. It drives headless tests, benchmarks and the title screen's demo.

use super::{Game, Phase, TickInput};
use crate::world::nav::{self, DistField};
use crate::world::{Dir, Mover, TilePos};
use super::predator::PredatorKind;

#[derive(Clone, Debug, Default)]
pub struct Autopilot {
    /// Bring the brood home once it has this many chicks.
    pub deliver_at: usize,
    pub use_fences: bool,
    last_fence_tick: u64,
}

impl Autopilot {
    pub fn new() -> Self {
        Autopilot {
            deliver_at: 3,
            use_fences: true,
            last_fence_tick: 0,
        }
    }

    pub fn decide(&mut self, game: &Game) -> TickInput {
        if game.phase != Phase::Playing {
            return TickInput::default();
        }
        let topo = &game.topo;
        let hen = game.hen.mover.tile();
        let hen_pos = game.hen.pos();

        // Danger: run to where the nearest fox is furthest away.
        let foxes: Vec<TilePos> = game
            .predators
            .iter()
            .filter(|p| p.kind == PredatorKind::Fox && p.can_bite_hen())
            .map(|p| p.mover.tile())
            .collect();
        let close_fox = game
            .predators
            .iter()
            .filter(|p| p.can_bite_hen())
            .any(|p| p.mover.pos.distance(hen_pos) < 3.2);
        if close_fox && !foxes.is_empty() {
            let field = DistField::compute(topo, &foxes, Mover::Fox);
            let mut best: Option<(u32, Dir)> = None;
            for d in Dir::ALL {
                let n = hen.step(d);
                if topo.passable(n, Mover::Hen) {
                    let v = field.get(n).min(1000);
                    if best.is_none_or(|(bv, _)| v > bv) {
                        best = Some((v, d));
                    }
                }
            }
            let mut input = TickInput {
                dir: best.map(|b| b.1),
                ..Default::default()
            };
            if self.use_fences && game.fence_supply >= 1.0 && game.tick > self.last_fence_tick + 90 {
                input.action = true;
                self.last_fence_tick = game.tick;
            }
            return input;
        }

        let brood = game.brood_size();
        let wild: Vec<TilePos> = game.chicks.iter().filter(|c| c.is_wild()).map(|c| TilePos::of(c.pos)).collect();
        let go_home = brood > 0 && (brood >= self.deliver_at || wild.is_empty());
        let goal = if go_home {
            Some(game.nearest_door(hen_pos))
        } else {
            wild.iter()
                .copied()
                .min_by_key(|&t| game.nav.hen_field.get(t))
        };
        let dir = goal.and_then(|g| {
            if g == hen {
                return None;
            }
            let route = nav::route(topo, hen, g, Mover::Hen)?;
            let next = *route.first()?;
            Dir::ALL.into_iter().find(|&d| hen.step(d) == next)
        });
        TickInput {
            dir,
            ..Default::default()
        }
    }
}
