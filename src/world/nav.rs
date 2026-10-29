//! Navigation over the tile grid: breadth-first distance fields (flow fields
//! the flock follows), A* routes for predators, line-of-sight rays and
//! region queries used to keep fences fair.

use super::{Dir, Mover, TilePos, Topology};
use glam::Vec2;
use std::collections::VecDeque;

/// Breadth-first distances (in tiles) from a set of sources.
#[derive(Clone, Debug)]
pub struct DistField {
    pub width: i32,
    pub height: i32,
    dist: Vec<u32>,
}

pub const UNREACHABLE: u32 = u32::MAX;

impl DistField {
    pub fn compute(topo: &Topology, sources: &[TilePos], who: Mover) -> Self {
        Self::compute_with(topo, sources, |p| topo.passable(p, who))
    }

    pub fn compute_with(topo: &Topology, sources: &[TilePos], passable: impl Fn(TilePos) -> bool) -> Self {
        let w = topo.width();
        let h = topo.height();
        let mut dist = vec![UNREACHABLE; (w * h).max(0) as usize];
        let mut queue = VecDeque::new();
        for &s in sources {
            if let Some(i) = topo.grid.index(s) {
                if dist[i] == UNREACHABLE {
                    // Sources count even when they are not passable to the
                    // mover (a chick standing in a doorway, a hen on a gate
                    // that just closed), so fields never vanish.
                    dist[i] = 0;
                    queue.push_back(s);
                }
            }
        }
        while let Some(p) = queue.pop_front() {
            let d = dist[topo.grid.index(p).expect("queued tiles are in bounds")];
            for dir in Dir::ALL {
                let q = p.step(dir);
                if let Some(qi) = topo.grid.index(q) {
                    if dist[qi] == UNREACHABLE && passable(q) {
                        dist[qi] = d + 1;
                        queue.push_back(q);
                    }
                }
            }
        }
        DistField { width: w, height: h, dist }
    }

    fn index(&self, p: TilePos) -> Option<usize> {
        (p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height).then(|| (p.y * self.width + p.x) as usize)
    }

    pub fn get(&self, p: TilePos) -> u32 {
        self.index(p).map_or(UNREACHABLE, |i| self.dist[i])
    }

    pub fn reachable(&self, p: TilePos) -> bool {
        self.get(p) != UNREACHABLE
    }

    pub fn count_reachable(&self) -> usize {
        self.dist.iter().filter(|&&d| d != UNREACHABLE).count()
    }

    /// The neighbouring tile one step closer to the sources.
    pub fn downhill(&self, p: TilePos) -> Option<TilePos> {
        let here = self.get(p);
        let mut best: Option<(u32, TilePos)> = None;
        for dir in Dir::ALL {
            let q = p.step(dir);
            let d = self.get(q);
            if d == UNREACHABLE {
                continue;
            }
            if d < here && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
        best.map(|(_, q)| q)
    }

    /// The neighbouring tile one step further from the sources.
    pub fn uphill(&self, p: TilePos) -> Option<TilePos> {
        let here = self.get(p);
        let mut best: Option<(u32, TilePos)> = None;
        for dir in Dir::ALL {
            let q = p.step(dir);
            let d = self.get(q);
            if d == UNREACHABLE {
                continue;
            }
            if (here == UNREACHABLE || d > here) && best.is_none_or(|(bd, _)| d > bd) {
                best = Some((d, q));
            }
        }
        best.map(|(_, q)| q)
    }

    /// Continuous path distance for a point inside a tile: the tile's
    /// distance, less how far the point has already moved towards the next
    /// tile downhill. Points sharing a tile are thus ordered along the path.
    pub fn at_point(&self, p: Vec2) -> f32 {
        let t = TilePos::of(p);
        let d = self.get(t);
        if d == UNREACHABLE {
            return f32::INFINITY;
        }
        match self.downhill(t) {
            Some(n) => d as f32 - (p - t.center()).dot(n.center() - t.center()),
            None => d as f32 + (p - t.center()).length() * 0.5,
        }
    }
}

/// Connected regions of the yard for one kind of mover: two tiles with the
/// same label can reach each other.
#[derive(Clone, Debug)]
pub struct Regions {
    pub version: u64,
    width: i32,
    label: Vec<u32>,
}

impl Regions {
    pub const NONE: u32 = u32::MAX;

