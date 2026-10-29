//! Terminal lifecycle: raw mode, the alternate screen, the modes the game
//! turns on and the exact sequence that turns them off again, signals,
//! suspension, and a panic hook that always leaves the terminal usable.
//!
//! crossterm handles the termios side (raw mode, window size). Everything
//! written to the terminal goes through [`Modes`], which records what was
//! enabled so that cleanup sends precisely the matching disables, whether
//! the game quits, is suspended with Ctrl+Z, receives SIGTERM, or panics.

use super::osc777::req;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Bytes that restore the terminal, kept up to date for the panic hook and
/// signal paths, which cannot reach the game's state.
static RESTORE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, Default)]
pub struct Modes {
    pub alt_screen: bool,
    pub cursor_hidden: bool,
    pub kitty_keyboard: Option<u32>,
    pub mouse: bool,
    pub sgr_pixels: bool,
    pub focus: bool,
    /// OSC 777 selectors with a mode turned on, and the mode value.
    pub osc: BTreeMap<String, u32>,
    /// Image ids the game created, deleted on exit.
    pub images: Vec<u32>,
    /// Pointer shapes pushed (OSC 22), popped on exit.
    pub pointer_pushes: u32,
}

impl Modes {
    /// Bytes that turn everything recorded here off, newest first.
    pub fn restore_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // Ink depends on tablet reports, so it goes off first.
        let mut sels: Vec<&String> = self.osc.keys().collect();
        sels.sort_by_key(|s| (s.as_str() != "ink", s.as_str()));
        for sel in sels {
            if let Ok(f) = req::set_mode(sel, 0).encode() {
                out.extend_from_slice(&f);
            }
        }
        for id in &self.images {
            super::kitty_gfx::delete_image(&mut out, *id);
        }
        for _ in 0..self.pointer_pushes {
            out.extend_from_slice(b"\x1b]22;<\x1b\\");
        }
        if self.focus {
            out.extend_from_slice(b"\x1b[?1004l");
        }
        if self.sgr_pixels {
            out.extend_from_slice(b"\x1b[?1016l");
        }
        if self.mouse {
            out.extend_from_slice(b"\x1b[?1006l\x1b[?1002l\x1b[?1003l");
        }
        if self.kitty_keyboard.is_some() {
            out.extend_from_slice(b"\x1b[<u");
        }
        out.extend_from_slice(b"\x1b[0m");
        if self.cursor_hidden {
            out.extend_from_slice(b"\x1b[?25h");
        }
        if self.alt_screen {
            out.extend_from_slice(b"\x1b[?1049l");
        }
        out
    }

    fn publish(&self) {
        if let Ok(mut r) = RESTORE.lock() {
            *r = self.restore_bytes();
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WriteStats {
    pub bytes: u64,
    pub writes: u64,
    pub last_write: Duration,
    pub max_write: Duration,
    pub last_bytes: usize,
}

/// Messages for the main loop from the input and signal threads.
#[derive(Debug)]
pub enum Msg {
    Input(Vec<u8>),
    InputClosed,
    Signal(i32),
}

pub struct Tty {
    pub modes: Modes,
    pub stats: WriteStats,
    raw: bool,
}

impl Tty {
    /// Puts the terminal into raw mode. Fails when stdin or stdout is not a
    /// terminal.
    pub fn open() -> anyhow::Result<Tty> {
        // SAFETY: isatty only inspects the descriptor.
        let tty = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
        if !tty {
            anyhow::bail!("Silkie Scramble needs to run in a terminal (stdin and stdout must be a tty)");
        }
        crossterm::terminal::enable_raw_mode()?;
        install_panic_hook();
        Ok(Tty {
            modes: Modes::default(),
            stats: WriteStats::default(),
            raw: true,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let started = Instant::now();
        let mut out = io::stdout().lock();
        out.write_all(bytes)?;
        out.flush()?;
        let d = started.elapsed();
        self.stats.bytes += bytes.len() as u64;
        self.stats.writes += 1;
        self.stats.last_write = d;
        self.stats.max_write = self.stats.max_write.max(d);
        self.stats.last_bytes = bytes.len();
        Ok(())
    }

    /// Enters the alternate screen and hides the cursor.
    pub fn enter_screen(&mut self) -> io::Result<()> {
        self.modes.alt_screen = true;
        self.modes.cursor_hidden = true;
        self.modes.publish();
        self.write(b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J\x1b[H")
    }

    /// Turns on the kitty keyboard protocol (when known), mouse reporting
    /// (with pixel positions when available) and focus reports.
    pub fn enable_input(&mut self, kitty_keyboard: bool, sgr_pixels: bool) -> io::Result<()> {
        let mut out = Vec::new();
        if kitty_keyboard {
            // Disambiguate (1), event types (2), all keys as escapes (8).
            out.extend_from_slice(b"\x1b[>11u");
            self.modes.kitty_keyboard = Some(11);
        }
        out.extend_from_slice(b"\x1b[?1002h\x1b[?1006h");
        self.modes.mouse = true;
        if sgr_pixels {
            out.extend_from_slice(b"\x1b[?1016h");
            self.modes.sgr_pixels = true;
        }
        out.extend_from_slice(b"\x1b[?1004h");
        self.modes.focus = true;
        self.modes.publish();
        self.write(&out)
    }

    /// Records an OSC 777 mode as on (the caller sends the frame).
    pub fn note_osc_mode(&mut self, selector: &str, mode: u32) {
        if mode == 0 {
            self.modes.osc.remove(selector);
        } else {
            self.modes.osc.insert(selector.to_string(), mode);
        }
        self.modes.publish();
    }

    pub fn note_image(&mut self, id: u32) {
        if !self.modes.images.contains(&id) {
            self.modes.images.push(id);
            self.modes.publish();
        }
    }

    /// Pushes a pointer shape (OSC 22), remembered so it is popped again.
    pub fn push_pointer(&mut self, shape: &str) -> io::Result<()> {
        self.modes.pointer_pushes += 1;
        self.modes.publish();
        self.write(format!("\x1b]22;>{shape}\x1b\\").as_bytes())
    }

    pub fn pop_pointer(&mut self) -> io::Result<()> {
        if self.modes.pointer_pushes == 0 {
            return Ok(());
        }
        self.modes.pointer_pushes -= 1;
        self.modes.publish();
        self.write(b"\x1b]22;<\x1b\\")
    }

    pub fn forget_image(&mut self, id: u32) {
        self.modes.images.retain(|&i| i != id);
        self.modes.publish();
    }

    /// Turns every mode off and leaves raw mode.
    pub fn restore(&mut self) {
        let bytes = self.modes.restore_bytes();
        let _ = self.write(&bytes);
        self.modes = Modes::default();
        self.modes.publish();
        if self.raw {
            let _ = crossterm::terminal::disable_raw_mode();
            self.raw = false;
        }
    }

    /// Ctrl+Z: restore the terminal, stop until continued, then re-enter
    /// raw mode. The caller re-enables its modes and redraws, since a shell
    /// prompt in between resets every OSC 777 mode anyway.
    pub fn suspend(&mut self) -> anyhow::Result<()> {
        let saved = self.modes.clone();
        self.restore();
        let _ = signal_hook::low_level::emulate_default_handler(libc::SIGTSTP);
        crossterm::terminal::enable_raw_mode()?;
        self.raw = true;
        self.modes = Modes {
            images: Vec::new(),
            osc: BTreeMap::new(),
            pointer_pushes: 0,
            ..saved
        };
        self.modes.publish();
        let mut out = Vec::new();
        if self.modes.alt_screen {
            out.extend_from_slice(b"\x1b[?1049h\x1b[?7l\x1b[2J\x1b[H");
        }
        if self.modes.cursor_hidden {
            out.extend_from_slice(b"\x1b[?25l");
        }
        if let Some(f) = self.modes.kitty_keyboard {
            out.extend_from_slice(format!("\x1b[>{f}u").as_bytes());
        }
        if self.modes.mouse {
            out.extend_from_slice(b"\x1b[?1002h\x1b[?1006h");
        }
        if self.modes.sgr_pixels {
            out.extend_from_slice(b"\x1b[?1016h");
        }
        if self.modes.focus {
            out.extend_from_slice(b"\x1b[?1004h");
        }
        self.write(&out)?;
        Ok(())
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.restore();
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(r) = RESTORE.lock() {
            let mut out = io::stdout().lock();
            let _ = out.write_all(&r);
            let _ = out.flush();
        }
        let _ = crossterm::terminal::disable_raw_mode();
        default(info);
    }));
}

/// Reads stdin on its own thread and forwards what it reads.
pub fn spawn_input_thread(tx: Sender<Msg>) {
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            let mut stdin = io::stdin().lock();
            // A read chunk, not a limit: anything longer arrives in the
            // next read.
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Msg::InputClosed);
                        return;
                    }
                    Ok(n) => {
                        if tx.send(Msg::Input(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        let _ = tx.send(Msg::InputClosed);
                        return;
                    }
                }
            }
        })
        .expect("spawn stdin thread");
}

