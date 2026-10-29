//! Headless simulation tests with fixed seeds.

use super::autopilot::Autopilot;
use super::chick::{ChickState, FleeMode};
use super::predator::{PredState, Target};
use super::replay::Recording;
use super::*;
use crate::world::level::{self, Level};
use crate::world::{Dir, Mover, TilePos};

fn level(map: &str, header: &str) -> Level {
    Level::parse(&format!("{header}\nmap:\n{map}")).expect("test level parses")
}

fn playing(mut g: Game) -> Game {
    while !matches!(g.phase, Phase::Playing) {
        g.step(&TickInput::default());
    }
    g.drain_events();
    g
}

fn run(g: &mut Game, ticks: u32, input: impl Fn(&Game) -> TickInput) {
    for _ in 0..ticks {
        let i = input(g);
        g.step(&i);
    }
}

const CORRIDORS: &str = "\
###########
#H.......c#
#.#######.#
#.#######.#
#.........#
#D#########
###########";

#[test]
fn simulation_is_deterministic() {
    let lvl = level::builtin(0).unwrap();
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let mut g = Game::new(lvl.clone(), 0, Session::default(), 42);
        let mut bot = Autopilot::new();
        for _ in 0..3000 {
            let i = bot.decide(&g);
            g.step(&i);
            g.drain_events();
        }
        hashes.push(g.state_hash());
    }
    assert_eq!(hashes[0], hashes[1]);
    let mut other = Game::new(lvl, 0, Session::default(), 43);
    let mut bot = Autopilot::new();
    for _ in 0..3000 {
        let i = bot.decide(&other);
        other.step(&i);
    }
    assert_ne!(hashes[0], other.state_hash(), "different seeds should play out differently");
}

#[test]
fn recordings_replay_exactly() {
    let lvl = level::builtin(0).unwrap();
    let mut g = Game::new(lvl.clone(), 0, Session::default(), 7);
    let mut rec = Recording::new(&g);
    let mut bot = Autopilot::new();
    for _ in 0..2400 {
        let mut i = bot.decide(&g);
        if g.tick % 400 == 200 {
            i.strokes.push(FenceStroke {
                id: g.tick,
                points: vec![[3.2, 4.5], [8.7, 4.5]],
            });
        }
        g.step(&i);
        rec.push(g.tick, &i);
        g.drain_events();
    }
    rec.finish(&g);
    let replayed = rec.replay(lvl);
    assert_eq!(Some(replayed.state_hash()), rec.final_hash);
    // And through JSON.
    let dir = std::env::temp_dir().join(format!("silkie-scramble-rec-{}.json", std::process::id()));
    rec.save(&dir).unwrap();
    let loaded = Recording::load(&dir).unwrap();
    std::fs::remove_file(&dir).ok();
    assert_eq!(loaded, rec);
}

#[test]
fn hen_follows_corridors_and_stops_at_walls() {
    let mut g = playing(Game::new(level(CORRIDORS, "foxes: 0"), 0, Session::default(), 1));
    run(&mut g, 120, |_| TickInput {
        dir: Some(Dir::Up),
        ..Default::default()
    });
    assert_eq!(g.hen.mover.tile(), TilePos::new(1, 1), "cannot walk into the hedge");
    run(&mut g, 60, |_| TickInput {
        dir: Some(Dir::Right),
        ..Default::default()
    });
    let p = g.hen.pos();
    assert!(p.x > 4.0 && (p.y - 1.5).abs() < 1e-3, "{p}");
    // Press down early: the turn is buffered until the corridor opens.
    run(&mut g, 200, |_| TickInput {
        dir: Some(Dir::Down),
        ..Default::default()
    });
    assert_eq!(g.hen.mover.tile().x, 9);
    assert!(g.hen.mover.tile().y >= 3);
}

