//! Silkie Scramble: gather the chicks, mind the fox, bring the brood home.

use clap::{Parser, Subcommand};
use silkie_scramble::app::{self, Options};
use silkie_scramble::sim::autopilot::Autopilot;
use silkie_scramble::sim::replay::Recording;
use silkie_scramble::sim::{Game, Phase, Session};
use silkie_scramble::world::level;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "A maze-chase arcade game about keeping an unruly flock of Silkie chicks alive.")]
struct Cli {
    /// Random seed (the same seed and inputs replay the same game).
    #[arg(long)]
    seed: Option<u64>,
    /// Level to start on (1-based).
    #[arg(long, default_value_t = 1)]
    level: usize,
    /// Show the diagnostics panel from the start (toggle with F2 or `).
    #[arg(long)]
    diagnostics: bool,
    /// Use only what plain kitty offers, even when the terminal has more.
    #[arg(long)]
    kitty_only: bool,
    /// Draw the scenery as bitmaps even where vector images are available.
    #[arg(long)]
    no_vector: bool,
    /// Do not let the terminal ink pen strokes.
    #[arg(long)]
    no_ink: bool,
    /// Do not ask for the terminal's low-latency mode.
    #[arg(long)]
    no_latency: bool,
    /// Pace frames by timer even where presentation reports exist.
    #[arg(long)]
    no_frame_feedback: bool,
    /// No haptic feedback.
    #[arg(long)]
    no_haptics: bool,
    /// No HDR glints, even where the terminal has the headroom.
    #[arg(long)]
    no_hdr: bool,
    /// Never send images through temporary files (as over SSH).
    #[arg(long)]
    no_tempfile: bool,
    /// Record the inputs of the game to this file, for `replay`.
    #[arg(long)]
    record: Option<PathBuf>,
    /// Print what the terminal supports and exit.
    #[arg(long)]
    report: bool,
    /// Let the autopilot play the hen (a demo).
    #[arg(long)]
    autoplay: bool,
    /// Use the PNG sprite sheets in assets/sprites (resampled) instead of
    /// painting sprites at the exact size; for trying out replacement art.
    #[arg(long)]
    png_sprites: bool,
    /// Write a log here (the terminal itself is busy with the game).
    #[arg(long)]
    log: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Re-runs a recording without a terminal and checks it ends in the same
    /// state.
    Replay { file: PathBuf },
    /// Lets the autopilot play every level without a terminal and prints how
    /// it went.
    Headless {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Seconds of play per level at most.
        #[arg(long, default_value_t = 240)]
        seconds: u32,
    },
    /// Measures simulation and renderer costs without a terminal.
    Bench {
        /// Chicks for the flocking stress test.
        #[arg(long, default_value_t = 200)]
        chicks: u32,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Some(path) = &cli.log {
        if let Ok(file) = std::fs::File::create(path) {
            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
                .init();
        }
    }
    let result = match cli.command {
        Some(Command::Replay { file }) => replay(&file),
        Some(Command::Headless { seed, seconds }) => {
            headless(seed, seconds);
            Ok(())
        }
        Some(Command::Bench { chicks }) => silkie_scramble::bench::run(chicks),
        None => app::run(Options {
            seed: cli.seed,
            level: cli.level.saturating_sub(1),
            diagnostics: cli.diagnostics,
            kitty_only: cli.kitty_only,
            no_vector: cli.no_vector,
            no_ink: cli.no_ink,
            no_latency: cli.no_latency,
            no_frame_feedback: cli.no_frame_feedback,
            no_haptics: cli.no_haptics,
            no_hdr: cli.no_hdr,
            no_tempfile: cli.no_tempfile,
            record: cli.record,
            report: cli.report,
            autoplay: cli.autoplay,
            png_sprites: cli.png_sprites,
        }),
    };
    if let Err(e) = result {
        eprintln!("silkie-scramble: {e:#}");
        std::process::exit(1);
    }
}

fn replay(file: &std::path::Path) -> anyhow::Result<()> {
    let rec = Recording::load(file)?;
    let lvl = level::builtin(rec.level_index).ok_or_else(|| anyhow::anyhow!("recording names level {} which does not exist", rec.level_index))?;
    let g = rec.replay(lvl);
    let hash = g.state_hash();
    println!(
        "{}: {} ticks, {} input changes, score {}, final state {:016x}",
        rec.level_name,
        rec.ticks,
        rec.inputs.len(),
        g.session.score,
        hash
    );
    match rec.final_hash {
        Some(h) if h == hash => println!("replay matches the recorded game"),
        Some(h) => anyhow::bail!("replay diverged: recorded {h:016x}"),
        None => println!("(the recording has no final state to compare)"),
    }
    Ok(())
}

fn headless(seed: u64, seconds: u32) {
    println!("| level | outcome | home | lost | time (s) | score | mean tick (µs) | max tick (µs) |");
    println!("|---|---|---|---|---|---|---|---|");
    for i in 0..level::BUILTIN.len() {
        let lvl = level::builtin(i).expect("built-in level");
        let name = lvl.name.clone();
        let mut g = Game::new(lvl, i, Session::default(), seed);
        let mut bot = Autopilot::new();
        let mut total = std::time::Duration::ZERO;
        let mut max = std::time::Duration::ZERO;
        let mut ticks = 0u32;
        while g.time < seconds as f32 && g.outcome().is_none() && ticks < seconds * 60 + 600 {
            let input = bot.decide(&g);
            g.step(&input);
            g.drain_events();
            total += g.stats.last_tick;
            max = max.max(g.stats.last_tick);
            ticks += 1;
        }
        let outcome = match g.phase {
            Phase::Cleared { .. } => "cleared",
            Phase::Failed { .. } => "too few home",
            Phase::GameOver => "game over",
            _ => "time up",
        };
        println!(
            "| {} {} | {} | {}/{} | {} | {:.0} | {} | {:.1} | {:.1} |",
            i + 1,
            name,
            outcome,
            g.delivered,
            g.level.chick_count,
            g.lost,
            g.time,
            g.session.score,
            total.as_secs_f64() * 1e6 / ticks.max(1) as f64,
            max.as_secs_f64() * 1e6
        );
    }
}
