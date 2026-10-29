//! Decoding of everything the terminal writes to the program: keys (legacy
//! and the kitty keyboard protocol), SGR mouse reports, focus changes, and
//! replies to queries (graphics protocol, OSC 777 extensions, device
//! attributes, mode reports, window sizes).
//!
//! The game reads stdin itself instead of through crossterm's event reader:
//! crossterm does not understand APC graphics replies or OSC 777 reports, and
//! two parsers reading one stream would steal each other's bytes. A single
//! tokenizer that knows the framing of every control-string type keeps the
//! stream intact, whatever is interleaved with what.

use super::osc777::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mods(pub u8);

impl Mods {
    pub const SHIFT: u8 = 1;
    pub const ALT: u8 = 2;
    pub const CTRL: u8 = 4;
    pub const SUPER: u8 = 8;

    pub fn shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }
    pub fn alt(self) -> bool {
        self.0 & Self::ALT != 0
    }
    pub fn ctrl(self) -> bool {
        self.0 & Self::CTRL != 0
    }
    pub fn super_key(self) -> bool {
        self.0 & Self::SUPER != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    Backspace,
    Escape,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    /// A key the game has no use for (modifier keys, media keys, ...).
    Other(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub mods: Mods,
    pub kind: KeyKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Press(MouseButton),
    Release(Option<MouseButton>),
    Drag(MouseButton),
    Move,
    ScrollUp,
    ScrollDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseKind,
    /// Zero-based cell column and row (derived from pixels in pixel mode).
    pub col: u32,
    pub row: u32,
    /// Pixel position when SGR pixel reporting (mode 1016) is active.
    pub px: Option<(i32, i32)>,
    pub mods: Mods,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// A graphics protocol response: `ESC _ G i=..;message ESC \`.
    Graphics {
        id: u32,
        placement: Option<u32>,
        number: Option<u32>,
        message: String,
    },
    KeyboardFlags(u32),
    DeviceAttributes(String),
    CellSize { width: u32, height: u32 },
    WindowPixels { width: u32, height: u32 },
    TextArea { cols: u32, rows: u32 },
    /// DECRPM: `CSI ? mode ; value $ y`.
    Mode { mode: u32, value: u32 },
    Osc777(Frame),
    /// XTGETTCAP reply; `value` is `None` when the terminal does not know it.
    Tcap { name: String, value: Option<String> },
    /// Answer to a pointer shape query (OSC 22).
    Pointer(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Focus(bool),
    Reply(Reply),
}

/// A raw control-string token.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Text(char),
    Ctrl(u8),
    Csi {
        private: Option<u8>,
        params: Vec<u8>,
        intermediates: Vec<u8>,
        final_byte: u8,
    },
    Osc(Vec<u8>),
    Apc(Vec<u8>),
    Dcs(Vec<u8>),
    Ss3(u8),
    Alt(char),
    Escape,
    /// A string type the game ignores (PM, SOS) or a malformed sequence.
    Ignored,
}

enum Step {
    Token(Token, usize),
    Incomplete,
}

#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    /// Interpret SGR mouse coordinates as pixels (mode 1016 is on).
    pub sgr_pixels: bool,
    /// Cell size used to derive cell coordinates in pixel mode.
    pub cell_px: (u32, u32),
}