/// Gathers every chick of a fox-free map, walks the brood around corners,
/// and checks the procession: nobody inside a wall, nobody on top of a
/// sibling for long, and everybody still with the hen at the end.
#[test]
fn brood_follows_around_corners_without_overlap() {
    let map = "\
##############
#H...........#
#.##########.#
#.##########.#
#.....ccccc..#
######.#######
######D#######
##############";
    let mut g = playing(Game::new(level(map, "foxes: 0\nchicks: 5"), 0, Session::default(), 3));
    for (i, c) in g.chicks.iter_mut().enumerate() {
        c.pos = TilePos::new(2 + i as i32, 1).center();
        c.prev_pos = c.pos;
        c.traits.curiosity = 0.0;
        c.join_brood();
    }
    // Clockwise around the block and back.
    let route = [Dir::Right, Dir::Down, Dir::Left, Dir::Up];
    let mut overlaps = 0;
    let mut samples = 0;
    let mut leg = 0;
    for tick in 0..2400 {
        let here = g.hen.mover.tile();
        let corner = matches!(
            (leg % 4, here.x, here.y),
            (0, 12, 1) | (1, 12, 4) | (2, 1, 4) | (3, 1, 1)
        );
        if corner && g.hen.mover.dir.is_none() {
            leg += 1;
        }
        g.step(&TickInput {
            dir: Some(route[leg % 4]),
            ..Default::default()
        });
        for c in &g.chicks {
            assert!(
                !g.topo.circle_blocked(c.pos, super::rules::CHICK_RADIUS - 0.02, Mover::Chick),
                "tick {tick}: chick {} inside a wall at {}",
                c.id,
                c.pos
            );
        }
        if tick > 120 {
            for (i, a) in g.chicks.iter().enumerate() {
                for b in &g.chicks[i + 1..] {
                    samples += 1;
                    if a.pos.distance(b.pos) < 0.18 {
                        overlaps += 1;
                    }
                }
            }
        }
    }
    assert!(leg >= 4, "the hen went round the block ({leg} legs)");
    assert!(overlaps * 50 < samples, "chicks overlapped {overlaps} of {samples} samples");
    let hen = g.hen.pos();
    for c in &g.chicks {
        assert!(c.in_brood(), "chick {} left the brood: {:?}", c.id, c.state);
        let d = g.nav.hen_field.get(TilePos::of(c.pos));
        assert!(d <= 6, "chick {} is {d} tiles behind", c.id);
    }
    let _ = hen;
}

#[test]
fn walking_into_wild_chicks_gathers_them() {
    let mut g = playing(Game::new(level(CORRIDORS, "foxes: 0"), 0, Session::default(), 5));
    // The chick spawns at the far end of the top corridor.
    run(&mut g, 300, |_| TickInput {
        dir: Some(Dir::Right),
        ..Default::default()
    });
    assert!(g.chicks[0].in_brood(), "{:?}", g.chicks[0].state);
    assert!(g.drain_events().iter().any(|e| matches!(e, SimEvent::ChickJoined { .. })));
}

#[test]
fn delivering_a_brood_scores_a_batch_and_clears_the_level() {
    let map = "\
#########
#H......#
#D.ccc..#
#.......#
#########";
    let mut g = playing(Game::new(level(map, "foxes: 0\nchicks: 3"), 0, Session::default(), 9));
    for c in g.chicks.iter_mut() {
        c.join_brood();
    }
    let mut bot = Autopilot::new();
    let mut scored = None;
    for _ in 0..1200 {
        let i = bot.decide(&g);
        g.step(&i);
        for e in g.drain_events() {
            if let SimEvent::BatchScored { count, points, .. } = e {
                scored = Some((count, points));
            }
        }
        if matches!(g.phase, Phase::Cleared { .. }) {
            break;
        }
    }
    assert_eq!(scored, Some((3, 450)), "three chicks at once: 100 x 3 x 1.5");
    assert!(matches!(g.phase, Phase::Cleared { .. }), "{:?}", g.phase);
    assert_eq!(g.delivered, 3);
}

#[test]
fn fox_cannot_see_through_hedges() {
    let map = "\
#########
#H..#..F#
#...#...#
#.......#
#D###.#c#
#########";
    let mut g = playing(Game::new(level(map, "foxes: 1"), 0, Session::default(), 2));
    // Put the fox facing the hen across the hedge.
    g.predators[0].state = PredState::Prowl {
        goal: TilePos::new(7, 1),
        pause: 100.0,
    };
    g.predators[0].mover.facing = Dir::Left;
    run(&mut g, 30, |_| TickInput::default());
    assert!(matches!(g.predators[0].state, PredState::Prowl { .. }), "{:?}", g.predators[0].state);
    // Now in plain view down the bottom corridor.
    g.hen.mover.place(TilePos::new(2, 3));
    g.predators[0].mover.place(TilePos::new(5, 3));
    g.predators[0].mover.facing = Dir::Left;
    run(&mut g, 5, |_| TickInput::default());
    assert!(
        matches!(g.predators[0].state, PredState::Alert { target: Target::Hen, .. }),
        "{:?}",
        g.predators[0].state
    );
}

#[test]
fn fox_hunts_the_hen_down() {
    // The only chick is safe behind the coop door, so the hen is the prey.
    let map = "\
###########
#H........#
#.###.###.#
#.........#
#D#######F#
#c#########
###########";
    let lvl = level(map, "foxes: 1\ncunning: 3");
    let mut g = playing(Game::new(lvl, 0, Session::default(), 11));
    g.hen.mover.place(TilePos::new(3, 1));
    let mut caught = false;
    for _ in 0..1800 {
        g.step(&TickInput::default());
        if g.drain_events().iter().any(|e| matches!(e, SimEvent::HenCaught { .. })) {
            caught = true;
            break;
        }
    }
    assert!(caught, "a standing hen is eventually found and caught");
}

