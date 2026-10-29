//! The garden: static tile grid, level definitions, dynamic topology (gates
//! and temporary fences) and navigation queries.

pub mod level;
pub mod nav;

use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    pub const fn new(x: i32, y: i32) -> Self {
        TilePos { x, y }
    }

    pub fn center(self) -> Vec2 {
        Vec2::new(self.x as f32 + 0.5, self.y as f32 + 0.5)
    }

    pub fn of(p: Vec2) -> Self {
        TilePos::new(p.x.floor() as i32, p.y.floor() as i32)
    }

    pub fn step(self, d: Dir) -> Self {
        let (dx, dy) = d.delta();
        TilePos::new(self.x + dx, self.y + dy)
    }

    pub fn manhattan(self, o: TilePos) -> u32 {
        self.x.abs_diff(o.x) + self.y.abs_diff(o.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Left, Dir::Down, Dir::Right];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }

    pub fn vec(self) -> Vec2 {
        let (x, y) = self.delta();
        Vec2::new(x as f32, y as f32)
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }

    /// The direction closest to a vector, if it is not zero.
    pub fn from_vec(v: Vec2) -> Option<Dir> {
        if v.length_squared() < 1e-8 {
            return None;
        }
        Some(if v.x.abs() >= v.y.abs() {
            if v.x > 0.0 { Dir::Right } else { Dir::Left }
        } else if v.y > 0.0 {
            Dir::Down
        } else {
            Dir::Up
        })
    }
}

/// Static terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tile {
    /// Bare dirt path: open ground, exposed to hawks.
    Path,
    /// Short lawn: open ground, exposed.
    Lawn,
    /// Tall grass and ferns: walkable, hides chicks from hawks.
    Cover,
    /// Hedge: wall that blocks sight.
    Hedge,
    /// Flower bed: low wall.
    Flowers,
    /// White picket fence: low wall.
    Picket,
    /// Pond: low wall.
    Water,
    /// Vegetable patch: low wall.
    Veg,
    /// Dry stone wall: blocks sight.
    Stone,
    /// Tree trunk and canopy: blocks sight, shades its neighbours.
    Tree,
    /// The henhouse building.
    Coop,
    /// The coop door: the brood's way home; predators will not enter.
    Door,
    /// Drain pipe: only weasels fit.
    Pipe,
    /// Fox den: walkable, predators spawn here.
    Den,
    /// A garden gate on a timer.
    GateTimed,
    /// A latched gate the hen can open and close.
    GateLatch,
}

impl Tile {
    pub fn from_char(c: char) -> Option<Tile> {
        Some(match c {
            '.' => Tile::Path,
            ',' => Tile::Lawn,
            'b' => Tile::Cover,
            '#' => Tile::Hedge,
            'f' => Tile::Flowers,
            'p' => Tile::Picket,
            'w' => Tile::Water,
            'v' => Tile::Veg,
            's' => Tile::Stone,
            'T' => Tile::Tree,
            'C' => Tile::Coop,
            'D' => Tile::Door,
            '=' => Tile::Pipe,
            'F' => Tile::Den,
            'G' => Tile::GateTimed,
            'L' => Tile::GateLatch,
            _ => return None,
        })
    }

    pub fn to_char(self) -> char {
        match self {
            Tile::Path => '.',
            Tile::Lawn => ',',
            Tile::Cover => 'b',
            Tile::Hedge => '#',
            Tile::Flowers => 'f',
            Tile::Picket => 'p',
            Tile::Water => 'w',
            Tile::Veg => 'v',
            Tile::Stone => 's',
            Tile::Tree => 'T',
            Tile::Coop => 'C',
            Tile::Door => 'D',
            Tile::Pipe => '=',
            Tile::Den => 'F',
            Tile::GateTimed => 'G',
            Tile::GateLatch => 'L',
        }
    }

    /// Solid scenery nobody walks through.
    pub fn is_wall(self) -> bool {
        matches!(
            self,
            Tile::Hedge
                | Tile::Flowers
                | Tile::Picket
                | Tile::Water
                | Tile::Veg
                | Tile::Stone
                | Tile::Tree
                | Tile::Coop
        )
    }

    /// Open ground that anybody may walk on.
    pub fn is_ground(self) -> bool {
        matches!(self, Tile::Path | Tile::Lawn | Tile::Cover | Tile::Den)
    }

