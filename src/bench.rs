//! `silkie-scramble bench`: reproducible measurements without a terminal.
//!
//! The renderer writes into memory, so what is measured is what the game
//! would send: bytes, commands, uploads and placements, and the time it
//! takes to produce them. How fast a particular terminal then draws it is
//! not measured here; the diagnostics panel shows that live.

use crate::app::show::Show;
use crate::render::layout::Layout;
use crate::render::upload::Uploader;
use crate::render::{FrameModel, RenderConfig, Renderer, SceneRef, SpriteSource};
use crate::sim::autopilot::Autopilot;
use crate::sim::{FenceStroke, Game, Session};
use crate::world::level;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn flock_cost(chicks: u32) -> (f64, f64) {
    let mut lvl = level::builtin(0).expect("level");
    lvl.chick_count = chicks;
    lvl.fox_count = 0;
    let mut g = Game::new(lvl, 0, Session::default(), 9);
    let hen = g.hen.pos();
    for (i, c) in g.chicks.iter_mut().enumerate() {
        // Start the whole brood around the hen.
        let a = i as f32 * 0.7;
        c.pos = hen + glam::Vec2::new(a.cos(), a.sin()) * (0.5 + (i % 7) as f32 * 0.15);
        c.prev_pos = c.pos;
        c.traits.curiosity = 0.0;
        c.join_brood();
    }
    let mut bot = Autopilot::new();
    bot.deliver_at = usize::MAX;
    let mut total = Duration::ZERO;
    let mut max = Duration::ZERO;
    let n = 600;
    for _ in 0..n {
        let i = bot.decide(&g);
        g.step(&i);
        g.drain_events();
        total += g.stats.last_flock;
        max = max.max(g.stats.last_flock);
    }
    (total.as_secs_f64() * 1e6 / n as f64, max.as_secs_f64() * 1e6)
}

#[derive(Default)]
struct RunStats {
    first_frame: usize,
    steady_bytes: Vec<usize>,
    placements: u64,
    build: Duration,
    fence_bytes: usize,
    small_resize_bytes: usize,
    resize_bytes: usize,
    resize_time: Duration,
}

fn render_run(vector: bool, temp_file: bool) -> anyhow::Result<RunStats> {
    let mut r = Renderer::new(RenderConfig {
        vector_scenery: vector,
        uploader: Uploader { temp_file, gamut_p3: false },
        sync_output: true,
        cell_alpha: vector,
        // Silkie with HDR images gets the glint overlay.
        hdr_boost: if vector { 3.0 } else { 1.0 },
        sprites: SpriteSource::Painted,
    })?;
    let lvl = level::builtin(0).expect("level");
    r.set_layout(Layout::new(200, 56, 18, 36, lvl.grid.width, lvl.grid.height));
    let scene = SceneRef {
        key: "bench".into(),
        level: Arc::new(lvl.clone()),
    };
    let mut g = Game::new(lvl, 0, Session::default(), 5);
    let mut bot = Autopilot::new();
    bot.use_fences = false;
    let mut show = Show::new();
    let mut st = RunStats::default();
    for frame in 0..1800u32 {
        let mut input = bot.decide(&g);
        if frame == 900 {
            input.strokes.push(FenceStroke {
                id: 1,
                points: vec![[3.5, 13.2], [3.5, 13.8]],
            });
        }
        g.step(&input);
        for e in g.drain_events() {
            show.on_event(&e);
        }
        show.update(1.0 / 60.0);
        let t = Instant::now();
        let m = FrameModel {
            scene: Some(scene.clone()),
            gates: Show::gates(&g),
            fences: Show::fences(&g),
            sprites: show.sprites(&g, 1.0),
            top: format!("score {}", g.session.score),
            bottom: format!("brood {}", g.brood_size()),
            ..Default::default()
        };
        let out = r.render(&m);
        st.build += t.elapsed();
        if frame == 0 {
            st.first_frame = out.len();
        } else if frame == 901 {
            st.fence_bytes = out.len();
        } else {
            st.steady_bytes.push(out.len());
        }
        st.placements += r.last.placements as u64;
    }
    let lvl = level::builtin(0).expect("level");
    let frame = |g: &Game, show: &mut Show| FrameModel {
        scene: Some(scene.clone()),
        gates: Show::gates(g),
        fences: Show::fences(g),
        sprites: show.sprites(g, 1.0),
        ..Default::default()
    };
    // A couple of columns wider: the same sprite step.
    r.set_layout(Layout::new(202, 56, 18, 36, lvl.grid.width, lvl.grid.height));
    st.small_resize_bytes = r.render(&frame(&g, &mut show)).len();
    let t = Instant::now();
    r.set_layout(Layout::new(240, 64, 18, 36, lvl.grid.width, lvl.grid.height));
    st.resize_bytes = r.render(&frame(&g, &mut show)).len();
    st.resize_time = t.elapsed();
    Ok(st)
}