#[test]
fn closing_a_gate_reroutes_the_fox() {
    let map = "\
###########
#H...L...F#
#.#######.#
#.........#
#D#####c###
###########";
    let mut g = playing(Game::new(level(map, "foxes: 1"), 0, Session::default(), 4));
    let gate = g.gates[0].tile;
    assert!(!g.gates[0].closed);
    assert_eq!(gate::latch_in_reach(&g), None, "too far away at the start");
    // The hen stands next to the latch and closes it.
    g.hen.mover.place(TilePos::new(4, 1));
    assert_eq!(gate::latch_in_reach(&g), Some(0));
    g.step(&TickInput {
        action: true,
        ..Default::default()
    });
    assert!(g.gates[0].closed);
    assert!(!g.topo.passable(gate, Mover::Fox));
    let route = crate::world::nav::route(&g.topo, TilePos::new(8, 1), TilePos::new(3, 1), Mover::Fox).unwrap();
    assert!(!route.contains(&gate));
    assert!(route.len() > 6, "the long way round");
}

#[test]
fn fences_never_cut_off_chicks_or_the_coop() {
    let map = "\
###########
#H........#
#.#######.#
#.#######.#
#D.......c#
###########";
    let mut g = playing(Game::new(level(map, "foxes: 0\nfence: 10"), 0, Session::default(), 6));
    // A loop: blocking one side is fine...
    let (ok, rej) = fence::plan(&g, &[TilePos::new(5, 1)]);
    assert_eq!(ok, vec![TilePos::new(5, 1)]);
    assert!(rej.is_empty());
    // ...but blocking both sides of the loop would strand the chick.
    let (ok, rej) = fence::plan(&g, &[TilePos::new(5, 1), TilePos::new(5, 4)]);
    assert_eq!(ok, vec![TilePos::new(5, 1)]);
    assert_eq!(rej, vec![(TilePos::new(5, 4), FenceReject::WouldTrapBrood)]);
    // Nobody can be fenced in where they stand.
    let (ok, rej) = fence::plan(&g, &[TilePos::new(1, 1)]);
    assert!(ok.is_empty());
    assert_eq!(rej[0].1, FenceReject::Occupied);
    // Building really changes the topology and spends supply.
    let before = g.fence_supply;
    g.step(&TickInput {
        strokes: vec![FenceStroke {
            id: 1,
            points: vec![[5.5, 0.6], [5.5, 1.9]],
        }],
        ..Default::default()
    });
    assert!(!g.topo.passable(TilePos::new(5, 1), Mover::Hen));
    assert!(g.fence_supply < before);
    assert!(g.drain_events().iter().any(|e| matches!(e, SimEvent::FenceBuilt { stroke: Some(1), .. })));
}

#[test]
fn fences_cannot_pen_a_fox() {
    let map = "\
##############################
#H...........................#
#.##########################.#
#............................#
#D######################F#c###
##############################";
    let mut g = playing(Game::new(level(map, "foxes: 1\nfence: 10"), 0, Session::default(), 8));
    g.predators[0].mover.place(TilePos::new(24, 4));
    // Sealing the den's only exit would leave the fox one tile of yard.
    let (ok, rej) = fence::plan(&g, &[TilePos::new(24, 3)]);
    assert!(ok.is_empty());
    assert_eq!(rej, vec![(TilePos::new(24, 3), FenceReject::WouldPenPredator)]);
}

#[test]
fn space_drops_a_hurdle_behind_the_brood() {
    let map = "\
############
#H.........#
#.########.#
#..........#
#D#######c##
############";
    let mut g = playing(Game::new(level(map, "foxes: 0"), 0, Session::default(), 6));
    g.hen.mover.place(TilePos::new(6, 1));
    g.hen.mover.facing = Dir::Right;
    // Two chicks right behind her.
    for (c, x) in g.chicks.iter_mut().zip([5, 4]) {
        c.pos = TilePos::new(x, 1).center();
        c.prev_pos = c.pos;
        c.join_brood();
    }
    while g.chicks.len() < 2 {
        let mut c = g.chicks[0].clone();
        c.id += 100;
        c.pos = TilePos::new(4, 1).center();
        g.chicks.push(c);
    }
    let t = fence::plan(&g, &[TilePos::new(5, 1)]);
    assert!(t.0.is_empty(), "the tile right behind her is taken");
    g.step(&TickInput {
        action: true,
        ..Default::default()
    });
    let f = g.fences.first().expect("a hurdle went up");
    assert!(f.tiles[0].x < 5, "behind the brood: {:?}", f.tiles);
}

