//! The hawk: it circles out of sight, picks a chick out in the open, and
//! announces itself with a shadow that slides over its prey and grows for
//! two seconds before it stoops. Chicks under tall grass or a tree are safe,
//! and so is a hen standing in shade; one caught in the open is stunned.

use super::chick::ChickState;
use super::rules::*;
use super::{Game, SimEvent};
use crate::world::TilePos;
use glam::Vec2;
use rand::RngExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum HawkState {
    Away { t: f32 },
    Circling { t: f32, target: Option<u32> },
    Diving { t: f32 },
    Leaving { t: f32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hawk {
    pub id: u32,
    pub state: HawkState,
    /// Where its shadow falls.
    pub pos: Vec2,
    pub prev_pos: Vec2,
}

impl Hawk {
    pub fn new(id: u32) -> Self {
        Hawk {
            id,
            state: HawkState::Away { t: HAWK_FIRST_DELAY },
            pos: Vec2::new(-4.0, -4.0),
            prev_pos: Vec2::new(-4.0, -4.0),
        }
    }

    /// The shadow on the ground, while there is one to see.
    pub fn shadow(&self) -> Option<Vec2> {
        match self.state {
            HawkState::Circling { .. } | HawkState::Diving { .. } => Some(self.pos),
            _ => None,
        }
    }

    /// 0 when the shadow first appears, 1 at the moment of the strike.
    pub fn warning(&self) -> f32 {
        match self.state {
            HawkState::Circling { t, .. } => (1.0 - t / HAWK_WARNING).clamp(0.0, 1.0),
            HawkState::Diving { .. } => 1.0,
            _ => 0.0,
        }
    }
}

pub fn update(game: &mut Game) {
    let Some(mut hawk) = game.hawk.take() else {
        return;
    };
    match hawk.state {
        HawkState::Away { t } => {
            let t = t - DT;
            if t > 0.0 {
                hawk.state = HawkState::Away { t };
            } else {
                // Prey: chicks out in the open, stragglers first.
                let hen = game.hen.pos();
                let prey = game
                    .chicks
                    .iter()
                    .filter(|c| c.is_on_ground() && !game.topo.grid.sheltered(TilePos::of(c.pos)))
                    .max_by(|a, b| a.pos.distance(hen).total_cmp(&b.pos.distance(hen)).then(b.id.cmp(&a.id)));
                match prey {
                    Some(c) => {
                        let start = edge_point(game, c.pos);
                        hawk.pos = start;
                        hawk.prev_pos = start;
                        hawk.state = HawkState::Circling {
                            t: HAWK_WARNING,
                            target: Some(c.id),
                        };
                        game.events.push(SimEvent::HawkCircling { pos: c.pos });
                    }
                    None => hawk.state = HawkState::Away { t: 2.0 },
                }
            }
        }
        HawkState::Circling { t, target } => {
            let aim = target
                .and_then(|id| game.chicks.iter().find(|c| c.id == id && c.is_on_ground()))
                .map(|c| c.pos);
            if let Some(aim) = aim {
                // Close the distance so as to arrive as the warning ends,
                // but no faster than a hawk glides.
                let k = (DT * 2.5 / t.max(0.25)).min(1.0);
                let step = (aim - hawk.pos) * k;
                let max = 9.0 * DT;
                hawk.pos += if step.length() > max { step.normalize() * max } else { step };
            }
            let t = t - DT;
            hawk.state = if t <= 0.0 {
                HawkState::Diving { t: 0.3 }
            } else {
                HawkState::Circling { t, target }
            };
        }
        HawkState::Diving { t } => {
            let t = t - DT;
            if t > 0.0 {
                hawk.state = HawkState::Diving { t };
            } else {
                strike(game, hawk.pos);
                hawk.state = HawkState::Leaving { t: 1.2 };
            }
        }
        HawkState::Leaving { t } => {
            hawk.pos += Vec2::new(5.0, -7.0) * DT;
            let t = t - DT;
            hawk.state = if t <= 0.0 {
                HawkState::Away {
                    t: game.rng.random_range(HAWK_COOLDOWN_MIN..HAWK_COOLDOWN_MAX),
                }
            } else {
                HawkState::Leaving { t }
            };
        }
    }
    game.hawk = Some(hawk);
}

fn edge_point(game: &Game, toward: Vec2) -> Vec2 {
    let w = game.topo.width() as f32;
    let h = game.topo.height() as f32;
    let center = Vec2::new(w / 2.0, h / 2.0);
    let dir = (toward - center).normalize_or(Vec2::new(0.0, -1.0));
    toward + dir * 3.0
}

fn strike(game: &mut Game, at: Vec2) {
    let topo = &game.topo;
    let victim = game
        .chicks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_on_ground() && !topo.grid.sheltered(TilePos::of(c.pos)))
        .filter(|(_, c)| c.pos.distance(at) < HAWK_STRIKE_RADIUS)
        .min_by(|a, b| a.1.pos.distance(at).total_cmp(&b.1.pos.distance(at)))
        .map(|(i, _)| i);
    let took = victim.map(|i| {
        let c = &mut game.chicks[i];
        c.state = ChickState::Lost;
        c.vel = Vec2::ZERO;
        c.id
    });
    game.events.push(SimEvent::HawkStrike { pos: at, took });
    if let Some(id) = took {
        game.lost += 1;
        game.events.push(SimEvent::ChickLost { chick: id, pos: at });
    }
    let hen = game.hen.pos();
    if game.hen.alive
        && game.hen.invulnerable <= 0.0
        && hen.distance(at) < HAWK_STRIKE_RADIUS
        && !game.topo.grid.sheltered(game.hen.mover.tile())
    {
        game.hen.stun = STUN_TIME;
        game.events.push(SimEvent::HenStunned { pos: hen });
    }
}
