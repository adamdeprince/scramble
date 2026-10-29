//! The game application: start-up and capability discovery, the main loop
//! (fixed-step simulation, paced rendering, input), screens, and cleanup.

pub mod control;
pub mod diag;
pub mod pacer;
pub mod scores;
pub mod screens;
pub mod show;
pub mod strokes;

use crate::render::layout::{self, Layout};
use crate::render::text;
use crate::render::upload::Uploader;
use crate::render::{FrameModel, IMG_GROUND, RenderConfig, Renderer, SceneRef, SpriteSource};
use crate::sim::autopilot::Autopilot;
use crate::sim::replay::Recording;
use crate::sim::rules::DT;
use crate::sim::{Game, Outcome, Phase, Session, SimEvent, TickInput};
use crate::term::caps::{Capabilities, Negotiator, Status};
use crate::term::input::{Decoder, InputEvent, Reply};
use crate::term::osc777::{Frame, req};
use crate::term::tty::{self, Msg, Tty};
use crate::world::Dir;
use crate::world::level::{self, Level};
use diag::Metrics;
use scores::Scores;
use show::Show;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use strokes::Strokes;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub seed: Option<u64>,
    pub level: usize,
    pub diagnostics: bool,
    /// Use only what plain kitty offers, even on Silkie.
    pub kitty_only: bool,
    pub no_vector: bool,
    pub no_ink: bool,
    pub no_latency: bool,
    pub no_frame_feedback: bool,
    pub no_haptics: bool,
    pub no_hdr: bool,
    pub no_tempfile: bool,
    pub record: Option<PathBuf>,
    pub report: bool,
    /// Let the autopilot play (a demo, and a way to watch the game).
    pub autoplay: bool,
    /// Load the PNG sprite sheets instead of painting them.
    pub png_sprites: bool,
}

pub enum Screen {
    Title { demo: Box<Game>, bot: Autopilot },
    Play,
    Paused { menu: usize },
    GameOver { name: String, rank: Option<usize> },
    Report,
}

/// What the game has turned on in the terminal beyond the basics.
#[derive(Clone, Debug, Default)]
pub struct Features {
    pub vector: bool,
    pub ink: bool,
    pub frame_feedback: bool,
    pub latency: bool,
    pub haptics: bool,
    pub tablet_mode: u32,
    pub display_events: bool,
    pub ink_brush_sent: bool,
    /// Modes read back from the terminal after asking: selector to mode.
    pub granted: std::collections::BTreeMap<String, u64>,
}

pub struct App {
    pub tty: Tty,
    rx: Receiver<Msg>,
    pub decoder: Decoder,
    pub caps: Capabilities,
    pub features: Features,
    pub renderer: Renderer,
    pub show: Show,
    pub screen: Screen,
    pub game: Option<Game>,
    pub level: Arc<Level>,
    pub level_key: String,
    pub round: u32,
    pub opts: Options,
    pub scores: Scores,
    pub strokes: Strokes,
    pub want_dir: Option<Dir>,
    pub action: bool,
    pub metrics: Metrics,
    pub show_diag: bool,
    pub quit: bool,
    recording: Option<Recording>,
    last_tick: Instant,
    pub pacer: pacer::Pacer,
    pending_resize: Option<Instant>,
    last_input_at: Instant,
    key_pending: Option<Instant>,
    pen_pending: Option<u64>,
    pen_in_flight: Option<u64>,
    last_haptic: Instant,
    last_report_t: Option<f64>,
    pub latency_on: bool,
    pub cell_px: (u32, u32),
    seed: u64,
    /// The diagnostics panel is refreshed a few times a second, so that
    /// rewriting it does not dominate what it measures.
    diag_cache: Option<(Instant, crate::render::text::Overlay)>,
    autopilot: Autopilot,
    /// The window size the "too small" message was drawn for.
    too_small: Option<(u32, u32)>,
}

const DEFAULT_PERIOD: Duration = Duration::from_micros(16_667);

pub fn run(opts: Options) -> anyhow::Result<()> {
    let mut tty = Tty::open()?;
    tty.enter_screen()?;
    let (tx, rx) = mpsc::channel();
    tty::spawn_input_thread(tx.clone());
    tty::spawn_signal_thread(tx)?;
    let mut decoder = Decoder::new();
    let (caps, stash) = negotiate(&mut tty, &rx, &mut decoder, !opts.no_tempfile)?;
    if !caps.graphics {
        tty.restore();
        anyhow::bail!(
            "this terminal did not answer the kitty graphics protocol query; Silkie Scramble needs kitty, Silkie, or another terminal with that protocol"
        );
    }
    if opts.report {
        tty.restore();
        println!("{}", screens::capability_report(&caps).join("\n"));
        return Ok(());
    }
    let mut app = App::new(tty, rx, decoder, caps, opts)?;
    app.setup_terminal()?;
    for ev in stash {
        app.on_input_event(ev);
    }
    let result = app.main_loop();
    app.shutdown();
    result
}

