//! Frame pacing.
//!
//! Without presentation reports, frames go out on a timer at the display
//! period. With Silkie's frame presentation protocol the terminal reports
//! each frame it hands to the display that contains new output.
//!
//! A report says that *a* frame with our output was presented, not which
//! write it held: a terminal presents whatever has arrived at each refresh,
//! so writes coalesce, and across a network link the report for a frame
//! comes back while newer frames are already on their way. So the link's
//! round trip is measured separately, with an OSC 777 query whose reply
//! does not wait for the display, and pacing works in one of two ways:
//!
//! * Strict, when the round trip is shorter than a display period (a local
//!   terminal): write a frame, wait for the next report (or a timeout,
//!   since a hidden window presents nothing), write the next. The game draws
//!   exactly as fast as the terminal presents, and the time from a write to
//!   its report is a meaningful measurement.
//! * Pipelined, over a slower link: write at the display rate, keeping no
//!   more frames on the way than one round trip holds, and stop writing when
//!   reports stop coming (the link is congested or the window hidden), so
//!   frames never pile up in the link.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the round trip is measured.
const PROBE_EVERY: Duration = Duration::from_secs(2);
/// The round trip estimate is the lowest seen over this many probes.
const RTT_SAMPLES: usize = 5;

#[derive(Debug)]
pub struct Pacer {
    pub period: Duration,
    pub feedback: bool,
    /// Recent write times, for the pipelined window.
    writes: VecDeque<Instant>,
    /// Strict mode: the frame waiting for its report.
    waiting: Option<Instant>,
    last_report: Option<Instant>,
    rtts: VecDeque<Duration>,
    probe_sent: Option<Instant>,
    next_probe: Instant,
    next: Instant,
    pub timeouts: u64,
    /// How often the terminal actually presents our frames (terminal
    /// clock), which can be slower than the display's refresh period.
    presented: Option<Duration>,
}

impl Pacer {
    pub fn new(period: Duration, feedback: bool, now: Instant) -> Self {
        Pacer {
            period,
            feedback,
            writes: VecDeque::new(),
            waiting: None,
            last_report: None,
            rtts: VecDeque::new(),
            probe_sent: None,
            next_probe: now,
            next: now,
            timeouts: 0,
            presented: None,
        }
    }

    /// The interval at which frames are worth drawing: the display period,
    /// or the terminal's observed presentation interval when that is
    /// slower (a terminal that redraws every other refresh).
    pub fn interval(&self) -> Duration {
        match self.presented {
            Some(p) if p > self.period => p,
            _ => self.period,
        }
    }

    /// The mean interval between presentation reports, as the terminal
    /// measured it.
    pub fn observe_presentation(&mut self, interval: Duration) {
        self.presented = Some(interval);
    }

    /// The link's round trip (lowest of recent probes), on our clock.
    pub fn rtt(&self) -> Option<Duration> {
        self.rtts.iter().min().copied()
    }

    pub fn strict(&self) -> bool {
        self.rtt().is_none_or(|r| r < self.period)
    }

    fn window(&self) -> Duration {
        self.rtt().unwrap_or_default() + self.period.mul_f32(1.5)
    }

    /// Frames allowed on the way at once in pipelined mode.
    pub fn max_in_flight(&self) -> usize {
        if self.strict() {
            1
        } else {
            (self.window().as_secs_f64() / self.period.as_secs_f64().max(1e-4)).ceil() as usize
        }
    }

    pub fn in_flight(&self, now: Instant) -> usize {
        if self.strict() {
            usize::from(self.waiting.is_some())
        } else {
            let w = self.window();
            self.writes.iter().filter(|&&t| now.duration_since(t) < w).count()
        }
    }

    fn timeout(&self) -> Duration {
        self.rtt().unwrap_or_default() + self.period * 3
    }

    /// When the next frame may be drawn.
    pub fn due_at(&self, now: Instant) -> Instant {
        if !self.feedback {
            return self.next;
        }
        if self.strict() {
            return match self.waiting {
                Some(sent) => (sent + self.timeout()).max(self.next),
                None => self.next,
            };
        }
        let w = self.window();
        let mut due = self.next;
        let recent: Vec<Instant> = self.writes.iter().copied().filter(|&t| now.duration_since(t) < w).collect();
        let max = self.max_in_flight();
        if recent.len() >= max {
            // Wait until enough of them have left the window to be under
            // the limit again.
            due = due.max(recent[recent.len() - max] + w);
        }
        // Reports have stopped while frames are on the way: hold off until
        // a report or the timeout, rather than flooding the link.
        if let (Some(last_write), Some(last_report)) = (self.writes.back(), self.last_report) {
            if *last_write > last_report && now.duration_since(last_report) > self.timeout() {
                due = due.max(*last_write + self.timeout());
            }
        }
        due
    }

    /// Gives up on a strict frame whose report is overdue.
    pub fn expire(&mut self, now: Instant) {
        if let Some(sent) = self.waiting {
            if now.duration_since(sent) >= self.timeout() {
                self.waiting = None;
                self.timeouts += 1;
            }
        }
        let keep = self.window() * 2;
        while self.writes.front().is_some_and(|&t| now.duration_since(t) > keep) {
            self.writes.pop_front();
        }
    }