/// Forwards SIGWINCH, SIGINT, SIGTERM, SIGHUP, SIGTSTP and SIGCONT.
pub fn spawn_signal_thread(tx: Sender<Msg>) -> anyhow::Result<()> {
    use signal_hook::consts::*;
    let mut signals = signal_hook::iterator::Signals::new([SIGWINCH, SIGINT, SIGTERM, SIGHUP, SIGTSTP, SIGCONT])?;
    std::thread::Builder::new().name("signals".into()).spawn(move || {
        for s in signals.forever() {
            if tx.send(Msg::Signal(s)).is_err() {
                return;
            }
        }
    })?;
    Ok(())
}

/// Columns, rows, and window size in pixels (0 when the terminal does not
/// say).
pub fn size() -> (u32, u32, u32, u32) {
    match crossterm::terminal::window_size() {
        Ok(w) => (w.columns as u32, w.rows as u32, w.width as u32, w.height as u32),
        Err(_) => match crossterm::terminal::size() {
            Ok((c, r)) => (c as u32, r as u32, 0, 0),
            Err(_) => (80, 24, 0, 0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_undoes_everything() {
        let mut m = Modes {
            alt_screen: true,
            cursor_hidden: true,
            kitty_keyboard: Some(11),
            mouse: true,
            sgr_pixels: true,
            focus: true,
            ..Default::default()
        };
        m.osc.insert("ink".into(), 1);
        m.osc.insert("tablet-pointer".into(), 1);
        m.osc.insert("frame".into(), 1);
        m.images = vec![7, 9];
        let r = String::from_utf8(m.restore_bytes()).unwrap();
        for needle in [
            "\x1b]777;ink;v=1:op=mode:mode=0\x1b\\",
            "\x1b]777;tablet-pointer;v=1:op=mode:mode=0\x1b\\",
            "\x1b]777;frame;v=1:op=mode:mode=0\x1b\\",
            "a=d,d=I,i=7",
            "a=d,d=I,i=9",
            "\x1b[<u",
            "\x1b[?1006l",
            "\x1b[?1016l",
            "\x1b[?1004l",
            "\x1b[?25h",
            "\x1b[?1049l",
        ] {
            assert!(r.contains(needle), "missing {needle:?}");
        }
        // Ink goes off before the tablet reports it depends on, and the
        // alternate screen is left last.
        assert!(r.find("777;ink").unwrap() < r.find("777;tablet-pointer").unwrap());
        assert!(r.ends_with("\x1b[?1049l"));
    }

    #[test]
    fn nothing_enabled_restores_nothing_but_sgr() {
        assert_eq!(Modes::default().restore_bytes(), b"\x1b[0m".to_vec());
    }
}
