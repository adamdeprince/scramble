//! Steering behaviours in the manner of Reynolds' "Steering Behaviors For
//! Autonomous Characters": seek, arrive, flee, separation, cohesion,
//! alignment and wall avoidance, combined with grid flow fields where walls
//! hide the target. Everything returns a desired velocity or a force in
//! tiles per second (squared).

use crate::world::nav::{self, DistField};
use crate::world::{Mover, TilePos, Topology};
use glam::Vec2;

pub fn seek(pos: Vec2, target: Vec2, max_speed: f32) -> Vec2 {
    (target - pos).normalize_or_zero() * max_speed
}

/// Seek that slows down inside `slow_radius` and stops at `stop_dist`.
pub fn arrive(pos: Vec2, target: Vec2, max_speed: f32, slow_radius: f32, stop_dist: f32) -> Vec2 {
    let to = target - pos;
    let d = to.length();
    if d <= stop_dist || d < 1e-5 {
        return Vec2::ZERO;
    }
    let speed = if d < slow_radius + stop_dist {
        max_speed * (d - stop_dist) / slow_radius
    } else {
        max_speed
    };
    to / d * speed
}

/// Offset pursuit with velocity feed-forward: move at the leader's speed
/// (only the part of it heading towards the target, which keeps followers
/// off the walls at corners) plus a correction proportional to how far the
/// target is, stopping at `stop_dist`.
pub fn pursue(pos: Vec2, target: Vec2, leader_vel: Vec2, max_speed: f32, stop_dist: f32, gain: f32) -> Vec2 {
    let to = target - pos;
    let d = to.length();
    if d <= stop_dist || d < 1e-5 {
        return Vec2::ZERO;
    }
    let dir = to / d;
    let feed = leader_vel.dot(dir).max(0.0);
    dir * (feed + (d - stop_dist) * gain).min(max_speed)
}

pub fn flee(pos: Vec2, threat: Vec2, max_speed: f32) -> Vec2 {
    (pos - threat).normalize_or_zero() * max_speed
}

/// Push away from neighbours closer than `radius`, stronger when closer.
pub fn separation(pos: Vec2, others: impl Iterator<Item = Vec2>, radius: f32) -> Vec2 {
    let mut force = Vec2::ZERO;
    for o in others {
        let d = pos - o;
        let len = d.length();
        if len < radius && len > 1e-5 {
            force += d / len * ((radius - len) / radius);
        } else if len <= 1e-5 {
            // Exactly overlapping: nudge deterministically.
            force += Vec2::new(0.3, 0.17);
        }
    }
    force
}

pub fn cohesion(pos: Vec2, others: impl Iterator<Item = Vec2>) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut n = 0;
    for o in others {
        sum += o;
        n += 1;
    }
    if n == 0 {
        return Vec2::ZERO;
    }
    sum / n as f32 - pos
}

pub fn alignment(vel: Vec2, others: impl Iterator<Item = Vec2>) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut n = 0;
    for v in others {
        sum += v;
        n += 1;
    }
    if n == 0 {
        return Vec2::ZERO;
    }
    sum / n as f32 - vel
}

/// Repulsion from impassable tiles within `margin` of the body's edge.
pub fn wall_avoid(topo: &Topology, pos: Vec2, radius: f32, margin: f32, who: Mover) -> Vec2 {
    let t = TilePos::of(pos);
    let reach = radius + margin;
    let mut force = Vec2::ZERO;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let q = TilePos::new(t.x + dx, t.y + dy);
            if topo.passable(q, who) {
                continue;
            }
            let min = Vec2::new(q.x as f32, q.y as f32);
            let closest = pos.clamp(min, min + Vec2::ONE);
            let d = pos - closest;
            let len = d.length();
            if len < reach && len > 1e-5 {
                force += d / len * ((reach - len) / margin).min(1.0);
            }
        }
    }
    force
}

/// The furthest point along the flow field's downhill route (up to
/// `lookahead` tiles) that can be walked to in a straight line: a cheap
/// form of string pulling that lets followers cut corners naturally instead
/// of zig-zagging tile by tile.
pub fn flow_target(topo: &Topology, field: &DistField, pos: Vec2, radius: f32, lookahead: usize, who: Mover) -> Option<Vec2> {
    let mut tile = TilePos::of(pos);
    if !field.reachable(tile) {
        return None;
    }
    let mut best = None;
    for _ in 0..lookahead {
        let Some(next) = field.downhill(tile) else {
            break;
        };
        let c = next.center();
        if nav::clear_walk(topo, pos, c, radius, who) {
            best = Some(c);
        } else if best.is_some() {
            break;
        } else {
            // Not even the first step is in a straight line (we are pressed
            // against a corner): aim at it anyway, the collision response
            // slides us round.
            best = Some(c);
            break;
        }
        tile = next;
    }
    best
}

/// How quickly a body closes the gap between its velocity and the desired
/// one, per second, before the acceleration limit applies.
const RESPONSE: f32 = 8.0;

/// Integrates a steering request with acceleration and speed limits.
pub fn integrate(vel: Vec2, desired: Vec2, force: Vec2, max_accel: f32, max_speed: f32, dt: f32) -> Vec2 {
    let mut dv = (desired - vel + force) * (RESPONSE * dt);
    let max_dv = max_accel * dt;
    if dv.length() > max_dv {
        dv = dv.normalize() * max_dv;
    }
    let v = vel + dv;
    if v.length() > max_speed {
        v.normalize() * max_speed
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrive_slows_and_stops() {
        let far = arrive(Vec2::ZERO, Vec2::new(5.0, 0.0), 4.0, 1.0, 0.5);
        assert!((far.length() - 4.0).abs() < 1e-5);
        let near = arrive(Vec2::ZERO, Vec2::new(1.0, 0.0), 4.0, 1.0, 0.5);
        assert!(near.length() < 4.0 && near.length() > 0.0);
        assert_eq!(arrive(Vec2::ZERO, Vec2::new(0.4, 0.0), 4.0, 1.0, 0.5), Vec2::ZERO);
    }

    #[test]
    fn separation_pushes_apart() {
        let f = separation(Vec2::ZERO, [Vec2::new(0.1, 0.0)].into_iter(), 0.4);
        assert!(f.x < 0.0);
        let none = separation(Vec2::ZERO, [Vec2::new(1.0, 0.0)].into_iter(), 0.4);
        assert_eq!(none, Vec2::ZERO);
    }

    #[test]
    fn integrate_respects_limits() {
        let v = integrate(Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::ZERO, 20.0, 5.0, 1.0 / 60.0);
        assert!(v.length() <= 20.0 / 60.0 + 1e-5);
        let mut v = Vec2::ZERO;
        for _ in 0..600 {
            v = integrate(v, Vec2::new(100.0, 0.0), Vec2::ZERO, 20.0, 5.0, 1.0 / 60.0);
        }
        assert!((v.length() - 5.0).abs() < 1e-3);
    }
}
