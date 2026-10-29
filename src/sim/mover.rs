//! Lane movement for creatures that travel the garden paths tile by tile
//! (the hen, foxes, weasels): they move along the centre lines of corridors,
//! may reverse at any moment, and turn at junctions with a tolerance window
//! so that a turn pressed slightly early or late still takes. The offset left
//! by an early or late turn is worked off smoothly, cutting the corner.

use crate::world::{Dir, Mover, TilePos, Topology};
use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaneMover {
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub dir: Option<Dir>,
    pub facing: Dir,
    pub who: Mover,
    /// Distance travelled, for walk-cycle animation.
    pub odometer: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Moved {
    pub entered_tile: bool,
    pub blocked: bool,
    pub turned: bool,
}

impl LaneMover {
    pub fn new(at: TilePos, who: Mover) -> Self {
        LaneMover {
            pos: at.center(),
            prev_pos: at.center(),
            dir: None,
            facing: Dir::Down,
            who,
            odometer: 0.0,
        }
    }

    pub fn tile(&self) -> TilePos {
        TilePos::of(self.pos)
    }

    pub fn place(&mut self, at: TilePos) {
        self.pos = at.center();
        self.prev_pos = self.pos;
        self.dir = None;
    }

    fn open(&self, topo: &Topology, t: TilePos) -> bool {
        topo.passable(t, self.who)
    }

    /// Moves `dist` along the lane, turning towards `want` when possible.
    pub fn advance(&mut self, topo: &Topology, want: Option<Dir>, dist: f32, window: f32) -> Moved {
        let mut out = Moved::default();
        let start_tile = self.tile();
        let tile = start_tile;
        let c = tile.center();

        if let Some(w) = want {
            match self.dir {
                Some(d) if d == w => {}
                Some(d) if d == w.opposite() => {
                    self.dir = Some(w);
                    out.turned = true;
                }
                Some(d) => {
                    let along = (self.pos - c).dot(d.vec());
                    if along.abs() <= window && self.open(topo, tile.step(w)) {
                        self.dir = Some(w);
                        out.turned = true;
                    }
                }
                None => {
                    // From a standstill, go if the way is open or we are not
                    // yet at the centre of the tile in that direction.
                    let along = (self.pos - c).dot(w.vec());
                    if self.open(topo, tile.step(w)) || along < -0.01 {
                        self.dir = Some(w);
                        out.turned = true;
                    }
                }
            }
        }

        let Some(d) = self.dir else {
            return out;
        };
        self.facing = d;
        let axis = d.vec();
        let perp = Vec2::new(axis.y.abs(), axis.x.abs());

        // Work off the offset from the lane centre left by a cornering turn.
        let off = (self.pos - c).dot(perp);
        if off.abs() > 1e-5 {
            let fix = off.abs().min(dist * 0.9);
            self.pos -= perp * off.signum() * fix;
        }

        let along = (self.pos - c).dot(axis);
        let mut step = dist;
        if !self.open(topo, tile.step(d)) {
            // Only as far as the centre of this tile.
            let room = (-along).max(0.0);
            if room <= 1e-5 {
                self.pos -= axis * along.max(0.0);
                self.dir = None;
                out.blocked = true;
                return out;
            }
            if step >= room {
                step = room;
                out.blocked = true;
            }
        }
        self.pos += axis * step;
        self.odometer += step;
        if out.blocked {
            self.dir = None;
        }
        out.entered_tile = self.tile() != start_tile;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Grid, Tile};

    fn topo(rows: &[&str]) -> Topology {
        let h = rows.len() as i32;
        let w = rows[0].len() as i32;
        let mut g = Grid::new(w, h, Tile::Hedge);
        for (y, r) in rows.iter().enumerate() {
            for (x, c) in r.chars().enumerate() {
                g.set(TilePos::new(x as i32, y as i32), Tile::from_char(c).unwrap());
            }
        }
        Topology::new(g)
    }

    #[test]
    fn stops_at_walls() {
        let t = topo(&["#####", "#...#", "#####"]);
        let mut m = LaneMover::new(TilePos::new(1, 1), Mover::Hen);
        for _ in 0..100 {
            m.advance(&t, Some(Dir::Right), 0.07, 0.3);
        }
        assert!((m.pos.x - 3.5).abs() < 1e-4, "{}", m.pos);
        assert_eq!(m.dir, None);
    }

    #[test]
    fn early_turn_is_taken_and_corner_cut() {
        let t = topo(&["#####", "#...#", "##.##", "##.##", "#####"]);
        let mut m = LaneMover::new(TilePos::new(1, 1), Mover::Hen);
        m.dir = Some(Dir::Right);
        // Approach the junction at x=2 and press down 0.2 tiles early.
        m.pos = Vec2::new(2.3, 1.5);
        let r = m.advance(&t, Some(Dir::Down), 0.07, 0.34);
        assert!(r.turned);
        assert_eq!(m.dir, Some(Dir::Down));
        for _ in 0..20 {
            m.advance(&t, Some(Dir::Down), 0.07, 0.34);
        }
        assert!((m.pos.x - 2.5).abs() < 1e-3, "lane centred: {}", m.pos);
        assert!(m.pos.y > 2.5);
    }

    #[test]
    fn turns_into_walls_are_ignored() {
        let t = topo(&["#####", "#...#", "#####"]);
        let mut m = LaneMover::new(TilePos::new(1, 1), Mover::Hen);
        m.dir = Some(Dir::Right);
        m.advance(&t, Some(Dir::Up), 0.07, 0.34);
        assert_eq!(m.dir, Some(Dir::Right));
    }

    #[test]
    fn reverse_is_immediate() {
        let t = topo(&["######", "#....#", "######"]);
        let mut m = LaneMover::new(TilePos::new(2, 1), Mover::Fox);
        m.dir = Some(Dir::Right);
        m.pos = Vec2::new(2.8, 1.5);
        m.advance(&t, Some(Dir::Left), 0.1, 0.3);
        assert_eq!(m.dir, Some(Dir::Left));
        assert!(m.pos.x < 2.8);
    }
}
