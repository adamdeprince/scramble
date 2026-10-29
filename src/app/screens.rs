//! Status lines, banners, menus, the diagnostics panel and the capability
//! report.

use super::control::PAUSE_MENU;
use super::{App, Screen};
use crate::render::FrameModel;
use crate::render::text::{Anchor, Line, Overlay, Style, width};
use crate::sim::{Game, Phase};
use crate::term::caps::Capabilities;

const GOLD: &str = "\x1b[1;38;2;255;214;102m";
const CREAM: &str = "\x1b[38;2;244;236;214m";
const DIM: &str = "\x1b[38;2;150;160;130m";
const RED: &str = "\x1b[38;2;255;105;120m";
const WOOD: &str = "\x1b[38;2;214;150;90m";
const GREEN: &str = "\x1b[38;2;156;222;120m";
const RESET: &str = "\x1b[0m";

fn spread(left: &str, right: &str, cols: usize) -> String {
    let gap = cols.saturating_sub(width(left) + width(right)).max(1);
    format!("{left}{}{right}", " ".repeat(gap))
}

fn status_top(g: Option<&Game>, best: u32, cols: usize, title: bool) -> String {
    let left = match g {
        Some(g) if !title => format!(
            " {GOLD}SILKIE SCRAMBLE{RESET}  {DIM}Level {}{RESET} {CREAM}{}{RESET}",
            g.level_index + 1,
            g.level.name
        ),
        _ => format!(" {GOLD}SILKIE SCRAMBLE{RESET}  {DIM}a maze chase about an unruly brood{RESET}"),
    };
    let right = match g {
        Some(g) if !title => format!(
            "{DIM}SCORE{RESET} {GOLD}{:06}{RESET}   {DIM}BEST{RESET} {CREAM}{:06}{RESET} ",
            g.session.score,
            best.max(g.session.score)
        ),
        _ => format!("{DIM}BEST{RESET} {CREAM}{best:06}{RESET} "),
    };
    spread(&left, &right, cols)
}

fn status_bottom(g: &Game, cols: usize) -> String {
    let hearts: String = "♥".repeat(g.session.lives.min(9) as usize);
    let brood = g.brood_size();
    let total = g.level.chick_count;
    let supply = g.fence_supply.floor().max(0.0) as usize;
    let max = g.level.fence_supply as usize;
    let fence: String = format!("{WOOD}{}{DIM}{}{RESET}", "▮".repeat(supply), "▯".repeat(max.saturating_sub(supply)));
    let need = g.needed();
    let home_color = if g.delivered >= need { GREEN } else { CREAM };
    let left = format!(
        " {RED}{hearts}{RESET}  {DIM}brood{RESET} {CREAM}{brood}{RESET}  {DIM}home{RESET} {home_color}{}/{total}{RESET} {DIM}(need {need}){RESET}  {DIM}lost{RESET} {CREAM}{}{RESET}  {DIM}fence{RESET} {fence}",
        g.delivered, g.lost
    );
    let right = format!("{DIM}space: fence/latch · draw: fence · p: pause{RESET} ");
    spread(&left, &right, cols)
}