    pub fn is_gate(self) -> bool {
        matches!(self, Tile::GateTimed | Tile::GateLatch)
    }

    /// Scenery tall enough to hide what is behind it.
    pub fn blocks_sight(self) -> bool {
        matches!(self, Tile::Hedge | Tile::Stone | Tile::Tree | Tile::Coop)
    }
}

/// Who is moving: passability differs per kind of creature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mover {
    Hen,
    Chick,
    Fox,
    Weasel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<Tile>,
}

impl Grid {
    pub fn new(width: i32, height: i32, fill: Tile) -> Self {
        Grid {
            width,
            height,
            tiles: vec![fill; (width.max(0) * height.max(0)) as usize],
        }
    }

    pub fn in_bounds(&self, p: TilePos) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height
    }

    pub fn index(&self, p: TilePos) -> Option<usize> {
        self.in_bounds(p).then(|| (p.y * self.width + p.x) as usize)
    }

    pub fn pos_of(&self, index: usize) -> TilePos {
        TilePos::new(index as i32 % self.width, index as i32 / self.width)
    }

    /// Out-of-bounds positions read as hedge, so the yard is always closed.
    pub fn get(&self, p: TilePos) -> Tile {
        self.index(p).map_or(Tile::Hedge, |i| self.tiles[i])
    }

    pub fn set(&mut self, p: TilePos, t: Tile) {
        if let Some(i) = self.index(p) {
            self.tiles[i] = t;
        }
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn positions(&self) -> impl Iterator<Item = TilePos> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| TilePos::new(x, y)))
    }

    /// Tiles shaded from a hawk: cover, and ground next to a tree.
    pub fn sheltered(&self, p: TilePos) -> bool {
        match self.get(p) {
            Tile::Cover | Tile::Door => true,
            t if t.is_ground() => Dir::ALL.iter().any(|&d| self.get(p.step(d)) == Tile::Tree),
            _ => false,
        }
    }
}

/// Per-tile dynamic blockers on top of the static grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dynamic {
    pub gate_closed: bool,
    pub fence: Option<u32>,
}

/// The walkable structure of the yard at this moment: the grid plus gates
/// and fences. `version` changes whenever passability changes so cached
/// navigation can be refreshed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub grid: Grid,
    pub dynamic: Vec<Dynamic>,
    pub version: u64,
}

impl Topology {
    pub fn new(grid: Grid) -> Self {
        let n = grid.len();
        Topology {
            grid,
            dynamic: vec![Dynamic::default(); n],
            version: 0,
        }
    }

    pub fn width(&self) -> i32 {
        self.grid.width
    }

    pub fn height(&self) -> i32 {
        self.grid.height
    }

    pub fn dyn_at(&self, p: TilePos) -> Dynamic {
        self.grid.index(p).map_or(Dynamic::default(), |i| self.dynamic[i])
    }

    pub fn set_gate(&mut self, p: TilePos, closed: bool) {
        if let Some(i) = self.grid.index(p) {
            if self.dynamic[i].gate_closed != closed {
                self.dynamic[i].gate_closed = closed;
                self.version += 1;
            }
        }
    }

    pub fn set_fence(&mut self, p: TilePos, fence: Option<u32>) {
        if let Some(i) = self.grid.index(p) {
            if self.dynamic[i].fence != fence {
                self.dynamic[i].fence = fence;
                self.version += 1;
            }
        }
    }

    pub fn passable(&self, p: TilePos, who: Mover) -> bool {
        let Some(i) = self.grid.index(p) else {
            return false;
        };
        let t = self.grid.tiles[i];
        let d = self.dynamic[i];
        if d.fence.is_some() {
            return false;
        }
        match t {
            _ if t.is_wall() => false,
            Tile::GateTimed | Tile::GateLatch => !d.gate_closed,
            Tile::Door => matches!(who, Mover::Hen | Mover::Chick),
            Tile::Pipe => who == Mover::Weasel,
            Tile::Den => true,
            _ => t.is_ground(),
        }
    }

    pub fn blocks_sight(&self, p: TilePos) -> bool {
        let t = self.grid.get(p);
        t.blocks_sight() || (t.is_gate() && self.dyn_at(p).gate_closed)
    }