/// Sends the capability queries and collects the answers. Input that is not
/// a reply (an impatient key press) is kept for the game.
fn negotiate(tty: &mut Tty, rx: &Receiver<Msg>, decoder: &mut Decoder, tempfile: bool) -> anyhow::Result<(Capabilities, Vec<InputEvent>)> {
    let now = Instant::now();
    let mut n = Negotiator::new(now);
    tty.write(&n.start(now, tempfile))?;
    let mut stash = Vec::new();
    loop {
        let (status, extra) = n.poll(Instant::now());
        if let Some(b) = extra {
            tty.write(&b)?;
        }
        let deadline = match status {
            Status::Done => break,
            Status::Waiting(d) => d,
        };
        let wait = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Msg::Input(bytes)) => {
                decoder.feed(&bytes);
                for ev in decoder.drain() {
                    match ev {
                        InputEvent::Reply(r) => {
                            if let Some(b) = n.on_reply(&r, Instant::now()) {
                                tty.write(&b)?;
                            }
                        }
                        other => stash.push(other),
                    }
                }
            }
            Ok(Msg::Signal(s)) if s == libc::SIGINT || s == libc::SIGTERM || s == libc::SIGHUP => {
                anyhow::bail!("interrupted during start-up");
            }
            Ok(Msg::InputClosed) => anyhow::bail!("the terminal closed its input"),
            Ok(Msg::Signal(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!("input thread stopped"),
        }
    }
    Ok((n.caps, stash))
}