pub fn decorate(app: &App, m: &mut FrameModel) {
    let cols = app.renderer.layout.map_or(80, |l| l.cols as usize);
    let best = app.scores.best();
    match &app.screen {
        Screen::Title { .. } => {
            m.top = status_top(None, best, cols, true);
            m.bottom = spread(
                &format!(" {CREAM}Enter{RESET}{DIM}: play{RESET}   {CREAM}C{RESET}{DIM}: terminal features{RESET}   {CREAM}Q{RESET}{DIM}: quit{RESET}"),
                &format!("{DIM}arrows/WASD move · space or draw to fence{RESET} "),
                cols,
            );
            let mut lines = vec![
                Line::new("Gather the chicks. Mind the fox. Bring them home.", Style::Normal),
                Line::new("", Style::Normal),
                Line::new("Press Enter to play", Style::Heading),
            ];
            let enhanced: Vec<&str> = app
                .caps
                .summary()
                .into_iter()
                .skip(2)
                .filter(|(_, on)| *on)
                .map(|(n, _)| n)
                .collect();
            if !enhanced.is_empty() && !app.opts.kitty_only {
                lines.push(Line::new("", Style::Normal));
                lines.push(Line::new(format!("this terminal adds: {}", enhanced.join(" · ")), Style::Dim));
            }
            m.overlays.push(Overlay::new(Anchor::Low, lines));
            if !app.scores.entries.is_empty() {
                let mut hs = vec![Line::new("Best broods", Style::Heading)];
                for (i, e) in app.scores.entries.iter().take(5).enumerate() {
                    hs.push(Line::new(format!("{}. {:<12} {:>7}", i + 1, e.name, e.score), Style::Normal));
                }
                let mut o = Overlay::new(Anchor::TopLeft, hs);
                o.alpha = 60;
                m.overlays.push(o);
            }
        }
        Screen::Report => {
            m.top = status_top(None, best, cols, true);
            m.bottom = format!(" {DIM}any key: back{RESET}");
            let lines: Vec<Line> = capability_report(&app.caps)
                .into_iter()
                .enumerate()
                .map(|(i, s)| Line::new(s, if i == 0 { Style::Heading } else { Style::Normal }))
                .collect();
            let mut o = Overlay::new(Anchor::Center, lines);
            o.alpha = 92;
            m.overlays.push(o);
        }
        _ => {
            let Some(g) = &app.game else { return };
            m.top = status_top(Some(g), best, cols, false);
            m.bottom = status_bottom(g, cols);
            match &app.screen {
                Screen::Paused { menu } => {
                    let mut lines = vec![Line::new("Paused", Style::Heading), Line::new("", Style::Normal)];
                    for (i, item) in PAUSE_MENU.iter().enumerate() {
                        lines.push(Line::new(
                            format!("  {item}  "),
                            if i == *menu { Style::Selected } else { Style::Normal },
                        ));
                    }
                    let mut o = Overlay::new(Anchor::Center, lines);
                    o.min_width = 24;
                    m.overlays.push(o);
                }
                Screen::GameOver { name, rank } => {
                    let mut lines = vec![
                        Line::new("Game over", Style::Heading),
                        Line::new(format!("Score {}  ·  chicks saved {}", g.session.score, g.session.chicks_saved), Style::Normal),
                        Line::new("", Style::Normal),
                    ];
                    match rank {
                        Some(r) => {
                            lines.push(Line::new(format!("A new best brood, number {}!", r + 1), Style::Good));
                            lines.push(Line::new(format!("Your name: {name}_"), Style::Accent));
                            lines.push(Line::new("Enter to save", Style::Dim));
                        }
                        None => lines.push(Line::new("Enter: back to the title", Style::Dim)),
                    }
                    let mut o = Overlay::new(Anchor::Center, lines);
                    o.min_width = 30;
                    m.overlays.push(o);
                }
                _ => phase_banner(g, m),
            }
        }
    }
}

