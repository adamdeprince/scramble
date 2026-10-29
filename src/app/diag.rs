//! Measurements for the diagnostics overlay and `--bench`.
//!
//! Clocks are never mixed. Times measured by the game (simulation cost,
//! building and writing a frame, from a key arriving to its frame being
//! written) are on our monotonic clock. Times reported by the terminal (when
//! a frame was handed to the display, when a pen sample happened) are on
//! the terminal's clock, and only ever subtracted from each other. Neither
//! is the moment light leaves the screen, which no protocol here reports.

use std::collections::VecDeque;
use std::time::Duration;

/// A moving window of samples. The window length is the definition of the
/// statistic ("the last N frames"), not a capacity limit: older samples are
/// meant to fall out.
#[derive(Clone, Debug)]
pub struct Rolling {
    samples: VecDeque<f64>,
    window: usize,
    pub count: u64,
    pub max_ever: f64,
}

impl Rolling {
    pub fn new(window: usize) -> Self {
        Rolling {
            samples: VecDeque::with_capacity(window),
            window: window.max(1),
            count: 0,
            max_ever: 0.0,
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.samples.len() == self.window {
            self.samples.pop_front();
        }
        self.samples.push_back(v);
        self.count += 1;
        self.max_ever = self.max_ever.max(v);
    }

    pub fn push_dur(&mut self, d: Duration) {
        self.push(d.as_secs_f64() * 1000.0);
    }

    pub fn mean(&self) -> f64 {
        if self.samples.is_empty() {
            0.0
        } else {
            self.samples.iter().sum::<f64>() / self.samples.len() as f64
        }
    }

    pub fn max(&self) -> f64 {
        self.samples.iter().copied().fold(0.0, f64::max)
    }

    pub fn percentile(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut v: Vec<f64> = self.samples.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let i = ((v.len() - 1) as f64 * p).round() as usize;
        v[i]
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Metrics {
    /// Simulation cost per tick (ms, our clock).
    pub tick: Rolling,
    pub flock: Rolling,
    pub ai: Rolling,
    /// Building and encoding a frame (ms, our clock).
    pub build: Rolling,
    /// Time for write() and flush() to return (ms, our clock): this is when
    /// the bytes left the game, not when the terminal drew them.
    pub write: Rolling,
    pub frame_bytes: Rolling,
    /// Interval between frame presentation reports (ms, terminal clock).
    pub present_interval: Rolling,
    /// From finishing a write to receiving the terminal's report for that
    /// frame (ms, our clock; includes the trip back over any link).
    pub write_to_report: Rolling,
    /// From a pen sample to the presentation of the frame that drew our
    /// response (ms, terminal clock).
    pub pen_to_present: Rolling,
    /// From a key arriving to the frame showing its effect being written
    /// (ms, our clock).
    pub key_to_write: Rolling,
    /// Round trip of a query to the terminal and back (ms, our clock).
    pub link_rtt: Rolling,
    pub reports: u64,
    pub report_timeouts: u64,
    pub frames: u64,
    pub period_us: u64,
}

impl Default for Metrics {
    fn default() -> Self {
        let w = 240;
        Metrics {
            tick: Rolling::new(w),
            flock: Rolling::new(w),
            ai: Rolling::new(w),
            build: Rolling::new(w),
            write: Rolling::new(w),
            frame_bytes: Rolling::new(w),
            present_interval: Rolling::new(w),
            write_to_report: Rolling::new(w),
            pen_to_present: Rolling::new(w),
            key_to_write: Rolling::new(w),
            link_rtt: Rolling::new(16),
            reports: 0,
            report_timeouts: 0,
            frames: 0,
            period_us: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_statistics() {
        let mut r = Rolling::new(4);
        for v in [1.0, 2.0, 3.0, 4.0, 10.0] {
            r.push(v);
        }
        assert_eq!(r.mean(), (2.0 + 3.0 + 4.0 + 10.0) / 4.0);
        assert_eq!(r.max(), 10.0);
        assert_eq!(r.count, 5);
        assert_eq!(r.percentile(0.0), 2.0);
        assert_eq!(r.max_ever, 10.0);
    }
}