impl Decoder {
    pub fn new() -> Self {
        Decoder {
            buf: Vec::new(),
            sgr_pixels: false,
            cell_px: (1, 1),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// True when the buffer holds only the start of an escape sequence that
    /// may be a lone Escape key press.
    pub fn has_pending_escape(&self) -> bool {
        self.buf == [0x1b]
    }

    /// Bytes held back because they end in an incomplete sequence.
    pub fn pending_len(&self) -> usize {
        self.buf.len()
    }

    /// Treats a lone pending ESC as the Escape key. Called by the event loop
    /// after a short pause with no further input.
    pub fn flush_lone_escape(&mut self) -> Option<InputEvent> {
        if self.has_pending_escape() {
            self.buf.clear();
            return Some(InputEvent::Key(KeyEvent {
                code: KeyCode::Escape,
                mods: Mods::default(),
                kind: KeyKind::Press,
            }));
        }
        None
    }

    /// Decodes every complete event in the buffer.
    pub fn drain(&mut self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        let mut pos = 0;
        while pos < self.buf.len() {
            match next_token(&self.buf[pos..]) {
                Step::Incomplete => break,
                Step::Token(tok, used) => {
                    pos += used;
                    if let Some(ev) = self.interpret(tok) {
                        events.push(ev);
                    }
                }
            }
        }
        self.buf.drain(..pos);
        events
    }

    fn interpret(&self, tok: Token) -> Option<InputEvent> {
        match tok {
            Token::Text(c) => Some(key(KeyCode::Char(c), 0, KeyKind::Press)),
            Token::Ctrl(b) => Some(ctrl_key(b)),
            Token::Alt(c) => Some(key(KeyCode::Char(c), Mods::ALT, KeyKind::Press)),
            Token::Escape => Some(key(KeyCode::Escape, 0, KeyKind::Press)),
            Token::Ss3(b) => ss3_key(b),
            Token::Csi {
                private,
                params,
                intermediates,
                final_byte,
            } => self.csi(private, &params, &intermediates, final_byte),
            Token::Osc(body) => match body.strip_prefix(b"22;") {
                Some(rest) => Some(InputEvent::Reply(Reply::Pointer(String::from_utf8_lossy(rest).into_owned()))),
                None => Frame::parse_osc_body(&body).map(|f| InputEvent::Reply(Reply::Osc777(f))),
            },
            Token::Apc(body) => parse_apc(&body),
            Token::Dcs(body) => parse_dcs(&body),
            Token::Ignored => None,
        }
    }

    fn csi(&self, private: Option<u8>, params: &[u8], inter: &[u8], fin: u8) -> Option<InputEvent> {
        let p = Params::parse(params);
        match (private, inter, fin) {
            (Some(b'<'), [], b'M' | b'm') => self.sgr_mouse(&p, fin == b'm'),
            (Some(b'?'), [], b'u') => Some(InputEvent::Reply(Reply::KeyboardFlags(p.get(0, 0, 0)))),
            (Some(b'?'), [], b'c') => Some(InputEvent::Reply(Reply::DeviceAttributes(
                String::from_utf8_lossy(params).into_owned(),
            ))),
            (Some(b'?'), [b'$'], b'y') => Some(InputEvent::Reply(Reply::Mode {
                mode: p.get(0, 0, 0),
                value: p.get(1, 0, 0),
            })),
            (None, [], b't') => match p.get(0, 0, 0) {
                4 => Some(InputEvent::Reply(Reply::WindowPixels {
                    height: p.get(1, 0, 0),
                    width: p.get(2, 0, 0),
                })),
                6 => Some(InputEvent::Reply(Reply::CellSize {
                    height: p.get(1, 0, 0),
                    width: p.get(2, 0, 0),
                })),
                8 => Some(InputEvent::Reply(Reply::TextArea {
                    rows: p.get(1, 0, 0),
                    cols: p.get(2, 0, 0),
                })),
                _ => None,
            },
            (None, [], b'I') if p.is_empty() => Some(InputEvent::Focus(true)),
            (None, [], b'O') if p.is_empty() => Some(InputEvent::Focus(false)),
            (None, [], b'u') => {
                let code = p.get(0, 0, 0);
                let (mods, kind) = mods_and_kind(&p);
                Some(key(kitty_code(code), mods, kind))
            }
            (None, [], b'~') => {
                let code = match p.get(0, 0, 0) {
                    2 => KeyCode::Insert,
                    3 => KeyCode::Delete,
                    5 => KeyCode::PageUp,
                    6 => KeyCode::PageDown,
                    1 | 7 => KeyCode::Home,
                    4 | 8 => KeyCode::End,
                    11 => KeyCode::F(1),
                    12 => KeyCode::F(2),
                    13 => KeyCode::F(3),
                    14 => KeyCode::F(4),
                    15 => KeyCode::F(5),
                    17 => KeyCode::F(6),
                    18 => KeyCode::F(7),
                    19 => KeyCode::F(8),
                    20 => KeyCode::F(9),
                    21 => KeyCode::F(10),
                    23 => KeyCode::F(11),
                    24 => KeyCode::F(12),
                    // Bracketed paste markers and unknown keys.
                    other => KeyCode::Other(other),
                };
                let (mods, kind) = mods_and_kind(&p);
                Some(key(code, mods, kind))
            }
            (None, [], b'A' | b'B' | b'C' | b'D' | b'H' | b'F' | b'P' | b'Q' | b'R' | b'S' | b'E') => {
                let code = match fin {
                    b'A' => KeyCode::Up,
                    b'B' => KeyCode::Down,
                    b'C' => KeyCode::Right,
                    b'D' => KeyCode::Left,
                    b'H' => KeyCode::Home,
                    b'F' => KeyCode::End,
                    b'P' => KeyCode::F(1),
                    b'Q' => KeyCode::F(2),
                    b'R' => KeyCode::F(3),
                    b'S' => KeyCode::F(4),
                    _ => KeyCode::Other(57427), // keypad begin
                };
                let (mods, kind) = mods_and_kind(&p);
                Some(key(code, mods, kind))
            }
            _ => None,
        }
    }

    fn sgr_mouse(&self, p: &Params, release: bool) -> Option<InputEvent> {
        let b = p.get(0, 0, 0);
        let x = p.get(1, 0, 1) as i64;
        let y = p.get(2, 0, 1) as i64;
        let mut mods = 0u8;
        if b & 4 != 0 {
            mods |= Mods::SHIFT;
        }
        if b & 8 != 0 {
            mods |= Mods::ALT;
        }
        if b & 16 != 0 {
            mods |= Mods::CTRL;
        }
        let motion = b & 32 != 0;
        let button = match b & 3 {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Middle),
            2 => Some(MouseButton::Right),
            _ => None,
        };
        let kind = if b & 64 != 0 {
            if b & 1 == 0 {
                MouseKind::ScrollUp
            } else {
                MouseKind::ScrollDown
            }
        } else if release {
            MouseKind::Release(button)
        } else if motion {
            match button {
                Some(btn) => MouseKind::Drag(btn),
                None => MouseKind::Move,
            }
        } else {
            match button {
                Some(btn) => MouseKind::Press(btn),
                None => MouseKind::Move,
            }
        };
        let (col, row, px) = if self.sgr_pixels {
            let px = ((x - 1).max(0) as i32, (y - 1).max(0) as i32);
            let cw = self.cell_px.0.max(1) as i32;
            let ch = self.cell_px.1.max(1) as i32;
            ((px.0 / cw) as u32, (px.1 / ch) as u32, Some(px))
        } else {
            ((x - 1).max(0) as u32, (y - 1).max(0) as u32, None)
        };
        Some(InputEvent::Mouse(MouseEvent {
            kind,
            col,
            row,
            px,
            mods: Mods(mods),
        }))
    }
}

fn key(code: KeyCode, mods: u8, kind: KeyKind) -> InputEvent {
    InputEvent::Key(KeyEvent {
        code,
        mods: Mods(mods),
        kind,
    })
}

fn ctrl_key(b: u8) -> InputEvent {
    match b {
        0x0d | 0x0a => key(KeyCode::Enter, 0, KeyKind::Press),
        0x09 => key(KeyCode::Tab, 0, KeyKind::Press),
        0x7f | 0x08 => key(KeyCode::Backspace, 0, KeyKind::Press),
        0x00 => key(KeyCode::Char(' '), Mods::CTRL, KeyKind::Press),
        0x01..=0x1a => key(KeyCode::Char((b'a' + b - 1) as char), Mods::CTRL, KeyKind::Press),
        other => key(KeyCode::Other(other as u32), 0, KeyKind::Press),
    }
}

fn ss3_key(b: u8) -> Option<InputEvent> {
    let code = match b {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        b'M' => KeyCode::Enter,
        _ => return None,
    };
    Some(key(code, 0, KeyKind::Press))
}

/// Maps a kitty keyboard protocol key number to a key code.
fn kitty_code(code: u32) -> KeyCode {
    match code {
        27 => KeyCode::Escape,
        13 => KeyCode::Enter,
        9 => KeyCode::Tab,
        127 | 8 => KeyCode::Backspace,
        57399..=57408 => KeyCode::Char(char::from(b'0' + (code - 57399) as u8)),
        57414 => KeyCode::Enter, // KP_ENTER
        57417 => KeyCode::Left,
        57418 => KeyCode::Right,
        57419 => KeyCode::Up,
        57420 => KeyCode::Down,
        57421 => KeyCode::PageUp,
        57422 => KeyCode::PageDown,
        57423 => KeyCode::Home,
        57424 => KeyCode::End,
        57425 => KeyCode::Insert,
        57426 => KeyCode::Delete,
        57364..=57398 => KeyCode::F((code - 57364 + 13) as u8),
        c if (57344..=63743).contains(&c) => KeyCode::Other(c),
        c => match char::from_u32(c) {
            Some(ch) => KeyCode::Char(ch),
            None => KeyCode::Other(c),
        },
    }
}

fn mods_and_kind(p: &Params) -> (u8, KeyKind) {
    let m = p.get(1, 0, 1).max(1) - 1;
    let kind = match p.get(1, 1, 1) {
        2 => KeyKind::Repeat,
        3 => KeyKind::Release,
        _ => KeyKind::Press,
    };
    // Only shift, alt, ctrl and super matter to the game; lock keys are
    // dropped so caps lock does not turn WASD into shortcuts.
    ((m & 0x0f) as u8, kind)
}

/// CSI parameters: `;` separated fields of `:` separated sub-fields.
struct Params(Vec<Vec<Option<u32>>>);

impl Params {
    fn parse(raw: &[u8]) -> Self {
        if raw.is_empty() {
            return Params(Vec::new());
        }
        let text = String::from_utf8_lossy(raw);
        Params(
            text.split(';')
                .map(|field| field.split(':').map(|s| s.parse().ok()).collect())
                .collect(),
        )
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn get(&self, field: usize, sub: usize, default: u32) -> u32 {
        self.0
            .get(field)
            .and_then(|f| f.get(sub))
            .copied()
            .flatten()
            .unwrap_or(default)
    }
}

fn parse_apc(body: &[u8]) -> Option<InputEvent> {
    let rest = body.strip_prefix(b"G")?;
    let (control, message) = match rest.iter().position(|&b| b == b';') {
        Some(p) => (&rest[..p], &rest[p + 1..]),
        None => (rest, &b""[..]),
    };
    let control = String::from_utf8_lossy(control);
    let mut id = 0;
    let mut placement = None;
    let mut number = None;
    for kv in control.split(',') {
        if let Some((k, v)) = kv.split_once('=') {
            match k {
                "i" => id = v.parse().unwrap_or(0),
                "p" => placement = v.parse().ok(),
                "I" => number = v.parse().ok(),
                _ => {}
            }
        }
    }
    Some(InputEvent::Reply(Reply::Graphics {
        id,
        placement,
        number,
        message: String::from_utf8_lossy(message).into_owned(),
    }))
}

fn parse_dcs(body: &[u8]) -> Option<InputEvent> {
    // XTGETTCAP: DCS 1 + r <hex name> = <hex value> ST, or DCS 0 + r <hex name> ST.
    let text = std::str::from_utf8(body).ok()?;
    let (ok, rest) = match text.strip_prefix("1+r") {
        Some(r) => (true, r),
        None => (false, text.strip_prefix("0+r")?),
    };
    let (name_hex, value_hex) = match rest.split_once('=') {
        Some((n, v)) => (n, Some(v)),
        None => (rest, None),
    };
    let name = unhex(name_hex)?;
    let value = if ok { value_hex.and_then(unhex) } else { None };
    Some(InputEvent::Reply(Reply::Tcap { name, value }))
}

fn unhex(s: &str) -> Option<String> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// Scans one token from the start of `buf`.
fn next_token(buf: &[u8]) -> Step {
    let b = buf[0];
    if b == 0x1b {
        if buf.len() < 2 {
            return Step::Incomplete;
        }
        match buf[1] {
            b'[' => scan_csi(buf),
            b']' => scan_string(buf, 2, true).map_token(Token::Osc),
            b'_' => scan_string(buf, 2, false).map_token(Token::Apc),
            b'P' => scan_string(buf, 2, false).map_token(Token::Dcs),
            b'^' | b'X' => scan_string(buf, 2, false).map_token(|_| Token::Ignored),
            b'O' => {
                if buf.len() < 3 {
                    Step::Incomplete
                } else {
                    Step::Token(Token::Ss3(buf[2]), 3)
                }
            }
            0x1b => Step::Token(Token::Escape, 1),
            _ => match decode_utf8(&buf[1..]) {
                Utf8::Char(c, n) => {
                    if (c as u32) < 0x20 || c as u32 == 0x7f {
                        // ESC followed by a control: alt+ctrl key; report the
                        // escape and let the control be decoded on its own.
                        Step::Token(Token::Escape, 1)
                    } else {
                        Step::Token(Token::Alt(c), 1 + n)
                    }
                }
                Utf8::Incomplete => Step::Incomplete,
                Utf8::Invalid => Step::Token(Token::Escape, 1),
            },
        }
    } else if b < 0x20 || b == 0x7f {
        Step::Token(Token::Ctrl(b), 1)
    } else {
        match decode_utf8(buf) {
            Utf8::Char(c, n) => Step::Token(Token::Text(c), n),
            Utf8::Incomplete => Step::Incomplete,
            Utf8::Invalid => Step::Token(Token::Text(char::REPLACEMENT_CHARACTER), 1),
        }
    }
}

trait MapToken {
    fn map_token(self, f: impl FnOnce(Vec<u8>) -> Token) -> Step;
}

impl MapToken for Option<(Vec<u8>, usize)> {
    fn map_token(self, f: impl FnOnce(Vec<u8>) -> Token) -> Step {
        match self {
            Some((body, used)) => Step::Token(f(body), used),
            None => Step::Incomplete,
        }
    }
}

fn scan_csi(buf: &[u8]) -> Step {
    let mut i = 2;
    let mut private = None;
    let mut params = Vec::new();
    let mut inter = Vec::new();
    if i < buf.len() && matches!(buf[i], b'<' | b'=' | b'>' | b'?') {
        private = Some(buf[i]);
        i += 1;
    }
    while i < buf.len() {
        let c = buf[i];
        match c {
            0x30..=0x3f => {
                if !inter.is_empty() {
                    // Parameter after intermediate: malformed.
                    return Step::Token(Token::Ignored, i + 1);
                }
                params.push(c);
            }
            0x20..=0x2f => inter.push(c),
            0x40..=0x7e => {
                return Step::Token(
                    Token::Csi {
                        private,
                        params,
                        intermediates: inter,
                        final_byte: c,
                    },
                    i + 1,
                );
            }
            _ => {
                // A control byte inside a CSI aborts it; resume parsing at
                // that byte.
                return Step::Token(Token::Ignored, i);
            }
        }
        i += 1;
    }
    Step::Incomplete
}

/// Scans a control string to its terminator (ST, or BEL when allowed).
/// Returns the body and the number of bytes consumed.
fn scan_string(buf: &[u8], start: usize, allow_bel: bool) -> Option<(Vec<u8>, usize)> {
    let mut i = start;
    while i < buf.len() {
        match buf[i] {
            0x07 if allow_bel => return Some((buf[start..i].to_vec(), i + 1)),
            0x1b => {
                if i + 1 >= buf.len() {
                    return None;
                }
                if buf[i + 1] == b'\\' {
                    return Some((buf[start..i].to_vec(), i + 2));
                }
                // ESC without '\' ends the string; the ESC starts the next
                // sequence.
                return Some((buf[start..i].to_vec(), i));
            }
            _ => i += 1,
        }
    }
    None
}

enum Utf8 {
    Char(char, usize),
    Incomplete,
    Invalid,
}

fn decode_utf8(buf: &[u8]) -> Utf8 {
    let b = buf[0];
    let len = match b {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Utf8::Invalid,
    };
    if buf.len() < len {
        // Check that what we have so far are continuation bytes.
        if buf[1..].iter().all(|&c| c & 0xc0 == 0x80) {
            return Utf8::Incomplete;
        }
        return Utf8::Invalid;
    }
    match std::str::from_utf8(&buf[..len]) {
        Ok(s) => Utf8::Char(s.chars().next().unwrap_or(char::REPLACEMENT_CHARACTER), len),
        Err(_) => Utf8::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> Vec<InputEvent> {
        let mut d = Decoder::new();
        d.feed(bytes);
        d.drain()
    }

    fn k(code: KeyCode, mods: u8, kind: KeyKind) -> InputEvent {
        key(code, mods, kind)
    }

    #[test]
    fn plain_text_and_controls() {
        assert_eq!(
            decode(b"wa \r"),
            vec![
                k(KeyCode::Char('w'), 0, KeyKind::Press),
                k(KeyCode::Char('a'), 0, KeyKind::Press),
                k(KeyCode::Char(' '), 0, KeyKind::Press),
                k(KeyCode::Enter, 0, KeyKind::Press),
            ]
        );
        assert_eq!(decode(b"\x03"), vec![k(KeyCode::Char('c'), Mods::CTRL, KeyKind::Press)]);
    }

    #[test]
    fn legacy_arrows() {
        assert_eq!(
            decode(b"\x1b[A\x1bOB\x1b[1;2C"),
            vec![
                k(KeyCode::Up, 0, KeyKind::Press),
                k(KeyCode::Down, 0, KeyKind::Press),
                k(KeyCode::Right, Mods::SHIFT, KeyKind::Press),
            ]
        );
    }

    #[test]
    fn kitty_keyboard_events() {
        assert_eq!(
            decode(b"\x1b[119;1:1u\x1b[119;1:2u\x1b[119;1:3u\x1b[27u\x1b[1;1:3A\x1b[32;5u"),
            vec![
                k(KeyCode::Char('w'), 0, KeyKind::Press),
                k(KeyCode::Char('w'), 0, KeyKind::Repeat),
                k(KeyCode::Char('w'), 0, KeyKind::Release),
                k(KeyCode::Escape, 0, KeyKind::Press),
                k(KeyCode::Up, 0, KeyKind::Release),
                k(KeyCode::Char(' '), Mods::CTRL, KeyKind::Press),
            ]
        );
    }

    #[test]
    fn caps_lock_is_ignored_in_mods() {
        // caps_lock (64) + 1 = 65
        assert_eq!(decode(b"\x1b[97;65u"), vec![k(KeyCode::Char('a'), 0, KeyKind::Press)]);
    }

    #[test]
    fn sgr_mouse_cells_and_pixels() {
        let ev = decode(b"\x1b[<0;10;5M\x1b[<32;11;5M\x1b[<0;11;5m\x1b[<65;3;3M");
        assert_eq!(ev.len(), 4);
        match &ev[0] {
            InputEvent::Mouse(m) => {
                assert_eq!(m.kind, MouseKind::Press(MouseButton::Left));
                assert_eq!((m.col, m.row), (9, 4));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&ev[1], InputEvent::Mouse(m) if m.kind == MouseKind::Drag(MouseButton::Left)));
        assert!(matches!(&ev[2], InputEvent::Mouse(m) if m.kind == MouseKind::Release(Some(MouseButton::Left))));
        assert!(matches!(&ev[3], InputEvent::Mouse(m) if m.kind == MouseKind::ScrollDown));

        let mut d = Decoder::new();
        d.sgr_pixels = true;
        d.cell_px = (10, 20);
        d.feed(b"\x1b[<0;101;41M");
        match &d.drain()[0] {
            InputEvent::Mouse(m) => {
                assert_eq!(m.px, Some((100, 40)));
                assert_eq!((m.col, m.row), (10, 2));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn graphics_replies() {
        assert_eq!(
            decode(b"\x1b_Gi=31;OK\x1b\\\x1b_Gi=7,p=3;ENOENT:no such image\x1b\\"),
            vec![
                InputEvent::Reply(Reply::Graphics {
                    id: 31,
                    placement: None,
                    number: None,
                    message: "OK".into()
                }),
                InputEvent::Reply(Reply::Graphics {
                    id: 7,
                    placement: Some(3),
                    number: None,
                    message: "ENOENT:no such image".into()
                }),
            ]
        );
    }

    #[test]
    fn osc777_with_bel_and_st() {
        let ev = decode(b"\x1b]777;frame;v=1:op=event:seq=3:t=100:period=8333\x07\x1b]777;ink;v=1:op=cap:id=2:caps=1:mode=0\x1b\\");
        assert_eq!(ev.len(), 2);
        match &ev[0] {
            InputEvent::Reply(Reply::Osc777(f)) => {
                assert_eq!(f.selector, "frame");
                assert_eq!(f.get_u64("seq"), Some(3));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&ev[1], InputEvent::Reply(Reply::Osc777(f)) if f.selector == "ink"));
    }

    #[test]
    fn replies_to_queries() {
        let ev = decode(b"\x1b[?11u\x1b[?62;22c\x1b[6;36;18t\x1b[4;1800;3600t\x1b[?2026;2$y");
        assert_eq!(
            ev,
            vec![
                InputEvent::Reply(Reply::KeyboardFlags(11)),
                InputEvent::Reply(Reply::DeviceAttributes("62;22".into())),
                InputEvent::Reply(Reply::CellSize { width: 18, height: 36 }),
                InputEvent::Reply(Reply::WindowPixels { width: 3600, height: 1800 }),
                InputEvent::Reply(Reply::Mode { mode: 2026, value: 2 }),
            ]
        );
    }

    #[test]
    fn xtgettcap_reply() {
        // kitty-query-hdr_headroom = 1.5
        let name: String = "kitty-query-hdr_headroom".bytes().map(|b| format!("{b:02x}")).collect();
        let val: String = "1.5".bytes().map(|b| format!("{b:02x}")).collect();
        let seq = format!("\x1bP1+r{name}={val}\x1b\\");
        assert_eq!(
            decode(seq.as_bytes()),
            vec![InputEvent::Reply(Reply::Tcap {
                name: "kitty-query-hdr_headroom".into(),
                value: Some("1.5".into())
            })]
        );
    }

    #[test]
    fn sequences_split_across_reads() {
        let all = b"\x1b]777;tablet-pointer;v=1:op=event:kind=1:phase=2:device=3:t=5:contact=0:x=256:y=512\x1b\\\x1b[119;1:3u\xc3\xa9";
        for split in 1..all.len() {
            let mut d = Decoder::new();
            d.feed(&all[..split]);
            let mut ev = d.drain();
            d.feed(&all[split..]);
            ev.extend(d.drain());
            assert_eq!(ev.len(), 3, "split at {split}: {ev:?}");
            assert!(matches!(&ev[2], InputEvent::Key(k) if k.code == KeyCode::Char('é')));
        }
    }

    #[test]
    fn lone_escape_needs_flush() {
        let mut d = Decoder::new();
        d.feed(b"\x1b");
        assert!(d.drain().is_empty());
        assert!(d.has_pending_escape());
        assert_eq!(d.flush_lone_escape(), Some(k(KeyCode::Escape, 0, KeyKind::Press)));
        assert_eq!(d.pending_len(), 0);
    }

    #[test]
    fn pointer_shape_reply() {
        assert_eq!(decode(b"\x1b]22;1,0\x1b\\"), vec![InputEvent::Reply(Reply::Pointer("1,0".into()))]);
    }

    #[test]
    fn alt_keys_and_focus() {
        assert_eq!(
            decode(b"\x1bx\x1b[I\x1b[O"),
            vec![
                k(KeyCode::Char('x'), Mods::ALT, KeyKind::Press),
                InputEvent::Focus(true),
                InputEvent::Focus(false),
            ]
        );
    }

    #[test]
    fn interleaved_reply_does_not_eat_keys() {
        let ev = decode(b"w\x1b_Gi=5;OK\x1b\\d");
        assert_eq!(ev.len(), 3);
        assert!(matches!(ev[0], InputEvent::Key(k) if k.code == KeyCode::Char('w')));
        assert!(matches!(ev[2], InputEvent::Key(k) if k.code == KeyCode::Char('d')));
    }
}