fn phase_banner(g: &Game, m: &mut FrameModel) {
    match g.phase {
        Phase::Ready { .. } => {
            let mut lines = vec![
                Line::new(format!("Level {} · {}", g.level_index + 1, g.level.name), Style::Heading),
                Line::new(g.level.tagline.clone(), Style::Normal),
                Line::new(format!("Bring home at least {} of {} chicks", g.needed(), g.level.chick_count), Style::Accent),
            ];
            if g.level_index == 0 {
                lines.push(Line::new("", Style::Normal));
                lines.push(Line::new("Walk into chicks to gather them; lead them to the coop door.", Style::Dim));
                lines.push(Line::new("Space drops a fence behind you or works a latched gate.", Style::Dim));
                lines.push(Line::new("Or draw a fence across a path with the mouse or a pen.", Style::Dim));
            }
            let mut o = Overlay::new(Anchor::Center, lines);
            o.alpha = 78;
            m.overlays.push(o);
        }
        Phase::Caught { .. } => {
            let left = g.session.lives.saturating_sub(1);
            let msg = if left == 0 { "Caught! That was the last life.".to_string() } else { format!("Caught! {left} lives left. The brood has scattered.") };
            m.overlays.push(Overlay::new(Anchor::Low, vec![Line::new(msg, Style::Bad)]));
        }
        Phase::Cleared { .. } => m.overlays.push(Overlay::new(
            Anchor::Center,
            vec![
                Line::new("Yard cleared!", Style::Heading),
                Line::new(format!("{} of {} chicks home", g.delivered, g.level.chick_count), Style::Good),
                Line::new(if g.lost == 0 { "Not one lost: perfect brood bonus" } else { "" }, Style::Accent),
            ],
        )),
        Phase::Failed { .. } => m.overlays.push(Overlay::new(
            Anchor::Center,
            vec![
                Line::new("Too few made it home", Style::Bad),
                Line::new(format!("{} home, {} needed. Try the yard again.", g.delivered, g.needed()), Style::Normal),
            ],
        )),
        _ => {}
    }
}

fn mark(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}

/// What the terminal answered, for `--report`, the title screen's C key
/// and the diagnostics panel.
pub fn capability_report(c: &Capabilities) -> Vec<String> {
    let mut v = vec!["Terminal capabilities (as answered by the terminal)".to_string()];
    v.push(format!("graphics protocol: {}   temp-file medium: {}", mark(c.graphics), mark(c.temp_file)));
    v.push(format!(
        "kitty keyboard: {}   synchronized output: {}   SGR pixel mouse: {}",
        c.kitty_keyboard.map_or("no".to_string(), |f| format!("yes (flags {f})")),
        mark(c.sync_output),
        mark(c.sgr_pixels)
    ));
    v.push(format!(
        "cell: {}   window: {}",
        c.cell_px.map_or("?".into(), |(w, h)| format!("{w}x{h} px")),
        c.window_px.map_or("?".into(), |(w, h)| format!("{w}x{h} px"))
    ));
    match &c.listing {
        Some(l) => v.push(format!(
            "OSC 777 extensions: {}",
            l.iter().map(|(k, ver)| format!("{k}.{ver}")).collect::<Vec<_>>().join(" ")
        )),
        None => v.push("OSC 777 extensions: none listed".into()),
    }
    v.push(format!(
        "vector images: {}   HDR images: {}   wide-gamut images: {}",
        mark(c.vector),
        mark(c.hdr_images),
        mark(c.wide_gamut_images)
    ));
    v.push(format!(
        "pointer shapes: crosshair {}   image pointers {}",
        mark(c.pointer_crosshair),
        mark(c.pointer_image)
    ));
    for (name, s) in &c.selectors {
        let fields: Vec<String> = s
            .fields
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "v" | "op" | "id"))
            .map(|(k, val)| format!("{k}={val}"))
            .collect();
        v.push(format!("  {name}: {}", fields.join(" ")));
    }
    if let Some(d) = &c.display {
        v.push(format!(
            "display: {} {} {}-bit gamut={} headroom={} scale={} refresh={} ppi={}",
            d.kind,
            d.color,
            d.bits,
            d.gamut,
            d.headroom,
            d.scale,
            if d.period_us > 0 { format!("{:.1} Hz", 1e6 / d.period_us as f64) } else { "?".into() },
            d.ppi
        ));
    }
    for (what, d) in &c.timings {
        v.push(format!("discovery {what}: {:.1} ms", d.as_secs_f64() * 1000.0));
    }
    if c.timed_out {
        v.push("(some queries timed out)".into());
    }
    v
}
