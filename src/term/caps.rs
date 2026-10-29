//! Capability discovery.
//!
//! Every feature is detected by asking the terminal, never by its name or
//! environment variables. Queries go out in two rounds, each ended by a
//! primary device attributes request (DA1), which every terminal answers:
//! replies arrive in order, so once DA1's answer is in, anything not yet
//! answered is unsupported. A round also ends on a timeout, for terminals
//! (or links) that answer nothing at all.
//!
//! Round one asks about the graphics protocol, the temporary-file medium,
//! the kitty keyboard protocol, synchronized output and SGR pixel mouse
//! reports, the cell size, and Silkie's OSC 777 capability listing. Round
//! two asks each OSC 777 selector the game will use for its details (only
//! those the listing names, or all of them when there was no listing), and
//! probes the vector, HDR and wide-gamut image formats.
//!
//! The negotiator is a plain state machine: it produces bytes to write and
//! consumes decoded replies, so it is tested without a terminal.

use super::input::Reply;
use super::kitty_gfx;
use super::osc777::{Frame, req};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const ROUND_TIMEOUT: Duration = Duration::from_millis(2500);

const ID_GFX: u32 = 0x5c00_0001;
const ID_TEMPFILE: u32 = 0x5c00_0002;
const ID_VECTOR: u32 = 0x5c00_0003;
const ID_HDR: u32 = 0x5c00_0004;
const ID_GAMUT: u32 = 0x5c00_0005;

/// OSC 777 selectors the game uses.
pub const SELECTORS: [&str; 7] = ["frame", "latency", "display", "ink", "tablet-pointer", "haptic", "scroll"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayInfo {
    pub kind: String,
    pub color: String,
    pub bits: u32,
    pub gamut: String,
    pub headroom: f64,
    pub scale: f64,
    pub period_us: u64,
    pub ppi: u32,
}

impl DisplayInfo {
    pub fn from_frame(f: &Frame) -> Self {
        DisplayInfo {
            kind: f.get("kind").unwrap_or("unknown").to_string(),
            color: f.get("color").unwrap_or("rgb").to_string(),
            bits: f.get_u64("bits").unwrap_or(0) as u32,
            gamut: f.get("gamut").unwrap_or("unknown").to_string(),
            headroom: f.get_f64("headroom").unwrap_or(1.0),
            scale: f.get_f64("scale").unwrap_or(0.0),
            period_us: f.get_u64("period").unwrap_or(0),
            ppi: f.get_u64("ppi").unwrap_or(0) as u32,
        }
    }
}

/// The capability reply of one OSC 777 selector.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectorCaps {
    pub caps: u64,
    pub mode: u64,
    pub fields: BTreeMap<String, String>,
}

impl SelectorCaps {
    pub fn get_u64(&self, k: &str) -> Option<u64> {
        self.fields.get(k).and_then(|v| v.parse().ok())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub graphics: bool,
    pub temp_file: bool,
    pub kitty_keyboard: Option<u32>,
    pub sync_output: bool,
    pub sgr_pixels: bool,
    pub cell_px: Option<(u32, u32)>,
    pub window_px: Option<(u32, u32)>,
    pub da1: Option<String>,
    /// The `caps` listing: selector to highest version.
    pub listing: Option<BTreeMap<String, u32>>,
    pub listing_media: Vec<String>,
    pub listing_formats: Vec<u32>,
    pub reset_supported: bool,
    pub selectors: BTreeMap<String, SelectorCaps>,
    pub vector: bool,
    pub hdr_images: bool,
    pub wide_gamut_images: bool,
    pub display: Option<DisplayInfo>,
    /// Pointer shapes: the crosshair, and Silkie's image pointers.
    pub pointer_crosshair: bool,
    pub pointer_image: bool,
    /// Graphics errors seen while probing, for diagnostics.
    pub notes: Vec<String>,
    pub timings: Vec<(String, Duration)>,
    pub timed_out: bool,
}

impl Capabilities {
    pub fn selector(&self, name: &str) -> Option<&SelectorCaps> {
        self.selectors.get(name)
    }

    pub fn ink(&self) -> bool {
        self.selector("ink").is_some_and(|s| s.caps & 1 != 0) && self.vector
    }

    pub fn tablet_tools(&self) -> bool {
        self.selector("tablet-pointer").is_some_and(|s| s.caps & 1 != 0)
    }

    pub fn touchscreen(&self) -> bool {
        self.selector("tablet-pointer").is_some_and(|s| s.caps & 2 != 0)
    }

    pub fn frame_feedback(&self) -> bool {
        self.selector("frame").is_some_and(|s| s.caps & 1 != 0)
    }

    pub fn low_latency(&self) -> bool {
        self.selector("latency").is_some_and(|s| s.caps & 1 != 0)
    }