fn kb(b: usize) -> String {
    format!("{:.1} KB", b as f64 / 1024.0)
}

pub fn run(chicks: u32) -> anyhow::Result<()> {
    println!("## Simulation\n");
    println!("Flocking cost per tick with the whole brood following the hen (600 ticks, release build recommended):\n");
    println!("| chicks | mean flock update (µs) | worst (µs) |");
    println!("|---|---|---|");
    let mut sizes = vec![8, 32, 64];
    if chicks > 64 {
        sizes.push(chicks);
    }
    for n in sizes {
        let (mean, max) = flock_cost(n);
        println!("| {n} | {mean:.1} | {max:.1} |");
    }
    println!("\n## Renderer output\n");
    println!("One autopilot game on level 1, 1800 frames at a 200x56 cell window of 18x36 px cells; then a resize by two columns, and one to 240x64.\n");
    println!("| configuration | first frame | mean steady frame | p95 steady frame | fence placed | small resize | big resize | big resize work | build per frame |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (name, vector, temp) in [
        ("kitty, inline PNG (remote)", false, false),
        ("kitty, temp files (local)", false, true),
        ("Silkie, vector scenery (remote)", true, false),
    ] {
        let s = render_run(vector, temp)?;
        let mut v = s.steady_bytes.clone();
        v.sort_unstable();
        let mean = v.iter().sum::<usize>() as f64 / v.len().max(1) as f64;
        let p95 = v.get(v.len() * 95 / 100).copied().unwrap_or(0);
        println!(
            "| {name} | {} | {:.0} B | {} B | {} B | {} | {} | {:.0} ms | {:.2} ms |",
            kb(s.first_frame),
            mean,
            p95,
            s.fence_bytes,
            kb(s.small_resize_bytes),
            kb(s.resize_bytes),
            s.resize_time.as_secs_f64() * 1000.0,
            s.build.as_secs_f64() * 1000.0 / 1800.0
        );
    }
    println!("\nWith temp files the image data travels through /tmp rather than the terminal stream, so the bytes column counts only the commands.");
    // The HDR glint overlay that Silkie also gets, sent with the effects
    // sheet when the first effect appears.
    let lvl = level::builtin(0).expect("level");
    let l = Layout::new(200, 56, 18, 36, lvl.grid.width, lvl.grid.height).expect("layout");
    let fx = crate::render::paint_sheet(crate::render::SheetId::Fx, crate::render::sprite_res(l.tile_px));
    let mut out = Vec::new();
    let up = Uploader { temp_file: false, gamut_p3: false };
    let sdr = up.rgba(&mut out, 1, fx.width, fx.height, &fx.rgba).bytes;
    let hdr = up.hdr(&mut out, 2, fx.width, fx.height, &crate::render::upload::hdr_glints(&fx.rgba, 3.0)).bytes;
    println!(
        "\nThe effects sheet is {} inline; its HDR glint overlay (Silkie with HDR images) adds {}.",
        kb(sdr as usize),
        kb(hdr as usize)
    );
    Ok(())
}
