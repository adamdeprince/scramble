//! Terminal text: the status lines and overlay boxes (menus, banners, the
//! diagnostics panel).

use super::layout::Layout;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Center,
    /// Centred horizontally, near the bottom of the field.
    Low,
    TopRight,
    TopLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Normal,
    Heading,
    Dim,
    Accent,
    Selected,
    Good,
    Bad,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub style: Style,
}

impl Line {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Line { text: text.into(), style }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub anchor: Anchor,
    pub lines: Vec<Line>,
    /// Background opacity in percent where the terminal supports cell
    /// alpha; opaque elsewhere.
    pub alpha: u8,
    pub min_width: u32,
}

impl Overlay {
    pub fn new(anchor: Anchor, lines: Vec<Line>) -> Self {
        Overlay {
            anchor,
            lines,
            alpha: 72,
            min_width: 0,
        }
    }
}

pub const BG: (u8, u8, u8) = (34, 44, 26);

fn style_sgr(s: Style) -> &'static str {
    match s {
        Style::Normal => "\x1b[38;2;244;236;214m",
        Style::Heading => "\x1b[1;38;2;255;214;102m",
        Style::Dim => "\x1b[38;2;170;178;150m",
        Style::Accent => "\x1b[38;2;255;179;128m",
        Style::Selected => "\x1b[1;38;2;34;44;26;48;2;255;214;102m",
        Style::Good => "\x1b[38;2;156;222;120m",
        Style::Bad => "\x1b[38;2;255;128;112m",
    }
}

/// Visible width of a string (escape sequences do not count; the game only
/// uses single-width characters).
pub fn width(s: &str) -> usize {
    let mut n = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        n += 1;
    }
    n
}

/// Truncates a styled string to `cols` visible characters.
/// Breaks plain text into lines of at most `cols` columns at spaces; a word
/// longer than a line is cut to fit.
pub fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && width(&line) + 1 + width(word) > cols {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(|l| fit(&l, cols)).collect()
}

pub fn fit(s: &str, cols: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut n = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            if chars.peek() == Some(&'[') {
                out.push(chars.next().expect("peeked"));
                for c in chars.by_ref() {
                    out.push(c);
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        if n >= cols {
            continue;
        }
        out.push(c);
        n += 1;
    }
    out
}

/// Renders an overlay box; returns the bytes and the box's cell rectangle
/// (col, row, width, height).
pub fn render_overlay(o: &Overlay, l: &Layout, cell_alpha: bool) -> (String, (u32, u32, u32, u32)) {
    let inner = o
        .lines
        .iter()
        .map(|ln| width(&ln.text))
        .max()
        .unwrap_or(0)
        .max(o.min_width as usize) as u32;
    let w = (inner + 4).min(l.cols);
    let h = (o.lines.len() as u32 + 2).min(l.rows.saturating_sub(2));
    let field_mid_row = l.field_row + l.field_rows / 2;
    let (col, row) = match o.anchor {
        Anchor::Center => (
            l.cols.saturating_sub(w) / 2,
            field_mid_row.saturating_sub(h / 2).max(1),
        ),
        Anchor::Low => (
            l.cols.saturating_sub(w) / 2,
            (l.field_row + l.field_rows).saturating_sub(h + 1).max(1),
        ),
        Anchor::TopRight => (l.cols.saturating_sub(w + 1), l.field_row.max(1)),
        Anchor::TopLeft => (l.field_col.max(1), l.field_row.max(1)),
    };
    let mut s = String::new();
    let bg = format!("\x1b[48;2;{};{};{}m", BG.0, BG.1, BG.2);
    let alpha = if cell_alpha { format!("\x1b[70:{}m", o.alpha) } else { String::new() };
    for i in 0..h {
        let _ = write!(s, "\x1b[{};{}H\x1b[0m{bg}{alpha}", row + i + 1, col + 1);
        let content = if i == 0 || i == h - 1 {
            String::new()
        } else {
            o.lines.get(i as usize - 1).map_or(String::new(), |ln| ln.text.clone())
        };
        let style = if i == 0 || i == h - 1 {
            Style::Normal
        } else {
            o.lines.get(i as usize - 1).map_or(Style::Normal, |ln| ln.style)
        };
        let vis = width(&content) as u32;
        let avail = w.saturating_sub(4);
        let content = fit(&content, avail as usize);
        let vis = vis.min(avail);
        let left = (avail - vis) / 2;
        let right = avail - vis - left;
        let _ = write!(
            s,
            "  {}{}{}\x1b[0m{bg}{alpha}{}  ",
            " ".repeat(left as usize),
            style_sgr(style),
            content,
            " ".repeat(right as usize)
        );
    }
    s.push_str("\x1b[0m");
    (s, (col, row, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_ignores_escapes() {
        assert_eq!(width("\x1b[1;33mScore\x1b[0m 10"), 8);
        assert_eq!(fit("\x1b[31mabcdef\x1b[0m", 3), "\x1b[31mabc\x1b[0m");
    }

    #[test]
    fn overlay_is_centred_and_sized() {
        let l = Layout::new(100, 40, 9, 18, 32, 18).unwrap();
        let o = Overlay::new(Anchor::Center, vec![Line::new("Paused", Style::Heading), Line::new("P to resume", Style::Normal)]);
        let (s, (col, row, w, h)) = render_overlay(&o, &l, true);
        assert_eq!(h, 4);
        assert_eq!(w, 15);
        assert_eq!(col, (100 - 15) / 2);
        assert!(row > 1);
        assert!(s.contains("\x1b[70:72m"), "translucent background on Silkie");
        let (s2, _) = render_overlay(&o, &l, false);
        assert!(!s2.contains("\x1b[70:"));
    }

    #[test]
    fn wrap_breaks_at_spaces() {
        assert_eq!(wrap("needs a bigger window", 10), vec!["needs a", "bigger", "window"]);
        assert_eq!(wrap("needs a bigger window", 40), vec!["needs a bigger window"]);
        assert_eq!(wrap("unbreakable", 4), vec!["unbr"]);
    }
}
