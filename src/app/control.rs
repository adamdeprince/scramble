//! Keys, mouse and pen.
//!
//! Movement keys set the direction the hen wants to go; she keeps going
//! until she can turn that way (as in classic maze games), so the result is
//! the same whether the terminal reports key releases or not. Space works a
//! latch next to the hen or drops a fence behind her. Fences can also be
//! drawn: with the mouse anywhere, and on Silkie with a pen, whose ink the
//! terminal draws itself, or a finger on a touchscreen.

use super::strokes::Source;
use super::{App, Screen};
use crate::term::input::{KeyCode, KeyEvent, KeyKind, MouseButton, MouseEvent, MouseKind};
use crate::term::osc777::Frame;
use crate::sim::gate::latch_in_reach;
use crate::world::{Dir, TilePos};
use glam::Vec2;

pub const PAUSE_MENU: [&str; 4] = ["Resume", "Restart level", "Quit to title", "Quit game"];

fn direction(code: KeyCode) -> Option<Dir> {
    match code {
        KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('W') | KeyCode::Char('k') => Some(Dir::Up),
        KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') | KeyCode::Char('j') => Some(Dir::Down),
        KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('h') => Some(Dir::Left),
        KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Char('l') => Some(Dir::Right),
        _ => None,
    }
}

impl App {
    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyKind::Release {
            return;
        }
        let ch = match k.code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        if k.mods.ctrl() && ch == Some('c') {
            self.quit = true;
            return;
        }
        if k.mods.ctrl() && ch == Some('z') {
            if let Err(e) = self.suspend() {
                tracing::error!("suspend: {e}");
            }
            return;
        }
        if k.code == KeyCode::F(2) || ch == Some('`') || ch == Some('~') {
            self.show_diag = !self.show_diag;
            return;
        }
        match &mut self.screen {
            Screen::Title { .. } => match (k.code, ch) {
                (KeyCode::Enter, _) | (_, Some(' ')) => self.start_game(),
                (KeyCode::Escape, _) | (_, Some('q')) => self.quit = true,
                (_, Some('c')) => self.screen = Screen::Report,
                _ => {}
            },
            Screen::Report => self.go_title(),
            Screen::Play => {
                if let Some(d) = direction(k.code) {
                    if k.kind == KeyKind::Press {
                        self.want_dir = Some(d);
                        self.note_key();
                    }
                    return;
                }
                match (k.code, ch) {
                    (_, Some(' ')) | (KeyCode::Enter, _) => {
                        if k.kind == KeyKind::Press {
                            self.action = true;
                            self.note_key();
                        }
                    }
                    (_, Some('p')) | (KeyCode::Escape, _) => self.pause(),
                    _ => {}
                }
            }
            Screen::Paused { menu } => match (k.code, ch) {
                (KeyCode::Up, _) | (_, Some('w')) => *menu = (*menu + PAUSE_MENU.len() - 1) % PAUSE_MENU.len(),
                (KeyCode::Down, _) | (_, Some('s')) => *menu = (*menu + 1) % PAUSE_MENU.len(),
                (_, Some('p')) | (KeyCode::Escape, _) => self.resume(),
                (KeyCode::Enter, _) | (_, Some(' ')) => {
                    let m = *menu;
                    self.pause_menu(m);
                }
                (_, Some('q')) => self.quit = true,
                _ => {}
            },
            Screen::GameOver { name, rank } => {
                if rank.is_some() {
                    match k.code {
                        KeyCode::Char(c) if (c.is_alphanumeric() || c == ' ' || c == '-') && name.chars().count() < 12 && !k.mods.ctrl() => {
                            name.push(c)
                        }
                        KeyCode::Backspace => {
                            name.pop();
                        }
                        KeyCode::Enter => self.save_score(),
                        KeyCode::Escape => self.go_title(),
                        _ => {}
                    }
                } else if matches!(k.code, KeyCode::Enter | KeyCode::Escape) || ch == Some(' ') {
                    self.go_title();
                }
            }
        }
    }

    pub fn pause(&mut self) {
        self.screen = Screen::Paused { menu: 0 };
        self.strokes.clear();
        self.set_low_latency(false);
    }

    fn resume(&mut self) {
        self.screen = Screen::Play;
        self.set_low_latency(true);
    }

    fn pause_menu(&mut self, item: usize) {
        match item {
            0 => self.resume(),
            1 => {
                if let Some(g) = self.game.take() {
                    let mut s = g.session.clone();
                    s.lives = s.lives.saturating_sub(1).max(1);
                    self.load_level(g.level_index, s);
                }
                self.resume();
            }
            2 => self.go_title(),
            _ => self.quit = true,
        }
    }

    fn save_score(&mut self) {
        if let (Screen::GameOver { name, .. }, Some(g)) = (&self.screen, &self.game) {
            let name = if name.trim().is_empty() { "Hen".to_string() } else { name.trim().to_string() };
            let entry = super::scores::Entry {
                name,
                score: g.session.score,
                level: g.level_index as u32 + 1 + self.round * super::level_count() as u32,
                chicks: g.session.chicks_saved,
            };
            if let Err(e) = self.scores.add(entry) {
                tracing::warn!("saving high scores: {e}");
            }
        }
        self.go_title();
    }

    /// Ends a stroke; a tap on a latched gate within the hen's reach works
    /// the gate, exactly as Space would.
    fn finish_stroke(&mut self, at: Option<Vec2>) {
        let game = self.game.as_ref();
        let on_gate = |p: Vec2| {
            game.is_some_and(|g| latch_in_reach(g).is_some_and(|i| g.gates[i].tile == TilePos::of(p)))
        };
        let reach = game.and_then(latch_in_reach);
        let tapped = self.strokes.finish(at, on_gate);
        tracing::debug!(?at, ?reach, tapped, hen = ?game.map(|g| g.hen.pos()), "stroke finished");
        if tapped {
            self.action = true;
        }
    }

    fn world_at(&self, px: Option<(i32, i32)>, col: u32, row: u32) -> Option<Vec2> {
        let l = self.renderer.layout?;
        let (x, y) = match px {
            Some((x, y)) => (x as f32, y as f32),
            None => ((col as f32 + 0.5) * l.cell_w as f32, (row as f32 + 0.5) * l.cell_h as f32),
        };
        Some(l.px_to_world(x, y))
    }

    fn in_yard(&self, p: Vec2) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x < self.level.grid.width as f32 && p.y < self.level.grid.height as f32
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let pos = self.world_at(m.px, m.col, m.row);
        match &self.screen {
            Screen::Title { .. } => {
                if matches!(m.kind, MouseKind::Press(MouseButton::Left)) {
                    self.start_game();
                }
            }
            Screen::Paused { .. } => {
                if matches!(m.kind, MouseKind::Press(MouseButton::Left)) {
                    self.resume();
                }
            }
            Screen::Report => {
                if matches!(m.kind, MouseKind::Press(_)) {
                    self.go_title();
                }
            }
            Screen::GameOver { .. } => {}
            Screen::Play => {
                let Some(p) = pos else { return };
                match m.kind {
                    MouseKind::Press(MouseButton::Left) if self.in_yard(p) => self.strokes.begin(p, Source::Mouse, None, None),
                    MouseKind::Drag(MouseButton::Left) => self.strokes.extend(p),
                    MouseKind::Release(_) => self.finish_stroke(Some(p)),
                    MouseKind::Press(MouseButton::Right) => self.strokes.cancel(),
                    _ => {}
                }
            }
        }
    }

    /// A report of the tablet pointer protocol: a pen (tools) or a finger
    /// on a touchscreen. Positions are 1/256 physical pixel.
    pub fn on_pointer_report(&mut self, f: &Frame) {
        let kind = f.get_u64("kind").unwrap_or(0);
        if kind != 1 && kind != 2 {
            return;
        }
        if kind == 2 && f.get_u64("src") == Some(1) {
            // Fingers on a touchpad are positions on the pad, not the
            // screen.
            return;
        }
        let phase = f.get_u64("phase").unwrap_or(0);
        let ink = f.get_u64("ink").map(|v| v as u32);
        let t = f.get_u64("t");
        let x = f.get_i64("x").unwrap_or(0) as f32 / 256.0;
        let y = f.get_i64("y").unwrap_or(0) as f32 / 256.0;
        let pos = self.renderer.layout.map(|l| l.px_to_world(x, y));
        let playing = matches!(self.screen, Screen::Play);
        let source = if kind == 1 { Source::Pen } else { Source::Finger };
        match phase {
            1 => {
                match (playing, pos) {
                    (true, Some(p)) if self.in_yard(p) => {
                        self.strokes.begin(p, source, ink, t);
                        if ink.is_none() {
                            if let Some(t) = t {
                                self.note_pen(t);
                            }
                        }
                    }
                    _ => {
                        // Not drawing a fence now: take the terminal's ink
                        // away again.
                        if let Some(i) = ink {
                            self.strokes.ink_to_delete.push(i);
                        }
                        if matches!(self.screen, Screen::Title { .. }) {
                            self.start_game();
                        } else if matches!(self.screen, Screen::Paused { .. }) {
                            self.resume();
                        }
                    }
                }
            }
            2 => {
                if let Some(p) = pos {
                    let terminal_ink = self.strokes.active.as_ref().is_some_and(|s| s.ink.is_some());
                    self.strokes.extend(p);
                    if !terminal_ink {
                        if let Some(t) = t {
                            self.note_pen(t);
                        }
                    }
                }
            }
            3 => self.finish_stroke(pos),
            4 | 5 => self.strokes.cancel(),
            _ => {}
        }
    }
}