    pub fn written(&mut self, now: Instant) {
        if self.feedback {
            self.writes.push_back(now);
            if self.strict() {
                self.waiting = Some(now);
            }
        }
        // In strict mode the report gates the next frame, so the minimum
        // gap stays short and a terminal that can present faster gets to.
        let gap = if self.feedback && self.strict() { self.period.mul_f32(0.5) } else { self.interval().mul_f32(0.9) };
        self.next = now + gap;
    }

    /// Nothing was written; look again after a period.
    pub fn idle(&mut self, now: Instant) {
        self.next = now + self.interval().mul_f32(0.9);
    }

    /// A presentation report arrived. In strict mode, returns the time from
    /// writing the frame to its report (our clock).
    pub fn report(&mut self, now: Instant) -> Option<Duration> {
        self.last_report = Some(now);
        self.waiting.take().map(|sent| now.duration_since(sent))
    }

    /// Whether to send a round-trip probe now.
    pub fn probe_due(&self, now: Instant) -> bool {
        self.feedback && self.probe_sent.is_none() && now >= self.next_probe
    }

    pub fn probe_sent(&mut self, now: Instant) {
        self.probe_sent = Some(now);
        self.next_probe = now + PROBE_EVERY;
    }

    /// The probe's reply arrived.
    pub fn probe_reply(&mut self, now: Instant) -> Option<Duration> {
        let sent = self.probe_sent.take()?;
        let d = now.duration_since(sent);
        if self.rtts.len() == RTT_SAMPLES {
            self.rtts.pop_front();
        }
        self.rtts.push_back(d);
        Some(d)
    }

    /// Forget frames in flight (after a suspend or a mode change).
    pub fn reset(&mut self) {
        self.waiting = None;
        self.writes.clear();
        self.probe_sent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Duration = Duration::from_micros(16_667);

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn local_terminal_waits_for_each_report() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, true, t0);
        p.probe_sent(t0);
        p.probe_reply(t0 + ms(1));
        assert!(p.strict());
        assert_eq!(p.due_at(t0), t0);
        p.written(t0);
        assert!(p.due_at(t0 + ms(5)) > t0 + P, "waits for the report");
        assert_eq!(p.report(t0 + ms(12)), Some(ms(12)));
        assert_eq!(p.in_flight(t0 + ms(12)), 0);
    }

    #[test]
    fn long_round_trips_pipeline_frames() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, true, t0);
        p.probe_sent(t0);
        p.probe_reply(t0 + ms(80));
        assert!(!p.strict());
        // 80 ms plus a period and a half, in 16.7 ms frames.
        assert_eq!(p.max_in_flight(), 7);
        let mut t = t0 + ms(80);
        p.report(t);
        for _ in 0..12 {
            assert!(p.due_at(t) <= t + P, "frames flow at the display rate");
            p.written(t);
            p.report(t);
            t += P;
        }
        // A burst (frames written back to back, as after a stall) is held
        // to a round trip's worth.
        let b = t;
        for i in 0..7 {
            p.written(b + ms(i));
        }
        assert!(p.due_at(b + ms(7)) > b + ms(7) + P, "no more than a round trip of frames on the way");
    }

    #[test]
    fn stalled_reports_stop_the_flow() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, true, t0);
        p.probe_sent(t0);
        p.probe_reply(t0 + ms(80));
        p.report(t0 + ms(80));
        p.written(t0 + ms(90));
        // No report for a long time: the next frame waits for the timeout.
        let later = t0 + ms(300);
        assert!(p.due_at(later) > t0 + ms(90) + ms(80));
    }

    #[test]
    fn paces_to_what_the_terminal_presents() {
        let t0 = Instant::now();
        let mut p = Pacer::new(Duration::from_micros(8_333), true, t0);
        p.probe_sent(t0);
        p.probe_reply(t0 + ms(40));
        p.observe_presentation(Duration::from_micros(16_700));
        p.written(t0);
        p.report(t0);
        assert!(p.due_at(t0) >= t0 + ms(15), "pipelined frames follow the presentation rate");
    }

    #[test]
    fn lost_reports_time_out() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, true, t0);
        p.written(t0);
        p.expire(t0 + P * 4);
        assert_eq!(p.in_flight(t0 + P * 4), 0);
        assert_eq!(p.timeouts, 1);
    }

    #[test]
    fn timers_without_feedback() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, false, t0);
        p.written(t0);
        assert!(p.due_at(t0) >= t0 + P.mul_f32(0.89));
        assert!(!p.probe_due(t0));
    }

    #[test]
    fn round_trip_is_the_lowest_recent_probe() {
        let t0 = Instant::now();
        let mut p = Pacer::new(P, true, t0);
        for (i, d) in [90u64, 85, 140, 88].iter().enumerate() {
            let at = t0 + PROBE_EVERY * i as u32;
            assert!(p.probe_due(at));
            p.probe_sent(at);
            p.probe_reply(at + ms(*d));
        }
        assert_eq!(p.rtt(), Some(ms(85)));
    }
}