#[test]
fn fences_wear_out() {
    let mut g = playing(Game::new(level(CORRIDORS, "foxes: 0"), 0, Session::default(), 6));
    g.step(&TickInput {
        strokes: vec![FenceStroke {
            id: 3,
            points: vec![[5.5, 4.2], [5.5, 4.8]],
        }],
        ..Default::default()
    });
    assert_eq!(g.fences.len(), 1);
    run(&mut g, (super::rules::FENCE_LIFETIME * 60.0) as u32 + 5, |_| TickInput::default());
    assert!(g.fences.is_empty());
    assert!(g.topo.passable(TilePos::new(5, 4), Mover::Fox));
}

#[test]
fn frightened_chicks_take_cover() {
    let map = "\
############
#H.........#
#.########.#
#..bbb.....#
#D#######F##
##c#########";
    let lvl = Level::parse(&format!("foxes: 1\nmap:\n{}", map.replace("##c#########", "############").replace("#..bbb.....#", "#..bbb..c..#"))).unwrap();
    let mut g = playing(Game::new(lvl, 0, Session::default(), 12));
    let c = &mut g.chicks[0];
    c.pos = TilePos::new(6, 3).center();
    c.prev_pos = c.pos;
    c.home = c.pos;
    c.traits.bravery = 1.0;
    // A fox comes up the corridor from the right.
    let f = &mut g.predators[0];
    f.mover.place(TilePos::new(9, 3));
    f.state = PredState::Prowl {
        goal: TilePos::new(9, 3),
        pause: 100.0,
    };
    let mut scared = false;
    let mut hid = false;
    for _ in 0..240 {
        g.step(&TickInput::default());
        match g.chicks[0].state {
            ChickState::Frightened { mode: FleeMode::Cover, .. } => scared = true,
            ChickState::Hiding { .. } => hid = true,
            _ => {}
        }
        if hid {
            break;
        }
    }
    assert!(scared, "the chick panicked and made for the tall grass");
    assert!(hid, "and hid in it: {:?} at {}", g.chicks[0].state, g.chicks[0].pos);
    assert!(g.topo.grid.sheltered(TilePos::of(g.chicks[0].pos)));
}

#[test]
fn hen_rescues_chick_from_fox_mouth() {
    let map = "\
#############
#H..........#
#.#########.#
#D....c....F#
#############";
    let mut g = playing(Game::new(level(map, "foxes: 1"), 0, Session::default(), 13));
    // The fox has a chick and is heading home along the bottom corridor.
    let cid = g.chicks[0].id;
    let fid = g.predators[0].id;
    g.chicks[0].state = ChickState::Carried { by: fid };
    g.predators[0].carrying = Some(cid);
    g.predators[0].state = PredState::Carry;
    g.predators[0].mover.place(TilePos::new(6, 3));
    g.hen.mover.place(TilePos::new(4, 3));
    let mut rescued = false;
    for _ in 0..120 {
        g.step(&TickInput {
            dir: Some(Dir::Right),
            ..Default::default()
        });
        if g.drain_events().iter().any(|e| matches!(e, SimEvent::ChickRescued { .. })) {
            rescued = true;
            break;
        }
    }
    assert!(rescued);
    assert!(g.hen.alive, "a fox with its mouth full cannot bite");
    assert!(g.chicks[0].in_brood());
    assert!(matches!(g.predators[0].state, PredState::Stunned { .. }));
}

#[test]
fn autopilot_clears_a_fox_free_level() {
    let lvl = Level::parse(&level::BUILTIN[0].1.replace("foxes: 1", "foxes: 0")).unwrap();
    let mut g = Game::new(lvl, 0, Session::default(), 21);
    let mut bot = Autopilot::new();
    for _ in 0..60 * 240 {
        let i = bot.decide(&g);
        g.step(&i);
        g.drain_events();
        if !matches!(g.phase, Phase::Playing | Phase::Ready { .. }) {
            break;
        }
    }
    assert!(matches!(g.phase, Phase::Cleared { .. }), "{:?} after {}s with {} delivered", g.phase, g.time, g.delivered);
    assert_eq!(g.delivered, g.level.chick_count);
    assert!(g.session.score > 0);
}

#[test]
fn bigger_broods_slow_the_hen() {
    let mut g = Game::new(level::builtin(0).unwrap(), 0, Session::default(), 1);
    let base = g.hen_speed();
    for c in g.chicks.iter_mut().take(5) {
        c.join_brood();
    }
    assert!(g.hen_speed() < base);
    assert!(g.hen_speed() >= base * super::rules::HEN_MIN_SPEED_FACTOR);
}