    /// Tiles a temporary fence may stand on: open ground and the yard's
    /// paths, never doors, dens, pipes or gates.
    pub fn fenceable(&self, p: TilePos) -> bool {
        matches!(self.grid.get(p), Tile::Path | Tile::Lawn | Tile::Cover) && self.dyn_at(p).fence.is_none()
    }

    /// Whether a circle of radius `r` at `p` overlaps an impassable tile.
    pub fn circle_blocked(&self, p: Vec2, r: f32, who: Mover) -> bool {
        let t = TilePos::of(p);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let q = TilePos::new(t.x + dx, t.y + dy);
                if self.passable(q, who) {
                    continue;
                }
                let min = Vec2::new(q.x as f32, q.y as f32);
                let closest = p.clamp(min, min + Vec2::ONE);
                if closest.distance_squared(p) < r * r {
                    return true;
                }
            }
        }
        false
    }

    /// Pushes a circle out of every impassable tile it overlaps. Returns the
    /// corrected position and the summed push (a contact normal).
    pub fn resolve_circle(&self, mut p: Vec2, r: f32, who: Mover) -> (Vec2, Vec2) {
        let mut total = Vec2::ZERO;
        for _ in 0..3 {
            let t = TilePos::of(p);
            let mut moved = false;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let q = TilePos::new(t.x + dx, t.y + dy);
                    if self.passable(q, who) {
                        continue;
                    }
                    let min = Vec2::new(q.x as f32, q.y as f32);
                    let max = min + Vec2::ONE;
                    let closest = p.clamp(min, max);
                    let delta = p - closest;
                    let d2 = delta.length_squared();
                    if d2 < r * r {
                        let push = if d2 > 1e-10 {
                            let d = d2.sqrt();
                            delta / d * (r - d)
                        } else {
                            // Centre inside the tile: push out through the
                            // nearest face.
                            let to_left = p.x - min.x;
                            let to_right = max.x - p.x;
                            let to_top = p.y - min.y;
                            let to_bottom = max.y - p.y;
                            let m = to_left.min(to_right).min(to_top).min(to_bottom);
                            if m == to_left {
                                Vec2::new(-(to_left + r), 0.0)
                            } else if m == to_right {
                                Vec2::new(to_right + r, 0.0)
                            } else if m == to_top {
                                Vec2::new(0.0, -(to_top + r))
                            } else {
                                Vec2::new(0.0, to_bottom + r)
                            }
                        };
                        p += push;
                        total += push;
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        (p, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn passability_per_mover() {
        let t = topo(&["#####", "#.D=#", "#####"]);
        assert!(t.passable(TilePos::new(1, 1), Mover::Fox));
        assert!(t.passable(TilePos::new(2, 1), Mover::Hen));
        assert!(!t.passable(TilePos::new(2, 1), Mover::Fox));
        assert!(t.passable(TilePos::new(3, 1), Mover::Weasel));
        assert!(!t.passable(TilePos::new(3, 1), Mover::Hen));
        assert!(!t.passable(TilePos::new(0, 0), Mover::Weasel));
    }

    #[test]
    fn fences_and_gates_change_version() {
        let mut t = topo(&["#####", "#.G.#", "#####"]);
        let v0 = t.version;
        t.set_gate(TilePos::new(2, 1), true);
        assert!(!t.passable(TilePos::new(2, 1), Mover::Hen));
        assert!(t.blocks_sight(TilePos::new(2, 1)));
        t.set_fence(TilePos::new(1, 1), Some(3));
        assert!(!t.passable(TilePos::new(1, 1), Mover::Fox));
        assert!(t.version >= v0 + 2);
        // Setting the same state again does not bump the version.
        let v = t.version;
        t.set_fence(TilePos::new(1, 1), Some(3));
        assert_eq!(t.version, v);
    }

    #[test]
    fn circle_resolution_pushes_out_of_walls() {
        let t = topo(&["#####", "#...#", "#####"]);
        let (p, n) = t.resolve_circle(Vec2::new(2.5, 1.1), 0.2, Mover::Chick);
        assert!((p.y - 1.2).abs() < 1e-4, "{p}");
        assert!(n.y > 0.0);
        let (p, _) = t.resolve_circle(Vec2::new(2.5, 1.5), 0.2, Mover::Chick);
        assert_eq!(p, Vec2::new(2.5, 1.5));
        assert!(!t.circle_blocked(Vec2::new(2.5, 1.5), 0.2, Mover::Chick));
    }
}
