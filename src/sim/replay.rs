//! Input recordings: the seed, the level and every change of input, enough
//! to reproduce a game exactly with a headless run.

use super::{Game, Session, TickInput};
use crate::world::level::Level;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Recording {
    pub version: u32,
    pub seed: u64,
    pub level_index: usize,
    pub level_name: String,
    pub session: Session,
    /// Ticks whose input differs from carrying the previous one forward.
    pub inputs: Vec<(u64, TickInput)>,
    pub ticks: u64,
    pub final_hash: Option<u64>,
}

impl Recording {
    pub fn new(game: &Game) -> Self {
        Recording {
            version: FORMAT_VERSION,
            seed: game.seed,
            level_index: game.level_index,
            level_name: game.level.name.clone(),
            session: game.session.clone(),
            inputs: Vec::new(),
            ticks: 0,
            final_hash: None,
        }
    }

    /// Records the input applied at `tick` (1-based, as `Game::tick` after
    /// the step).
    pub fn push(&mut self, tick: u64, input: &TickInput) {
        let carried = self.carried_at(tick);
        if *input != carried {
            self.inputs.push((tick, input.clone()));
        }
        self.ticks = tick;
    }

    fn carried_at(&self, _tick: u64) -> TickInput {
        match self.inputs.last() {
            Some((_, last)) => TickInput {
                dir: last.dir,
                action: false,
                strokes: Vec::new(),
            },
            None => TickInput::default(),
        }
    }

    pub fn finish(&mut self, game: &Game) {
        self.final_hash = Some(game.state_hash());
    }

    /// Re-runs the recording, returning the finished game.
    pub fn replay(&self, level: Level) -> Game {
        let mut game = Game::new(level, self.level_index, self.session.clone(), self.seed);
        let mut next = 0;
        let mut carried = TickInput::default();
        for tick in 1..=self.ticks {
            let input = if next < self.inputs.len() && self.inputs[next].0 == tick {
                let i = self.inputs[next].1.clone();
                next += 1;
                i
            } else {
                carried.clone()
            };
            game.step(&input);
            game.drain_events();
            carried = TickInput {
                dir: input.dir,
                action: false,
                strokes: Vec::new(),
            };
        }
        game
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }
}