    pub fn haptics(&self) -> bool {
        self.selector("haptic").is_some_and(|s| s.caps & 1 != 0)
    }

    pub fn display_events(&self) -> bool {
        self.selector("display").is_some()
    }

    pub fn is_silkie_like(&self) -> bool {
        self.vector || self.listing.is_some()
    }

    /// A short summary of what is available, for the title screen.
    pub fn summary(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("bitmap sprites", self.graphics),
            ("kitty keyboard", self.kitty_keyboard.is_some()),
            ("vector scenery", self.vector),
            ("terminal-side ink", self.ink()),
            ("pen & tablet", self.tablet_tools()),
            ("frame feedback", self.frame_feedback()),
            ("low latency", self.low_latency()),
            ("haptics", self.haptics()),
            ("HDR", self.hdr_images && self.display.as_ref().is_some_and(|d| d.headroom > 1.0)),
            ("wide gamut", self.wide_gamut_images && self.display.as_ref().is_some_and(|d| d.gamut != "srgb" && d.gamut != "unknown")),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Round1,
    Round2,
    Done,
}

pub struct Negotiator {
    phase: Phase,
    started: Instant,
    round_started: Instant,
    pub caps: Capabilities,
    tempfile: Option<PathBuf>,
    caps_query_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    /// Keep feeding replies; give up waiting at this time.
    Waiting(Instant),
    Done,
}

impl Negotiator {
    pub fn new(now: Instant) -> Self {
        Negotiator {
            phase: Phase::Idle,
            started: now,
            round_started: now,
            caps: Capabilities::default(),
            tempfile: None,
            caps_query_id: 7,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// The first round of queries.
    pub fn start(&mut self, now: Instant, allow_tempfile: bool) -> Vec<u8> {
        self.phase = Phase::Round1;
        self.started = now;
        self.round_started = now;
        let mut out = Vec::new();
        kitty_gfx::query_rgb_probe(&mut out, ID_GFX, "");
        if allow_tempfile {
            match kitty_gfx::query_tempfile_probe(&mut out, ID_TEMPFILE) {
                Ok(p) => self.tempfile = Some(p),
                Err(e) => self.caps.notes.push(format!("temp file probe not written: {e}")),
            }
        }
        out.extend_from_slice(b"\x1b[?u");
        out.extend_from_slice(b"\x1b[?2026$p");
        out.extend_from_slice(b"\x1b[?1016$p");
        out.extend_from_slice(b"\x1b[16t");
        out.extend_from_slice(b"\x1b[14t");
        if let Ok(f) = req::query("caps", self.caps_query_id).encode() {
            out.extend_from_slice(&f);
        }
        out.extend_from_slice(b"\x1b[c");
        out
    }

    fn round2(&mut self, now: Instant) -> Vec<u8> {
        self.phase = Phase::Round2;
        self.caps
            .timings
            .push(("round 1".into(), now.saturating_duration_since(self.round_started)));
        self.round_started = now;
        let mut out = Vec::new();
        let wanted: Vec<&str> = match &self.caps.listing {
            Some(l) => SELECTORS.iter().copied().filter(|s| l.contains_key(*s)).collect(),
            None => SELECTORS.to_vec(),
        };
        for (i, s) in wanted.iter().enumerate() {
            if let Ok(f) = req::query(s, 100 + i as u32).encode() {
                out.extend_from_slice(&f);
            }
        }
        let formats_known = self.caps.listing.is_some();
        let fmt_listed = |f: u32| !formats_known || self.caps.listing_formats.contains(&f);
        if self.caps.graphics {
            if fmt_listed(200) {
                kitty_gfx::query_vector_probe(&mut out, ID_VECTOR);
            }
            if fmt_listed(64) {
                kitty_gfx::query_hdr_probe(&mut out, ID_HDR);
            }
            // The gamut key may make a terminal that does not know it drop
            // the command, which is what a query is for.
            kitty_gfx::query_rgb_probe(&mut out, ID_GAMUT, ",G=1");
        }
        out.extend_from_slice(b"\x1b]22;?crosshair,image:1:0:0\x1b\\");
        out.extend_from_slice(b"\x1b[c");
        out
    }

    fn finish(&mut self, now: Instant) {
        if self.phase != Phase::Done {
            self.caps
                .timings
                .push(("round 2".into(), now.saturating_duration_since(self.round_started)));
            self.caps
                .timings
                .push(("total".into(), now.saturating_duration_since(self.started)));
        }
        self.phase = Phase::Done;
        if let Some(p) = self.tempfile.take() {
            // The terminal deletes the probe file when it reads it; when it
            // could not (a remote terminal) we clean up ourselves.
            let _ = std::fs::remove_file(p);
        }
    }

    /// Feeds one reply. Returns bytes to write when a new round starts.
    pub fn on_reply(&mut self, r: &Reply, now: Instant) -> Option<Vec<u8>> {
        match r {
            Reply::Graphics { id, message, .. } => {
                let ok = message == "OK";
                match *id {
                    ID_GFX => self.caps.graphics = true,
                    ID_TEMPFILE => self.caps.temp_file = ok,
                    ID_VECTOR => self.caps.vector = ok,
                    ID_HDR => self.caps.hdr_images = ok,
                    ID_GAMUT => self.caps.wide_gamut_images = ok,
                    _ => {}
                }
                if !ok && *id != ID_TEMPFILE {
                    self.caps.notes.push(format!("graphics probe {id:#x}: {message}"));
                }
            }
            Reply::KeyboardFlags(f) => self.caps.kitty_keyboard = Some(*f),
            Reply::Mode { mode, value } => {
                // 1 set, 2 reset, 3 permanently set: all mean "known".
                let known = matches!(value, 1..=3);
                match mode {
                    2026 => self.caps.sync_output = known,
                    1016 => self.caps.sgr_pixels = known,
                    _ => {}
                }
            }
            Reply::CellSize { width, height } if *width > 0 && *height > 0 => {
                self.caps.cell_px = Some((*width, *height));
            }
            Reply::WindowPixels { width, height } if *width > 0 && *height > 0 => {
                self.caps.window_px = Some((*width, *height));
            }
            Reply::Osc777(f) => self.on_frame(f),
            Reply::Pointer(answer) => {
                let mut it = answer.split(',');
                self.caps.pointer_crosshair = it.next() == Some("1");
                self.caps.pointer_image = it.next() == Some("1");
            }
            Reply::DeviceAttributes(s) => {
                self.caps.da1.get_or_insert_with(|| s.clone());
                match self.phase {
                    Phase::Round1 => return Some(self.round2(now)),
                    Phase::Round2 => self.finish(now),
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    fn on_frame(&mut self, f: &Frame) {
        if f.version() != Some(1) {
            return;
        }
        match (f.selector.as_str(), f.op()) {
            ("caps", Some("cap")) => {
                let mut listing = BTreeMap::new();
                for item in f.get("ext").unwrap_or("").split(',') {
                    if let Some((name, v)) = item.rsplit_once('.') {
                        listing.insert(name.to_string(), v.parse().unwrap_or(1));
                    }
                }
                self.caps.listing = Some(listing);
                self.caps.listing_media = f.get("media").unwrap_or("").split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                self.caps.listing_formats = f.get("format").unwrap_or("").split(',').filter_map(|s| s.parse().ok()).collect();
                self.caps.reset_supported = f.get_u64("caps").unwrap_or(0) & 2 != 0;
            }
            (sel, Some("cap")) => {
                let mut fields = BTreeMap::new();
                for (k, v) in &f.fields {
                    fields.insert(k.clone(), v.clone());
                }
                if sel == "display" {
                    self.caps.display = Some(DisplayInfo::from_frame(f));
                }
                self.caps.selectors.insert(
                    sel.to_string(),
                    SelectorCaps {
                        caps: f.get_u64("caps").unwrap_or(if sel == "display" { 1 } else { 0 }),
                        mode: f.get_u64("mode").unwrap_or(0),
                        fields,
                    },
                );
            }
            _ => {}
        }
    }

    /// Call when no reply arrived before the deadline.
    pub fn poll(&mut self, now: Instant) -> (Status, Option<Vec<u8>>) {
        match self.phase {
            Phase::Done => (Status::Done, None),
            Phase::Idle => (Status::Waiting(now + ROUND_TIMEOUT), None),
            Phase::Round1 | Phase::Round2 => {
                let deadline = self.round_started + ROUND_TIMEOUT;
                if now < deadline {
                    return (Status::Waiting(deadline), None);
                }
                self.caps.timed_out = true;
                if self.phase == Phase::Round1 {
                    let out = self.round2(now);
                    (Status::Waiting(now + ROUND_TIMEOUT), Some(out))
                } else {
                    self.finish(now);
                    (Status::Done, None)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::input::{Decoder, InputEvent};

    fn feed(n: &mut Negotiator, bytes: &[u8], now: Instant) -> Vec<Vec<u8>> {
        let mut d = Decoder::new();
        d.feed(bytes);
        let mut out = Vec::new();
        for ev in d.drain() {
            if let InputEvent::Reply(r) = ev {
                if let Some(b) = n.on_reply(&r, now) {
                    out.push(b);
                }
            }
        }
        out
    }

    #[test]
    fn plain_kitty() {
        let t = Instant::now();
        let mut n = Negotiator::new(t);
        let q = n.start(t, false);
        assert!(q.ends_with(b"\x1b[c"));
        let r2 = feed(
            &mut n,
            b"\x1b_Gi=1543503873;OK\x1b\\\x1b[?0u\x1b[?2026;2$y\x1b[?1016;2$y\x1b[6;36;18t\x1b[4;1800;3600t\x1b[?62;c",
            t,
        );
        assert_eq!(r2.len(), 1, "round two starts on DA1");
        // Without a caps listing, every selector is probed individually.
        let round2 = String::from_utf8_lossy(&r2[0]).to_string();
        for s in SELECTORS {
            assert!(round2.contains(&format!("777;{s};")), "{s} probed");
        }
        // Kitty ignores the OSC 777 queries and rejects f=200 and f=64.
        feed(
            &mut n,
            b"\x1b_Gi=1543503875;EINVAL:unknown format\x1b\\\x1b_Gi=1543503876;EINVAL:unknown format\x1b\\\x1b_Gi=1543503877;OK\x1b\\\x1b[?62;c",
            t,
        );
        assert!(n.is_done());
        let c = &n.caps;
        assert!(c.graphics && c.sync_output && c.sgr_pixels);
        assert_eq!(c.kitty_keyboard, Some(0));
        assert_eq!(c.cell_px, Some((18, 36)));
        assert!(!c.vector && !c.ink() && !c.frame_feedback() && !c.is_silkie_like());
    }

    #[test]
    fn silkie_with_listing() {
        let t = Instant::now();
        let mut n = Negotiator::new(t);
        n.start(t, false);
        let r2 = feed(
            &mut n,
            b"\x1b_Gi=1543503873;OK\x1b\\\x1b[?0u\x1b]777;caps;v=1:op=cap:id=7:caps=3:ext=caps.1,frame.1,ink.1,tablet-pointer.1,display.1,latency.1,haptic.1:media=d,f,t,s:format=24,32,100,64,200\x1b\\\x1b[?62;c",
            t,
        );
        let round2 = String::from_utf8_lossy(&r2[0]).to_string();
        assert!(round2.contains("777;ink;"));
        assert!(!round2.contains("777;scroll;"), "only listed selectors are probed");
        assert!(round2.contains("f=200"));
        feed(
            &mut n,
            concat!(
                "\x1b]777;frame;v=1:op=cap:id=100:caps=1:mode=0:period=8333\x1b\\",
                "\x1b]777;latency;v=1:op=cap:id=101:caps=1:mode=0:input=3000:repaint=10000\x1b\\",
                "\x1b]777;display;v=1:op=cap:id=102:mode=0:kind=lcd:color=rgb:bits=8:gamut=p3:headroom=1:scale=2:period=8333:ppi=254\x1b\\",
                "\x1b]777;ink;v=1:op=cap:id=103:caps=1:mode=0\x1b\\",
                "\x1b]777;tablet-pointer;v=1:op=cap:id=104:caps=101:mode=0:w=3600:h=1800:cw=18:ch=36\x1b\\",
                "\x1b]777;haptic;v=1:op=cap:id=105:caps=1\x1b\\",
                "\x1b_Gi=1543503875;OK\x1b\\\x1b_Gi=1543503876;OK\x1b\\\x1b_Gi=1543503877;OK\x1b\\\x1b[?62;c"
            )
            .as_bytes(),
            t,
        );
        let c = &n.caps;
        assert!(n.is_done());
        assert!(c.vector && c.ink() && c.tablet_tools() && c.frame_feedback() && c.low_latency() && c.haptics());
        assert!(c.reset_supported);
        assert_eq!(c.display.as_ref().unwrap().period_us, 8333);
        assert_eq!(c.display.as_ref().unwrap().gamut, "p3");
        assert!(c.wide_gamut_images && c.hdr_images);
    }

    #[test]
    fn silent_terminal_times_out() {
        let t = Instant::now();
        let mut n = Negotiator::new(t);
        n.start(t, false);
        let (s, out) = n.poll(t + Duration::from_millis(100));
        assert!(matches!(s, Status::Waiting(_)) && out.is_none());
        let (s, out) = n.poll(t + ROUND_TIMEOUT + Duration::from_millis(1));
        assert!(matches!(s, Status::Waiting(_)));
        assert!(out.is_some(), "round two is still attempted");
        let (s, _) = n.poll(t + ROUND_TIMEOUT * 2 + Duration::from_millis(2));
        assert_eq!(s, Status::Done);
        assert!(n.caps.timed_out && !n.caps.graphics);
    }

    #[test]
    fn unknown_versions_are_ignored() {
        let t = Instant::now();
        let mut n = Negotiator::new(t);
        n.start(t, false);
        feed(&mut n, b"\x1b]777;caps;v=2:op=cap:id=7:caps=3:ext=caps.2\x1b\\", t);
        assert!(n.caps.listing.is_none());
    }
}