impl App {
    fn new(tty: Tty, rx: Receiver<Msg>, decoder: Decoder, caps: Capabilities, opts: Options) -> anyhow::Result<App> {
        let silkie = !opts.kitty_only;
        let features = Features {
            vector: silkie && caps.vector && !opts.no_vector,
            ink: silkie && caps.ink() && caps.tablet_tools() && !opts.no_ink && !opts.no_vector,
            frame_feedback: silkie && caps.frame_feedback() && !opts.no_frame_feedback,
            latency: silkie && caps.low_latency() && !opts.no_latency,
            haptics: silkie && caps.haptics() && !opts.no_haptics,
            tablet_mode: if silkie {
                u32::from(caps.tablet_tools()) | if caps.touchscreen() { 2 } else { 0 }
            } else {
                0
            },
            display_events: silkie && caps.display_events(),
            ink_brush_sent: false,
            granted: Default::default(),
        };
        let p3 = silkie
            && caps.wide_gamut_images
            && caps.display.as_ref().is_some_and(|d| d.gamut == "p3" || d.gamut == "rec2020");
        let renderer = Renderer::new(RenderConfig {
            vector_scenery: features.vector,
            uploader: Uploader {
                temp_file: caps.temp_file && !opts.no_tempfile,
                gamut_p3: p3,
            },
            sync_output: caps.sync_output,
            cell_alpha: silkie && caps.listing.is_some(),
            hdr_boost: if silkie && caps.hdr_images && !opts.no_hdr { HDR_GLINT } else { 1.0 },
            sprites: if opts.png_sprites { SpriteSource::Files } else { SpriteSource::Painted },
        })?;
        let period = caps
            .display
            .as_ref()
            .filter(|d| d.period_us > 0 && silkie)
            .map(|d| Duration::from_micros(d.period_us))
            .unwrap_or(DEFAULT_PERIOD);
        let seed = opts.seed.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
        });
        let level = Arc::new(level::builtin(0).expect("level one ships with the game"));
        let features_frame = features.frame_feedback;
        let mut decoder = decoder;
        decoder.sgr_pixels = caps.sgr_pixels;
        let cell = caps.cell_px.unwrap_or((10, 20));
        decoder.cell_px = cell;
        let mut app = App {
            tty,
            rx,
            decoder,
            caps,
            features,
            renderer,
            show: Show::new(),
            screen: Screen::Report,
            game: None,
            level,
            level_key: String::new(),
            round: 0,
            opts: opts.clone(),
            scores: Scores::load(),
            strokes: Strokes::default(),
            want_dir: None,
            action: false,
            metrics: Metrics::default(),
            show_diag: opts.diagnostics,
            quit: false,
            recording: None,
            last_tick: Instant::now(),
            pacer: pacer::Pacer::new(period, features_frame, Instant::now()),
            pending_resize: None,
            last_input_at: Instant::now(),
            key_pending: None,
            pen_pending: None,
            pen_in_flight: None,
            last_haptic: Instant::now(),
            last_report_t: None,
            latency_on: false,
            cell_px: cell,
            seed,
            diag_cache: None,
            autopilot: Autopilot::new(),
            too_small: None,
        };
        app.apply_display_kind();
        app.go_title();
        app.relayout();
        Ok(app)
    }

    fn osc(&mut self, f: Frame) {
        if let Ok(b) = f.encode() {
            let _ = self.tty.write(&b);
        }
    }

    /// Turns on input reporting and the Silkie modes the game uses, then
    /// asks the terminal to read each mode back (a user may refuse one).
    fn setup_terminal(&mut self) -> anyhow::Result<()> {
        self.tty.enable_input(self.caps.kitty_keyboard.is_some(), self.caps.sgr_pixels)?;
        for id in Renderer::image_ids() {
            self.tty.note_image(id);
        }
        if self.features.frame_feedback {
            self.set_mode("frame", 1);
        }
        if self.features.display_events {
            self.set_mode("display", 1);
        }
        if self.features.tablet_mode != 0 {
            self.set_mode("tablet-pointer", self.features.tablet_mode);
        }
        Ok(())
    }

    fn set_mode(&mut self, selector: &str, mode: u32) {
        self.osc(req::set_mode(selector, mode));
        self.osc(req::read_mode(selector));
        self.tty.note_osc_mode(selector, mode);
    }

    fn send_ink_brush(&mut self) {
        if !self.features.ink || self.features.ink_brush_sent || !self.renderer.has_vector_ground() {
            return;
        }
        let brush = Frame::new("ink", "brush")
            .field("i", IMG_GROUND)
            .field("g", crate::gfx::scenery::G_INK)
            .field("k", crate::gfx::scenery::INK_BASE)
            .field("n", 1_000_000)
            .field("fill", "#7a5230e6")
            .field("w0", 9)
            .field("w1", 17)
            .field("tools", 58);
        self.osc(brush);
        self.osc(req::read_mode("ink"));
        self.tty.note_osc_mode("ink", 1);
        self.features.ink_brush_sent = true;
    }

    /// E-ink: updates are slow and leave ghosts, so draw rarely and skip
    /// the particles.
    fn apply_display_kind(&mut self) {
        let eink = self.caps.display.as_ref().is_some_and(|d| d.kind == "eink");
        self.show.effects = !eink;
        if eink {
            self.pacer.period = Duration::from_millis(250);
        }
    }

    /// The fence-drawing pointer while playing: our own stake image where
    /// the terminal takes image pointers, else a crosshair.
    fn pointer_for_play(&mut self, on: bool) {
        if !on {
            let _ = self.tty.pop_pointer();
            return;
        }
        if self.tty.modes.pointer_pushes > 0 {
            return;
        }
        if self.caps.pointer_image && !self.opts.kitty_only {
            let scale = self.caps.display.as_ref().map_or(1.0, |d| d.scale.max(1.0));
            let px = (26.0 * scale).round() as u32;
            let (bytes, (hx, hy)) = self.renderer.pointer_image(px);
            let _ = self.tty.write(&bytes);
            let _ = self.tty.push_pointer(&format!("image:{}:{hx}:{hy}", crate::render::IMG_POINTER));
        } else if self.caps.pointer_crosshair {
            let _ = self.tty.push_pointer("crosshair");
        }
    }

    pub fn set_low_latency(&mut self, on: bool) {
        // Playing and not playing switch together: the pointer follows.
        self.pointer_for_play(on);
        if !self.features.latency || self.latency_on == on {
            return;
        }
        self.latency_on = on;
        self.osc(req::set_mode("latency", u32::from(on)));
        self.tty.note_osc_mode("latency", u32::from(on));
    }

    pub fn haptic(&mut self, pattern: &str) {
        if !self.features.haptics || self.last_haptic.elapsed() < Duration::from_millis(80) {
            return;
        }
        self.last_haptic = Instant::now();
        self.osc(req::haptic_play(pattern));
    }

    fn shutdown(&mut self) {
        if let Some(rec) = &mut self.recording {
            if let (Some(path), Some(g)) = (&self.opts.record, &self.game) {
                rec.finish(g);
                let _ = rec.save(path);
            }
        }
        let bytes = self.renderer.teardown();
        let _ = self.tty.write(&bytes);
        self.tty.restore();
    }

    /// Recomputes the layout from the terminal's size.
    pub fn relayout(&mut self) {
        let (cols, rows, xpx, ypx) = tty::size();
        if xpx > 0 && ypx > 0 && cols > 0 && rows > 0 {
            self.cell_px = (xpx / cols, ypx / rows);
        }
        self.decoder.cell_px = self.cell_px;
        let l = Layout::new(cols, rows, self.cell_px.0, self.cell_px.1, self.level.grid.width, self.level.grid.height);
        if l != self.renderer.layout {
            // Stray text from the old size goes; images stay and are moved.
            let mut clear = Vec::new();
            for r in 0..rows {
                clear.extend_from_slice(format!("\x1b[{};1H\x1b[0m\x1b[2K", r + 1).as_bytes());
            }
            let _ = self.tty.write(&clear);
            tracing::info!(cols, rows, cell_px = ?self.cell_px, layout = ?l, "layout");
            self.renderer.set_layout(l);
            self.renderer.reset_text();
        }
    }

    pub fn start_game(&mut self) {
        let idx = self.opts.level.min(level::BUILTIN.len() - 1);
        self.round = 0;
        self.load_level(idx, Session::default());
        self.screen = Screen::Play;
        self.set_low_latency(true);
    }

    pub fn load_level(&mut self, index: usize, session: Session) {
        let mut lvl = level::builtin(index).expect("built-in level");
        lvl.cunning = (lvl.cunning + self.round).min(5);
        self.level = Arc::new(lvl.clone());
        self.level_key = format!("{index}:{}", self.round);
        let seed = self.seed ^ ((self.round as u64) << 32);
        let game = Game::new(lvl, index, session, seed);
        if self.opts.record.is_some() {
            self.recording = Some(Recording::new(&game));
        }
        self.game = Some(game);
        self.show.clear();
        self.strokes.clear();
        self.flush_ink_deletes();
        self.want_dir = None;
        self.features.ink_brush_sent = false;
        self.relayout();
    }

    pub fn go_title(&mut self) {
        let lvl = level::builtin(0).expect("level one");
        self.level = Arc::new(lvl.clone());
        self.level_key = "title".into();
        let mut demo = Game::new(lvl, 0, Session::default(), self.seed ^ 0xdead_beef);
        demo.session.lives = 99;
        self.screen = Screen::Title {
            demo: Box::new(demo),
            bot: Autopilot::new(),
        };
        self.game = None;
        self.show.clear();
        self.strokes.clear();
        self.set_low_latency(false);
        self.features.ink_brush_sent = false;
    }

    fn main_loop(&mut self) -> anyhow::Result<()> {
        self.last_tick = Instant::now();
        while !self.quit {
            let now = Instant::now();
            let next_tick = self.last_tick + Duration::from_secs_f32(DT);
            let render_at = self.render_due_at();
            let mut deadline = next_tick.min(render_at);
            if let Some(r) = self.pending_resize {
                deadline = deadline.min(r + Duration::from_millis(60));
            }
            if self.decoder.has_pending_escape() && self.caps.kitty_keyboard.is_none() {
                deadline = deadline.min(self.last_input_at + Duration::from_millis(30));
            }
            let wait = deadline.saturating_duration_since(now);
            match self.rx.recv_timeout(wait) {
                Ok(m) => self.on_msg(m)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(m) = self.rx.try_recv() {
                self.on_msg(m)?;
            }
            let now = Instant::now();
            if self.decoder.has_pending_escape()
                && self.caps.kitty_keyboard.is_none()
                && now.duration_since(self.last_input_at) >= Duration::from_millis(30)
            {
                if let Some(ev) = self.decoder.flush_lone_escape() {
                    self.on_input_event(ev);
                }
            }
            if let Some(r) = self.pending_resize {
                if now.duration_since(r) >= Duration::from_millis(60) {
                    self.pending_resize = None;
                    self.relayout();
                }
            }
            self.advance_simulation(now);
            self.flush_ink_deletes();
            if self.pacer.probe_due(now) {
                // Measure the link: a mode read is answered at once, not at
                // the next presentation.
                self.osc(req::read_mode("frame"));
                self.pacer.probe_sent(now);
            }
            if now >= self.render_due_at() {
                self.render_frame();
            }
        }
        Ok(())
    }

    fn render_due_at(&self) -> Instant {
        self.pacer.due_at(Instant::now())
    }

    fn on_msg(&mut self, m: Msg) -> anyhow::Result<()> {
        match m {
            Msg::Input(bytes) => {
                self.last_input_at = Instant::now();
                self.decoder.feed(&bytes);
                for ev in self.decoder.drain() {
                    self.on_input_event(ev);
                }
            }
            Msg::InputClosed => self.quit = true,
            Msg::Signal(s) => match s {
                libc::SIGWINCH => self.pending_resize = Some(Instant::now()),
                libc::SIGINT | libc::SIGTERM | libc::SIGHUP => self.quit = true,
                libc::SIGTSTP => self.suspend()?,
                _ => {}
            },
        }
        Ok(())
    }

    /// Ctrl+Z or SIGTSTP: hand the terminal back, and set everything up
    /// again when continued.
    pub fn suspend(&mut self) -> anyhow::Result<()> {
        if matches!(self.screen, Screen::Play) {
            self.screen = Screen::Paused { menu: 0 };
        }
        let bytes = self.renderer.teardown();
        let _ = self.tty.write(&bytes);
        self.latency_on = false;
        self.tty.suspend()?;
        self.renderer.invalidate();
        self.features.ink_brush_sent = false;
        self.features.granted.clear();
        self.setup_terminal()?;
        self.relayout();
        self.pacer.reset();
        self.last_tick = Instant::now();
        Ok(())
    }

    pub fn on_input_event(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::Key(k) => self.on_key(k),
            InputEvent::Mouse(m) => self.on_mouse(m),
            InputEvent::Focus(false) => {
                if matches!(self.screen, Screen::Play) {
                    self.screen = Screen::Paused { menu: 0 };
                    self.set_low_latency(false);
                }
            }
            InputEvent::Focus(true) => {}
            InputEvent::Reply(r) => self.on_reply(r),
        }
    }

    fn on_reply(&mut self, r: Reply) {
        match r {
            Reply::Graphics { id, message, .. } => {
                if message != "OK" {
                    self.renderer.on_graphics_error(id, &message);
                }
            }
            Reply::CellSize { width, height } if width > 0 && height > 0 => {
                self.cell_px = (width, height);
                self.pending_resize = Some(Instant::now());
            }
            Reply::Osc777(f) => self.on_osc(f),
            _ => {}
        }
    }

    fn on_osc(&mut self, f: Frame) {
        if f.version() != Some(1) {
            return;
        }
        match (f.selector.as_str(), f.op()) {
            ("frame", Some("event")) => self.on_frame_report(&f),
            ("display", Some("event")) => {
                let d = crate::term::caps::DisplayInfo::from_frame(&f);
                if d.period_us > 0 {
                    self.pacer.period = Duration::from_micros(d.period_us);
                }
                // The HDR glints need nothing: the terminal clips them to
                // whatever headroom it has at the moment.
                self.caps.display = Some(d);
                self.apply_display_kind();
            }
            (sel, Some("mode")) => {
                if sel == "frame" {
                    if let Some(rtt) = self.pacer.probe_reply(Instant::now()) {
                        self.metrics.link_rtt.push_dur(rtt);
                    }
                }
                if let Some(m) = f.get_u64("mode") {
                    self.features.granted.insert(sel.to_string(), m);
                    if sel == "frame" && m == 0 {
                        // Refused: pace by timer instead.
                        self.features.frame_feedback = false;
                        self.pacer.feedback = false;
                        self.pacer.reset();
                    }
                    if sel == "ink" && m == 0 {
                        self.features.ink = false;
                    }
                }
            }
            ("tablet-pointer", Some("event")) => self.on_pointer_report(&f),
            _ => {}
        }
    }

    fn on_frame_report(&mut self, f: &Frame) {
        let now = Instant::now();
        self.metrics.reports += 1;
        if let Some(p) = f.get_u64("period").filter(|&p| p > 0) {
            self.pacer.period = Duration::from_micros(p);
            self.metrics.period_us = p;
        }
        let t = f.get_f64("t").unwrap_or(0.0) + f.get_f64("tu").unwrap_or(0.0) / 1000.0;
        if let Some(prev) = self.last_report_t {
            // Gaps longer than a few periods are pauses, not the rate.
            if t > prev && t - prev < self.pacer.period.as_secs_f64() * 1000.0 * 4.0 {
                self.metrics.present_interval.push(t - prev);
                if self.metrics.present_interval.count >= 30 {
                    let mean = self.metrics.present_interval.mean();
                    self.pacer.observe_presentation(Duration::from_secs_f64(mean / 1000.0));
                }
            }
        }
        self.last_report_t = Some(t);
        if let Some(d) = self.pacer.report(now) {
            self.metrics.write_to_report.push_dur(d);
        }
        if let Some(t0) = self.pen_in_flight.take() {
            if t >= t0 as f64 {
                self.metrics.pen_to_present.push(t - t0 as f64);
            }
        }
    }

    fn advance_simulation(&mut self, now: Instant) {
        let step = Duration::from_secs_f32(DT);
        let mut steps = 0;
        while now.duration_since(self.last_tick) >= step {
            self.last_tick += step;
            steps += 1;
            // After a stall (a suspend, a slow upload) skip ahead instead of
            // running dozens of ticks at once.
            if steps > 8 {
                self.last_tick = now;
                break;
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.show.update(DT);
        match &mut self.screen {
            Screen::Title { demo, bot } => {
                let input = bot.decide(demo);
                demo.step(&input);
                let events = demo.drain_events();
                for e in &events {
                    self.show.on_event(e);
                }
                if demo.outcome().is_some() || demo.phase == Phase::GameOver {
                    let lvl = (*self.level).clone();
                    let mut d = Game::new(lvl, 0, Session::default(), demo.seed.wrapping_add(1));
                    d.session.lives = 99;
                    **demo = d;
                    self.show.clear();
                }
            }
            Screen::Play => self.tick_play(),
            _ => {}
        }
    }

    fn tick_play(&mut self) {
        let Some(game) = &mut self.game else { return };
        let mut input = TickInput {
            dir: self.want_dir,
            action: std::mem::take(&mut self.action),
            strokes: std::mem::take(&mut self.strokes.outbox),
        };
        if self.opts.autoplay {
            let bot = self.autopilot.decide(game);
            input.dir = bot.dir.or(input.dir);
            input.action |= bot.action;
        }
        game.step(&input);
        if let Some(rec) = &mut self.recording {
            rec.push(game.tick, &input);
        }
        self.metrics.tick.push_dur(game.stats.last_tick);
        self.metrics.flock.push_dur(game.stats.last_flock);
        self.metrics.ai.push_dur(game.stats.last_ai + game.stats.last_nav);
        let events = game.drain_events();
        let outcome = game.outcome();
        if matches!(game.phase, Phase::Cleared { .. }) {
            let area = glam::Vec2::new(game.level.grid.width as f32, game.level.grid.height as f32);
            self.show.confetti(area);
        }
        for e in &events {
            self.show.on_event(e);
            match e {
                SimEvent::FenceBuilt { stroke, .. } => {
                    if let Some(id) = stroke {
                        self.strokes.judged(*id);
                    }
                    self.haptic("generic");
                }
                SimEvent::FenceRefused { stroke: Some(id), .. } => self.strokes.judged(*id),
                SimEvent::ChickJoined { .. } => self.haptic("alignment"),
                SimEvent::Emote {
                    entity,
                    emote: crate::sim::Emote::Alert,
                } => {
                    let near = self.game.as_ref().is_some_and(|g| {
                        g.predators.iter().any(|p| {
                            p.id == *entity && p.kind == crate::sim::predator::PredatorKind::Fox && p.mover.pos.distance(g.hen.pos()) < 4.5
                        })
                    });
                    if near {
                        self.show.startle();
                    }
                }
                SimEvent::BatchScored { .. } => self.haptic("complete"),
                SimEvent::GateMoved { .. } if input.action => self.haptic("click"),
                SimEvent::HenCaught { .. } => self.haptic("generic"),
                _ => {}
            }
        }
        self.flush_ink_deletes();
        if let Some(o) = outcome {
            self.on_outcome(o);
        }
    }

    fn on_outcome(&mut self, o: Outcome) {
        let Some(game) = self.game.take() else { return };
        let session = game.session.clone();
        match o {
            Outcome::NextLevel => {
                let mut next = game.level_index + 1;
                if next >= level::BUILTIN.len() {
                    next = 0;
                    self.round += 1;
                }
                self.load_level(next, session);
            }
            Outcome::Retry => self.load_level(game.level_index, session),
            Outcome::GameOver => {
                let rank = self.scores.rank(session.score);
                self.screen = Screen::GameOver {
                    name: String::new(),
                    rank,
                };
                self.set_low_latency(false);
                self.game = Some(game);
            }
        }
    }

    /// Deletes the terminal's ink for strokes that have been judged.
    fn flush_ink_deletes(&mut self) {
        if self.strokes.ink_to_delete.is_empty() {
            return;
        }
        let edits: Vec<crate::gfx::vector::Edit> = self
            .strokes
            .ink_to_delete
            .drain(..)
            .map(crate::gfx::vector::Edit::Delete)
            .collect();
        let bytes = self.renderer.ground_patch(&edits);
        let _ = self.tty.write(&bytes);
    }

    fn alpha(&self) -> f32 {
        (Instant::now().duration_since(self.last_tick).as_secs_f32() / DT).clamp(0.0, 1.0)
    }

    fn render_frame(&mut self) {
        let started = Instant::now();
        if self.renderer.layout.is_none() {
            self.relayout();
        }
        if self.renderer.layout.is_none() {
            // Too small for the garden: hide it and say so (again after
            // every resize), and place it anew when the window grows.
            // The game waits meanwhile.
            if matches!(self.screen, Screen::Play) {
                self.pause();
            }
            let (cols, rows, _, _) = tty::size();
            if self.too_small != Some((cols, rows)) {
                let mut out = if self.too_small.is_none() { self.renderer.hide() } else { Vec::new() };
                self.too_small = Some((cols, rows));
                out.extend_from_slice(b"\x1b[0m\x1b[2J");
                let message = format!(
                    "Silkie Scramble needs a bigger window ({}x{} cells or more)",
                    layout::MIN_COLS,
                    layout::MIN_ROWS
                );
                let lines = text::wrap(&message, cols as usize);
                let top = (rows as usize).saturating_sub(lines.len()) / 2;
                for (i, line) in lines.iter().enumerate() {
                    let col = (cols as usize).saturating_sub(text::width(line)) / 2;
                    out.extend_from_slice(format!("\x1b[{};{}H{line}", top + i + 1, col + 1).as_bytes());
                }
                let _ = self.tty.write(&out);
            }
            self.pacer.idle(Instant::now());
            return;
        }
        if self.too_small.take().is_some() {
            let _ = self.tty.write(b"\x1b[0m\x1b[2J");
            self.renderer.reset_text();
        }
        self.pacer.expire(started);
        let model = self.frame_model();
        if self.renderer.needs_heavy_upload(&model) {
            // Over a slow link the images take a while: say so first.
            let banner = self.renderer.banner("planting the garden…");
            let _ = self.tty.write(&banner);
        }
        let bytes = self.renderer.render(&model);
        self.send_ink_brush();
        let built = started.elapsed();
        if bytes.is_empty() {
            self.pacer.idle(Instant::now());
            return;
        }
        if let Err(e) = self.tty.write(&bytes) {
            tracing::error!("writing a frame: {e}");
            self.quit = true;
            return;
        }
        let written = Instant::now();
        self.metrics.build.push_dur(built);
        self.metrics.write.push_dur(self.tty.stats.last_write);
        self.metrics.frame_bytes.push(bytes.len() as f64);
        self.metrics.frames += 1;
        if let Some(k) = self.key_pending.take() {
            self.metrics.key_to_write.push_dur(written.duration_since(k));
        }
        if let Some(t) = self.pen_pending.take() {
            self.pen_in_flight = Some(t);
        }
        self.pacer.written(written);
        self.metrics.report_timeouts = self.pacer.timeouts;
    }

    fn frame_model(&mut self) -> FrameModel {
        let alpha = self.alpha();
        let mut m = FrameModel::default();
        let scene = SceneRef {
            key: self.level_key.clone(),
            level: self.level.clone(),
        };
        match &self.screen {
            Screen::Title { demo, .. } => {
                m.scene = Some(scene);
                m.gates = Show::gates(demo);
                m.fences = Show::fences(demo);
                let d: &Game = demo;
                m.sprites = self.show.sprites(d, alpha);
                m.title = true;
            }
            Screen::Report => {}
            _ => {
                if let Some(g) = &self.game {
                    m.scene = Some(scene);
                    m.gates = Show::gates(g);
                    m.fences = Show::fences(g);
                    let a = if matches!(self.screen, Screen::Play) { alpha } else { 1.0 };
                    m.sprites = self.show.sprites(g, a);
                    m.strokes = self.strokes.views();
                }
            }
        }
        screens::decorate(self, &mut m);
        if self.show_diag {
            let stale = self.diag_cache.as_ref().is_none_or(|(at, _)| at.elapsed() >= Duration::from_millis(250));
            if stale {
                self.diag_cache = Some((Instant::now(), diag_panel(self)));
            }
            if let Some((_, o)) = &self.diag_cache {
                m.overlays.push(o.clone());
            }
        }
        m
    }

    pub fn note_key(&mut self) {
        self.key_pending.get_or_insert_with(Instant::now);
    }

    pub fn note_pen(&mut self, t: u64) {
        self.pen_pending = Some(t);
    }
}

/// The diagnostics panel: renderer and terminal measurements, each labelled
/// with the clock it was measured on.
pub fn diag_panel(app: &App) -> crate::render::text::Overlay {
    use crate::render::text::{Anchor, Line, Overlay, Style};
    let m = &app.metrics;
    let r = &app.renderer;
    let f = &app.features;
    let yes = |b: bool| if b { "on" } else { "off" };
    let mut lines = vec![
        Line::new("Diagnostics  (F2 hides)", Style::Heading),
        Line::new(
            format!(
                "scenery {} · uploads {} · sync {}",
                if r.cfg.vector_scenery { "vector f=200" } else { "raster" },
                if r.cfg.uploader.temp_file { "temp file" } else { "inline PNG" },
                yes(r.cfg.sync_output)
            ),
            Style::Normal,
        ),
        Line::new(
            format!(
                "ink {} · frame reports {} · low latency {} · haptics {} · tablet {}",
                yes(f.ink && f.ink_brush_sent),
                yes(f.frame_feedback),
                yes(app.latency_on),
                yes(f.haptics),
                f.tablet_mode
            ),
            Style::Normal,
        ),
        Line::new(
            format!(
                "sim tick {:.3} ms (max {:.3}) · flock {:.3} · ai+nav {:.3}",
                m.tick.mean(),
                m.tick.max(),
                m.flock.mean(),
                m.ai.mean()
            ),
            Style::Normal,
        ),
        Line::new(
            format!(
                "frame build {:.2} ms · write {:.2} ms · {:.0} B/frame",
                m.build.mean(),
                m.write.mean(),
                m.frame_bytes.mean()
            ),
            Style::Normal,
        ),
        Line::new(
            format!(
                "last: {} B {} cmds {} placed {} deleted {} uploads ({} B)",
                r.last.bytes, r.last.commands, r.last.placements, r.last.deletions, r.last.uploads, r.last.upload_bytes
            ),
            Style::Dim,
        ),
        Line::new(
            format!(
                "vector: {} edits {} B {} full redraws (last frame)",
                r.last.vector_edits, r.last.vector_bytes, r.last.vector_full_redraws
            ),
            Style::Dim,
        ),
        Line::new(
            format!(
                "totals: {} frames · {:.2} MB written · uploads {:.2} MB · {} on screen",
                r.totals.frames,
                app.tty.stats.bytes as f64 / 1e6,
                r.totals.upload_bytes as f64 / 1e6,
                r.placement_count()
            ),
            Style::Dim,
        ),
    ];
    if f.frame_feedback {
        lines.push(Line::new(
            format!(
                "presented every {:.2} ms (terminal clock) · link round trip {:.1} ms (our clock)",
                m.present_interval.mean(),
                app.pacer.rtt().map_or(0.0, |d| d.as_secs_f64() * 1000.0),
            ),
            Style::Normal,
        ));
        lines.push(Line::new(
            if app.pacer.strict() {
                format!(
                    "strict pacing: report {:.1} ms after write (our clock) · {} timeouts",
                    m.write_to_report.mean(),
                    m.report_timeouts
                )
            } else {
                format!(
                    "pipelined pacing: {} of {} frames on the way · {} timeouts",
                    app.pacer.in_flight(Instant::now()),
                    app.pacer.max_in_flight(),
                    m.report_timeouts
                )
            },
            Style::Normal,
        ));
    } else {
        lines.push(Line::new(
            format!("paced by timer every {:.2} ms (no presentation reports)", app.pacer.period.as_secs_f64() * 1000.0),
            Style::Normal,
        ));
    }
    let mut lat = format!("key to frame written {:.2} ms (our clock)", m.key_to_write.mean());
    if !m.pen_to_present.is_empty() {
        lat.push_str(&format!(" · pen to presented {:.1} ms (terminal clock)", m.pen_to_present.mean()));
    }
    lines.push(Line::new(lat, Style::Normal));
    if let Some(g) = &app.game {
        lines.push(Line::new(
            format!("chicks active {} · nav fields recomputed {}", g.remaining(), g.nav.recomputes),
            Style::Dim,
        ));
    }
    if let Some(e) = &r.last_error {
        lines.push(Line::new(format!("terminal said ({}x): {e}", r.error_count), Style::Bad));
    }
    let mut o = Overlay::new(Anchor::TopRight, lines);
    o.alpha = 80;
    o
}

/// How much brighter than SDR white the white-hot cores of sparkles, stars
/// and hearts are drawn where the display has the headroom: enough to
/// glint, not to glare. The terminal clips them to its headroom, so on a
/// display without any they look exactly as they do in SDR.
const HDR_GLINT: f32 = 3.0;

pub fn level_count() -> usize {
    level::BUILTIN.len()
}