    pub fn compute(topo: &Topology, who: Mover) -> Self {
        let w = topo.width();
        let n = topo.grid.len();
        let mut label = vec![Self::NONE; n];
        let mut next = 0;
        let mut queue = VecDeque::new();
        for start in 0..n {
            let p = topo.grid.pos_of(start);
            if label[start] != Self::NONE || !topo.passable(p, who) {
                continue;
            }
            label[start] = next;
            queue.push_back(p);
            while let Some(q) = queue.pop_front() {
                for d in Dir::ALL {
                    let r = q.step(d);
                    if let Some(ri) = topo.grid.index(r) {
                        if label[ri] == Self::NONE && topo.passable(r, who) {
                            label[ri] = next;
                            queue.push_back(r);
                        }
                    }
                }
            }
            next += 1;
        }
        Regions {
            version: topo.version,
            width: w,
            label,
        }
    }

    pub fn of(&self, p: TilePos) -> u32 {
        if p.x < 0 || p.y < 0 || p.x >= self.width {
            return Self::NONE;
        }
        self.label.get((p.y * self.width + p.x) as usize).copied().unwrap_or(Self::NONE)
    }

    pub fn connected(&self, a: TilePos, b: TilePos) -> bool {
        let ra = self.of(a);
        ra != Self::NONE && ra == self.of(b)
    }
}

/// Grid ray cast (Amanatides and Woo): true when no tile strictly between
/// the endpoints blocks the view. Rays through a corner are blocked when
/// either tile beside the corner blocks.
pub fn line_of_sight(a: Vec2, b: Vec2, blocks: impl Fn(TilePos) -> bool) -> bool {
    let start = TilePos::of(a);
    let end = TilePos::of(b);
    if start == end {
        return true;
    }
    let d = b - a;
    let step_x: i32 = if d.x > 0.0 { 1 } else { -1 };
    let step_y: i32 = if d.y > 0.0 { 1 } else { -1 };
    let t_delta_x = if d.x != 0.0 { (1.0 / d.x).abs() } else { f32::INFINITY };
    let t_delta_y = if d.y != 0.0 { (1.0 / d.y).abs() } else { f32::INFINITY };
    let frac_x = a.x - a.x.floor();
    let frac_y = a.y - a.y.floor();
    let mut t_max_x = if d.x > 0.0 {
        (1.0 - frac_x) * t_delta_x
    } else if d.x < 0.0 {
        frac_x * t_delta_x
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if d.y > 0.0 {
        (1.0 - frac_y) * t_delta_y
    } else if d.y < 0.0 {
        frac_y * t_delta_y
    } else {
        f32::INFINITY
    };
    let mut cur = start;
    let max_steps = (start.manhattan(end) + 2) as usize;
    for _ in 0..max_steps {
        if (t_max_x - t_max_y).abs() < 1e-6 {
            // Passing exactly through a corner.
            let side_a = TilePos::new(cur.x + step_x, cur.y);
            let side_b = TilePos::new(cur.x, cur.y + step_y);
            if (side_a != end && blocks(side_a)) || (side_b != end && blocks(side_b)) {
                return false;
            }
            cur = TilePos::new(cur.x + step_x, cur.y + step_y);
            t_max_x += t_delta_x;
            t_max_y += t_delta_y;
        } else if t_max_x < t_max_y {
            cur.x += step_x;
            t_max_x += t_delta_x;
        } else {
            cur.y += step_y;
            t_max_y += t_delta_y;
        }
        if cur == end {
            return true;
        }
        if blocks(cur) {
            return false;
        }
    }
    true
}

/// Whether a creature of radius `r` can walk straight from `a` to `b`.
pub fn clear_walk(topo: &Topology, a: Vec2, b: Vec2, r: f32, who: Mover) -> bool {
    let len = a.distance(b);
    let steps = (len / 0.2).ceil().max(1.0) as usize;
    (0..=steps).all(|i| {
        let p = a.lerp(b, i as f32 / steps as f32);
        !topo.circle_blocked(p, r, who)
    })
}

/// A shortest route between two tiles, excluding the start.
pub fn route(topo: &Topology, from: TilePos, to: TilePos, who: Mover) -> Option<Vec<TilePos>> {
    let (path, _) = pathfinding::prelude::astar(
        &from,
        |&p| {
            Dir::ALL
                .into_iter()
                .map(move |d| p.step(d))
                .filter(|&q| q == to || topo.passable(q, who))
                .map(|q| (q, 1u32))
                .collect::<Vec<_>>()
        },
        |&p| p.manhattan(to),
        |&p| p == to,
    )?;
    Some(path.into_iter().skip(1).collect())
}

/// Number of tiles reachable from `from` when the extra tiles are blocked.
pub fn region_size(topo: &Topology, from: TilePos, who: Mover, extra_blocked: &[TilePos]) -> usize {
    DistField::compute_with(topo, &[from], |p| topo.passable(p, who) && !extra_blocked.contains(&p)).count_reachable()
}

/// Tiles a straight stroke from `a` to `b` passes through, in order
/// (a supercover line: every tile the segment touches).
pub fn tiles_on_segment(a: Vec2, b: Vec2) -> Vec<TilePos> {
    let mut out = vec![TilePos::of(a)];
    let len = a.distance(b);
    let steps = (len / 0.1).ceil() as usize;
    for i in 1..=steps {
        let t = TilePos::of(a.lerp(b, i as f32 / steps as f32));
        if *out.last().expect("starts non-empty") != t {
            let last = *out.last().expect("starts non-empty");
            // Insert the shared neighbour when stepping diagonally so the
            // tiles form a 4-connected chain, which is what blocks a path.
            if last.x != t.x && last.y != t.y {
                out.push(TilePos::new(t.x, last.y));
            }
            out.push(t);
        }
    }
    out
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
    fn bfs_distances_follow_corridors() {
        let t = topo(&["#####", "#...#", "###.#", "#...#", "#####"]);
        let f = DistField::compute(&t, &[TilePos::new(1, 1)], Mover::Hen);
        assert_eq!(f.get(TilePos::new(3, 1)), 2);
        assert_eq!(f.get(TilePos::new(1, 3)), 6);
        assert!(!f.reachable(TilePos::new(0, 0)));
        assert_eq!(f.downhill(TilePos::new(1, 3)), Some(TilePos::new(2, 3)));
    }

    #[test]
    fn sight_is_blocked_by_hedges_not_flowers() {
        let t = topo(&["#######", "#..#..#", "#..f..#", "#######"]);
        let blocks = |p: TilePos| t.blocks_sight(p);
        assert!(!line_of_sight(Vec2::new(1.5, 1.5), Vec2::new(5.5, 1.5), blocks));
        assert!(line_of_sight(Vec2::new(1.5, 2.5), Vec2::new(5.5, 2.5), blocks));
        assert!(line_of_sight(Vec2::new(1.5, 1.5), Vec2::new(2.5, 2.5), blocks));
    }

    #[test]
    fn corner_rays_are_blocked() {
        let t = topo(&["####", "#.##", "##.#", "####"]);
        let blocks = |p: TilePos| t.blocks_sight(p);
        assert!(!line_of_sight(Vec2::new(1.5, 1.5), Vec2::new(2.5, 2.5), blocks));
    }

    #[test]
    fn routes_avoid_walls() {
        let t = topo(&["#####", "#...#", "#.#.#", "#...#", "#####"]);
        let r = route(&t, TilePos::new(1, 1), TilePos::new(3, 3), Mover::Fox).unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(*r.last().unwrap(), TilePos::new(3, 3));
    }

    #[test]
    fn segment_tiles_are_four_connected() {
        let tiles = tiles_on_segment(Vec2::new(1.2, 1.2), Vec2::new(4.8, 3.9));
        for w in tiles.windows(2) {
            assert_eq!(w[0].manhattan(w[1]), 1, "{tiles:?}");
        }
        assert_eq!(tiles[0], TilePos::new(1, 1));
        assert_eq!(*tiles.last().unwrap(), TilePos::new(4, 3));
    }

    #[test]
    fn region_size_counts_with_blocks() {
        let t = topo(&["######", "#....#", "######"]);
        assert_eq!(region_size(&t, TilePos::new(1, 1), Mover::Fox, &[]), 4);
        assert_eq!(region_size(&t, TilePos::new(1, 1), Mover::Fox, &[TilePos::new(3, 1)]), 2);
    }
}
