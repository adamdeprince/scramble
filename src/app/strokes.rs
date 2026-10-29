//! Fence strokes from a mouse, a pen or a finger.
//!
//! A stroke is collected in yard coordinates while it is drawn and handed to
//! the simulation when it ends, which decides what becomes fence. Until the
//! simulation answers, the stroke stays visible: drawn by the terminal
//! itself on Silkie with a pen (terminal-side ink), as a vector pen stroke
//! the game appends to on Silkie otherwise, or as a trail of ink dots.

use crate::render::StrokeView;
use crate::sim::FenceStroke;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Mouse,
    Pen,
    Finger,
}

#[derive(Clone, Debug)]
pub struct Stroke {
    pub id: u64,
    pub source: Source,
    pub points: Vec<Vec2>,
    /// The element id of the terminal's ink, when the terminal draws it.
    pub ink: Option<u32>,
    /// Terminal timestamp (ms) of the input that started it, for latency.
    pub t0: Option<u64>,
}

#[derive(Default)]
pub struct Strokes {
    pub active: Option<Stroke>,
    /// Finished, waiting for the simulation's verdict.
    pub pending: Vec<Stroke>,
    /// Finished strokes not yet handed to the simulation.
    pub outbox: Vec<FenceStroke>,
    next: u64,
    /// Ink elements to delete once their stroke has been judged.
    pub ink_to_delete: Vec<u32>,
}

/// Ignore sub-pixel jitter: new points closer than this (in tiles) to the
/// last one are dropped.
const MIN_STEP: f32 = 0.04;

/// A stroke that stays this close (in tiles) to where it began is a tap.
const TAP_RADIUS: f32 = 0.33;

impl Strokes {
    pub fn begin(&mut self, at: Vec2, source: Source, ink: Option<u32>, t0: Option<u64>) {
        if let Some(s) = self.active.take() {
            // A fresh down while one is open replaces it.
            if let Some(i) = s.ink {
                self.ink_to_delete.push(i);
            }
        }
        self.next += 1;
        self.active = Some(Stroke {
            id: self.next,
            source,
            points: vec![at],
            ink,
            t0,
        });
    }

    pub fn extend(&mut self, at: Vec2) {
        if let Some(s) = &mut self.active {
            if s.points.last().is_none_or(|p| p.distance(at) >= MIN_STEP) {
                s.points.push(at);
            }
        }
    }

    /// Ends the stroke and hands it to the simulation as a fence, unless
    /// it was a tap (it never left a third of a tile) on something
    /// `interactive` claims, such as a latched gate: then it is consumed and
    /// `true` returned.
    pub fn finish(&mut self, at: Option<Vec2>, interactive: impl Fn(Vec2) -> bool) -> bool {
        if let Some(p) = at {
            self.extend(p);
        }
        let Some(s) = self.active.take() else {
            return false;
        };
        let first = s.points[0];
        let tap = s.points.iter().all(|p| p.distance(first) < TAP_RADIUS);
        if tap && interactive(first) {
            if let Some(ink) = s.ink {
                self.ink_to_delete.push(ink);
            }
            return true;
        }
        self.outbox.push(FenceStroke {
            id: s.id,
            points: s.points.iter().map(|p| [p.x, p.y]).collect(),
        });
        self.pending.push(s);
        false
    }

    /// A stroke the terminal cancelled never happened (its ink is already
    /// gone).
    pub fn cancel(&mut self) {
        self.active = None;
    }

    /// The simulation has judged stroke `id`.
    pub fn judged(&mut self, id: u64) {
        if let Some(i) = self.pending.iter().position(|s| s.id == id) {
            let s = self.pending.remove(i);
            if let Some(ink) = s.ink {
                self.ink_to_delete.push(ink);
            }
        }
    }

    /// Drops everything (level change, pause).
    pub fn clear(&mut self) {
        for s in self.active.take().into_iter().chain(self.pending.drain(..)) {
            if let Some(ink) = s.ink {
                self.ink_to_delete.push(ink);
            }
        }
        self.outbox.clear();
    }

    pub fn views(&self) -> Vec<StrokeView> {
        self.active
            .iter()
            .chain(self.pending.iter())
            .map(|s| StrokeView {
                id: s.id,
                points: s.points.clone(),
                terminal_ink: s.ink.is_some(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_flow_to_the_simulation_and_back() {
        let mut s = Strokes::default();
        s.begin(Vec2::new(1.0, 1.0), Source::Pen, Some(10_000_001), None);
        s.extend(Vec2::new(1.01, 1.0));
        s.extend(Vec2::new(2.0, 1.0));
        assert!(!s.finish(Some(Vec2::new(3.0, 1.0)), |_| true), "a drag is never a tap");
        assert_eq!(s.outbox.len(), 1);
        assert_eq!(s.outbox[0].points.len(), 3, "jitter dropped");
        assert_eq!(s.views().len(), 1);
        assert!(s.views()[0].terminal_ink);
        let id = s.outbox[0].id;
        s.judged(id);
        assert!(s.views().is_empty());
        assert_eq!(s.ink_to_delete, vec![10_000_001]);
    }

    #[test]
    fn taps_on_interactive_things_are_consumed() {
        let mut s = Strokes::default();
        s.begin(Vec2::new(5.5, 5.5), Source::Mouse, None, None);
        s.extend(Vec2::new(5.6, 5.5));
        assert!(s.finish(None, |p| p.x > 5.0));
        assert!(s.outbox.is_empty() && s.views().is_empty());
        // A tap on plain ground is a one-tile fence.
        s.begin(Vec2::new(2.5, 5.5), Source::Mouse, None, None);
        assert!(!s.finish(None, |p| p.x > 5.0));
        assert_eq!(s.outbox.len(), 1);
    }

    #[test]
    fn a_new_down_replaces_an_open_stroke() {
        let mut s = Strokes::default();
        s.begin(Vec2::ZERO, Source::Pen, Some(5), None);
        s.begin(Vec2::ONE, Source::Pen, Some(6), None);
        assert_eq!(s.ink_to_delete, vec![5]);
        assert_eq!(s.active.as_ref().unwrap().ink, Some(6));
    }
}
