//! The garden, drawn as vector scenes from a level.
//!
//! Two scenes are produced: the ground (lawn, paths, beds, hedges, the pond,
//! the coop) drawn beneath the creatures, and the canopy (tree crowns and the
//! tops of tall grass) drawn above them, so a chick can vanish into the
//! ferns. Gates and fences are separate elements with stable ids so that a
//! gate swinging shut, or a fence going up, is a patch of a few records.
//!
//! Units: one tile is [`TILE`] user units.

use super::vector::{Affine, Color, Element, Fill, PathData, Scene, StrokeStyle};
use crate::sim::fence::Fence;
use crate::sim::gate::{Gate, GateKind};
use crate::world::level::Level;
use crate::world::{Dir, Grid, Mover, Tile, TilePos, Topology};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub const TILE: f32 = 64.0;

// Groups of the ground scene, created in this order (which is drawing order).
pub const G_TERRAIN: u32 = 1;
pub const G_PATHS: u32 = 2;
pub const G_BEDS: u32 = 3;
pub const G_WALLS: u32 = 4;
pub const G_COOP: u32 = 5;
pub const G_GATES: u32 = 6;
pub const G_FENCES: u32 = 7;
pub const G_INK: u32 = 8;
pub const G_MARKS: u32 = 9;

const SYM_TUFT: u32 = 100;
const SYM_FLOWER: u32 = 101;
const SYM_LEAF: u32 = 102;
const SYM_PEBBLE: u32 = 103;
const SYM_LILY: u32 = 104;
const SYM_CABBAGE: u32 = 105;
const SYM_FROND: u32 = 106;
const SYM_CIRCLE: u32 = 107;
const GRAD_LAWN: u32 = 120;
const GRAD_POND: u32 = 121;

const STATIC_BASE: u32 = 1000;
pub const GATE_BASE: u32 = 1_000_000;
pub const FENCE_BASE: u32 = 2_000_000;
pub const MARK_BASE: u32 = 3_000_000;
/// Ids handed to the terminal for its own ink strokes.
pub const INK_BASE: u32 = 10_000_000;
/// Ids for strokes the game draws itself (mouse, or no terminal ink).
pub const STROKE_BASE: u32 = 20_000_000;

pub mod palette {
    use super::Color;
    pub const GRASS: Color = Color::hex(0x8cc169);
    pub const GRASS_LIGHT: Color = Color::hex(0xa2d07b);
    pub const GRASS_DARK: Color = Color::hex(0x6ea852);
    pub const TUFT: Color = Color::hex(0x5f9a45);
    pub const SOIL: Color = Color::hex(0xe4c895);
    pub const SOIL_EDGE: Color = Color::hex(0xc6a06a);
    pub const PEBBLE: Color = Color::hex(0xcfae7c);
    pub const PEBBLE_LIGHT: Color = Color::hex(0xf2e1b8);
    pub const MOWN: Color = Color::hex(0xa8d683);
    pub const HEDGE_INK: Color = Color::hex(0x2b5527);
    pub const HEDGE: Color = Color::hex(0x47863a);
    pub const HEDGE_LIGHT: Color = Color::hex(0x66a74a);
    pub const HEDGE_SHINE: Color = Color::hex(0x8fc760);
    pub const BED: Color = Color::hex(0x95643f);
    pub const BED_EDGE: Color = Color::hex(0x6f4a2f);
    pub const LEAF: Color = Color::hex(0x5a9d45);
    pub const PICKET: Color = Color::hex(0xf7f3ea);
    pub const PICKET_SHADE: Color = Color::hex(0xcfc8b8);
    pub const PICKET_INK: Color = Color::hex(0x857e72);
    pub const WATER: Color = Color::hex(0x67b4de);
    pub const WATER_DEEP: Color = Color::hex(0x4290c2);
    pub const WATER_RIM: Color = Color::hex(0x3a76a0);
    pub const WATER_SHINE: Color = Color::hex(0xc4e9f8);
    pub const LILY: Color = Color::hex(0x6aad4a);
    pub const VEG_SOIL: Color = Color::hex(0x7b5134);
    pub const CABBAGE: Color = Color::hex(0xa9d67c);
    pub const CABBAGE_DARK: Color = Color::hex(0x76ae58);
    pub const CARROT: Color = Color::hex(0xf08a32);
    pub const STONE: Color = Color::hex(0xbdb6aa);
    pub const STONE_DARK: Color = Color::hex(0x9b958a);
    pub const STONE_INK: Color = Color::hex(0x6c665d);
    pub const BARK: Color = Color::hex(0x8a5a3b);
    pub const BARK_DARK: Color = Color::hex(0x5f3d29);
    pub const CANOPY: Color = Color::hex(0x4b8c37);
    pub const CANOPY_DARK: Color = Color::hex(0x356c2a);
    pub const CANOPY_LIGHT: Color = Color::hex(0x6db24a);
    pub const COOP: Color = Color::hex(0xc4513a);
    pub const COOP_DARK: Color = Color::hex(0x983b2a);
    pub const TRIM: Color = Color::hex(0xf6efe2);
    pub const ROOF: Color = Color::hex(0x6e4b3f);
    pub const ROOF_DARK: Color = Color::hex(0x523629);
    pub const DOORWAY: Color = Color::hex(0x3e2b20);
    pub const RAMP: Color = Color::hex(0xb7864f);
    pub const LOG: Color = Color::hex(0x8d6748);
    pub const LOG_DARK: Color = Color::hex(0x5e4330);
    pub const LOG_RING: Color = Color::hex(0xd9b88a);
    pub const PIPE: Color = Color::hex(0xa3acb4);
    pub const PIPE_DARK: Color = Color::hex(0x737c85);
    pub const WOOD: Color = Color::hex(0xc48a51);
    pub const WOOD_DARK: Color = Color::hex(0x8c5d33);
    pub const GATE_PAINT: Color = Color::hex(0x79ac57);
    pub const LATCH_PAINT: Color = Color::hex(0x6f9fc8);
    pub const BRASS: Color = Color::hex(0xe8c45c);
    pub const INK: Color = Color::hex(0x7a5230);
    pub const SHADOW: Color = Color::rgba(30, 50, 20, 70);
    pub const FLOWERS: [Color; 6] = [
        Color::hex(0xf48fb1),
        Color::hex(0xffd54f),
        Color::hex(0xce93d8),
        Color::hex(0xfdfdfd),
        Color::hex(0xef5350),
        Color::hex(0xffa94d),
    ];
}
use palette as pal;

pub struct Scenery {
    pub ground: Scene,
    pub canopy: Scene,
    pub width: f32,
    pub height: f32,
}

struct Builder<'a> {
    grid: &'a Grid,
    rng: ChaCha8Rng,
    ground: Scene,
    canopy: Scene,
    next: u32,
}

fn seed_for(level: &Level) -> u64 {
    level
        .name
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

impl Builder<'_> {
    fn id(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    fn put(&mut self, e: Element) {
        let id = self.id();
        self.ground.set(id, e);
    }

    fn put_canopy(&mut self, e: Element) {
        let id = self.id();
        self.canopy.set(id, e);
    }

    fn jitter(&mut self, a: f32) -> f32 {
        self.rng.random_range(-a..a)
    }

    fn tiles_of(&self, f: impl Fn(Tile) -> bool) -> Vec<TilePos> {
        self.grid.positions().filter(|&p| f(self.grid.get(p))).collect()
    }
}

/// Line segments between the centres of neighbouring tiles. Stroked thick
/// with round caps and joins this is a smooth union of the tiles at a few
/// bytes per tile, which is how corridors, lawns and beds are drawn.
fn tile_network(tiles: &[TilePos]) -> PathData {
    let set: std::collections::BTreeSet<TilePos> = tiles.iter().copied().collect();
    let has = |p: TilePos| set.contains(&p);
    let c = |p: TilePos| (p.x as f32 * TILE + TILE / 2.0, p.y as f32 * TILE + TILE / 2.0);
    let mut d = PathData::new();
    for &p in tiles {
        let (x, y) = c(p);
        let r = TilePos::new(p.x + 1, p.y);
        let dn = TilePos::new(p.x, p.y + 1);
        let mut linked = false;
        if has(r) {
            let (x2, y2) = c(r);
            d.move_to(x, y).line_to(x2, y2);
            linked = true;
        }
        if has(dn) {
            let (x2, y2) = c(dn);
            d.move_to(x, y).line_to(x2, y2);
            linked = true;
        }
        if has(r) && has(dn) && has(TilePos::new(p.x + 1, p.y + 1)) {
            // Diagonals fill the middle of a 2x2 block.
            d.move_to(x, y).line_to(x + TILE, y + TILE);
            d.move_to(x + TILE, y).line_to(x, y + TILE);
        }
        let any = Dir::ALL.iter().any(|&dir| has(p.step(dir)));
        if !linked && !any {
            d.move_to(x, y).line_to(x + 0.1, y);
        }
    }
    d
}

fn network_stroke(group: u32, tiles: &[TilePos], width: f32, color: Color, offset: (f32, f32)) -> Element {
    let mut net = tile_network(tiles);
    if offset != (0.0, 0.0) {
        net = net.transformed(Affine::translate(offset.0, offset.1));
    }
    Element::stroke_path(group, net, StrokeStyle::new(color, width))
}

fn components(tiles: &[TilePos]) -> Vec<Vec<TilePos>> {
    let mut left: Vec<TilePos> = tiles.to_vec();
    let mut out = Vec::new();
    while let Some(start) = left.pop() {
        let mut comp = vec![start];
        let mut i = 0;
        while i < comp.len() {
            let p = comp[i];
            for d in Dir::ALL {
                let q = p.step(d);
                if let Some(k) = left.iter().position(|&t| t == q) {
                    left.swap_remove(k);
                    comp.push(q);
                }
            }
            i += 1;
        }
        comp.sort();
        out.push(comp);
    }
    out.sort();
    out
}

pub fn build(level: &Level) -> Scenery {
    let grid = &level.grid;
    let width = grid.width as f32 * TILE;
    let height = grid.height as f32 * TILE;
    let mut b = Builder {
        grid,
        rng: ChaCha8Rng::seed_from_u64(seed_for(level)),
        ground: Scene::new(width, height),
        canopy: Scene::new(width, height),
        next: STATIC_BASE,
    };
    for g in [G_TERRAIN, G_PATHS, G_BEDS, G_WALLS, G_COOP, G_GATES, G_FENCES, G_INK, G_MARKS] {
        b.ground.set(g, Element::group(0));
    }
    symbols(&mut b.ground);
    symbols(&mut b.canopy);
    b.canopy.set(G_TERRAIN, Element::group(0));
    b.canopy.set(G_WALLS, Element::group(0));

    lawn(&mut b, width, height);
    paths(&mut b);
    cover_ground(&mut b);
    beds(&mut b);
    water(&mut b);
    hedges(&mut b);
    pickets(&mut b);
    stones(&mut b);
    trees(&mut b);
    coop(&mut b);
    dens_and_pipes(&mut b);
    cover_canopy(&mut b);

    Scenery {
        ground: b.ground,
        canopy: b.canopy,
        width,
        height,
    }
}

fn symbols(s: &mut Scene) {
    // A tuft of three blades, about 14 units tall, origin at its base.
    let mut tuft = PathData::new();
    tuft.move_to(-6.0, 0.0).quad_to(-7.0, -8.0, -9.0, -12.0).quad_to(-4.0, -7.0, -2.0, 0.0).close();
    tuft.move_to(-2.0, 0.0).quad_to(-1.0, -10.0, 1.0, -15.0).quad_to(2.5, -8.0, 2.0, 0.0).close();
    tuft.move_to(2.0, 0.0).quad_to(5.0, -7.0, 9.0, -10.0).quad_to(6.0, -5.0, 6.0, 0.0).close();
    s.set(SYM_TUFT, Element::Symbol { d: tuft });
    // Five round petals around the origin.
    let mut flower = PathData::new();
    for k in 0..5 {
        let a = k as f32 * std::f32::consts::TAU / 5.0 - std::f32::consts::FRAC_PI_2;
        flower.extend(&PathData::circle(a.cos() * 4.2, a.sin() * 4.2, 3.6));
    }
    s.set(SYM_FLOWER, Element::Symbol { d: flower });
    let mut leaf = PathData::new();
    leaf.move_to(0.0, 0.0).quad_to(5.0, -5.0, 11.0, 0.0).quad_to(5.0, 5.0, 0.0, 0.0).close();
    s.set(SYM_LEAF, Element::Symbol { d: leaf });
    s.set(SYM_PEBBLE, Element::Symbol { d: PathData::ellipse(0.0, 0.0, 2.6, 1.8) });
    // A lily pad: a disc with a notch.
    let mut lily = PathData::new();
    let n = 14;
    lily.move_to(0.0, 0.0);
    for k in 1..n {
        let a = 0.35 + k as f32 / n as f32 * (std::f32::consts::TAU - 0.7);
        lily.line_to(a.cos() * 7.0, a.sin() * 7.0);
    }
    lily.close();
    s.set(SYM_LILY, Element::Symbol { d: lily });
    let mut cab = PathData::circle(0.0, 0.0, 9.0);
    cab.extend(&PathData::circle(-6.0, 4.0, 5.5));
    cab.extend(&PathData::circle(6.0, 4.0, 5.5));
    s.set(SYM_CABBAGE, Element::Symbol { d: cab });
    // A tall fern frond, origin at its base, about 30 units tall.
    let mut frond = PathData::new();
    frond.move_to(-2.0, 0.0).quad_to(-6.0, -16.0, -3.0, -30.0).quad_to(3.0, -16.0, 2.0, 0.0).close();
    s.set(SYM_FROND, Element::Symbol { d: frond });
    s.set(SYM_CIRCLE, Element::Symbol { d: PathData::circle(0.0, 0.0, 1.0) });
    s.set(
        GRAD_LAWN,
        Element::Linear {
            x1: 0.0,
            y1: 0.0,
            x2: s.width,
            y2: s.height,
            stops: vec![(0.0, pal::GRASS_LIGHT), (0.5, pal::GRASS), (1.0, pal::GRASS_DARK)],
        },
    );
    s.set(
        GRAD_POND,
        Element::Radial {
            cx: 0.0,
            cy: 0.0,
            r: 1.0,
            fx: 0.0,
            fy: 0.0,
            stops: vec![(0.0, pal::WATER), (1.0, pal::WATER_DEEP)],
        },
    );
}

fn lawn(b: &mut Builder, w: f32, h: f32) {
    b.put(Element::fill_path(G_TERRAIN, PathData::rect(0.0, 0.0, w, h), Fill::Gradient(GRAD_LAWN)));
    // Mown stripes.
    let mut stripes = PathData::new();
    let mut x = 0.0;
    while x < w {
        stripes.extend(&PathData::rect(x, 0.0, TILE * 1.5, h));
        x += TILE * 3.0;
    }
    b.put(Element::fill_path(G_TERRAIN, stripes, Color::rgba(255, 255, 255, 14)));
    // Tufts scattered over the grass, avoiding the paths.
    let open: Vec<TilePos> = b.tiles_of(|t| !matches!(t, Tile::Path | Tile::Den | Tile::Door | Tile::Water | Tile::Coop));
    for p in open {
        let n = b.rng.random_range(0..3);
        for _ in 0..n {
            let x = p.x as f32 * TILE + b.rng.random_range(6.0..58.0);
            let y = p.y as f32 * TILE + b.rng.random_range(10.0..60.0);
            let s = b.rng.random_range(0.7..1.15);
            let shade = b.rng.random_range(-0.12..0.1);
            let e = Element::Use {
                group: G_TERRAIN,
                symbol: SYM_TUFT,
                t: Some(Affine([s, 0.0, 0.0, s, x, y])),
                fill: Fill::Solid(pal::TUFT.shade(shade).with_alpha(200)),
            };
            b.put(e);
        }
    }
}

fn paths(b: &mut Builder) {
    let dirt = b.tiles_of(|t| matches!(t, Tile::Path | Tile::Den | Tile::Door | Tile::GateTimed | Tile::GateLatch));
    let lawn = b.tiles_of(|t| matches!(t, Tile::Lawn));
    if !lawn.is_empty() {
        // Walkable lawn: a lighter, mown strip with a soft rim.
        b.put(network_stroke(G_PATHS, &lawn, 58.0, pal::GRASS_DARK.with_alpha(110), (0.0, 0.0)));
        b.put(network_stroke(G_PATHS, &lawn, 54.0, pal::MOWN, (0.0, 0.0)));
    }
    if dirt.is_empty() {
        return;
    }
    b.put(network_stroke(G_PATHS, &dirt, 59.0, pal::SOIL_EDGE, (0.0, 0.0)));
    b.put(network_stroke(G_PATHS, &dirt, 52.0, pal::SOIL, (0.0, 0.0)));
    // Pebbles and a few footprints of wear.
    for p in dirt {
        let n = b.rng.random_range(1..4);
        for _ in 0..n {
            let x = p.x as f32 * TILE + b.rng.random_range(12.0..52.0);
            let y = p.y as f32 * TILE + b.rng.random_range(12.0..52.0);
            let s = b.rng.random_range(0.7..1.5);
            let rot = b.rng.random_range(0.0..3.0f32);
            let light = b.rng.random_bool(0.5);
            let t = Affine::rotate(rot).then(Affine::scale(s, s)).then(Affine::translate(x, y));
            b.put(Element::Use {
                group: G_PATHS,
                symbol: SYM_PEBBLE,
                t: Some(t),
                fill: Fill::Solid(if light { pal::PEBBLE_LIGHT } else { pal::PEBBLE }),
            });
        }
    }
}

fn cover_ground(b: &mut Builder) {
    let cover = b.tiles_of(|t| t == Tile::Cover);
    if cover.is_empty() {
        return;
    }
    b.put(network_stroke(G_PATHS, &cover, 58.0, pal::TUFT.with_alpha(150), (0.0, 0.0)));
}

fn bed_tiles(b: &mut Builder, kind: Tile) -> Vec<Vec<TilePos>> {
    components(&b.tiles_of(|t| t == kind))
}

fn beds(b: &mut Builder) {
    // Flower beds.
    for comp in bed_tiles(b, Tile::Flowers) {
        b.put(network_stroke(G_BEDS, &comp, 62.0, pal::BED_EDGE, (0.0, 0.0)));
        b.put(network_stroke(G_BEDS, &comp, 55.0, pal::BED, (0.0, 0.0)));
        let base = b.rng.random_range(0..pal::FLOWERS.len());
        for &p in &comp {
            for k in 0..4 {
                let x = p.x as f32 * TILE + 14.0 + (k % 2) as f32 * 32.0 + b.jitter(6.0);
                let y = p.y as f32 * TILE + 14.0 + (k / 2) as f32 * 32.0 + b.jitter(6.0);
                let rot = b.rng.random_range(0.0..1.2f32);
                let leaf_t = Affine::rotate(rot + 2.2).then(Affine::translate(x, y));
                b.put(Element::Use {
                    group: G_BEDS,
                    symbol: SYM_LEAF,
                    t: Some(leaf_t),
                    fill: Fill::Solid(pal::LEAF),
                });
                let leaf_t = Affine::rotate(rot - 0.6).then(Affine::translate(x, y));
                b.put(Element::Use {
                    group: G_BEDS,
                    symbol: SYM_LEAF,
                    t: Some(leaf_t),
                    fill: Fill::Solid(pal::LEAF.shade(0.1)),
                });
                let pick = if b.rng.random_bool(0.7) {
                    base
                } else {
                    b.rng.random_range(0..pal::FLOWERS.len())
                };
                let s = b.rng.random_range(0.95..1.3);
                let t = Affine::rotate(rot).then(Affine::scale(s, s)).then(Affine::translate(x, y));
                b.put(Element::Use {
                    group: G_BEDS,
                    symbol: SYM_FLOWER,
                    t: Some(t),
                    fill: Fill::Solid(pal::FLOWERS[pick]),
                });
                b.put(Element::Use {
                    group: G_BEDS,
                    symbol: SYM_CIRCLE,
                    t: Some(Affine::scale(2.6 * s, 2.6 * s).then(Affine::translate(x, y))),
                    fill: Fill::Solid(Color::hex(0xffe066)),
                });
            }
        }
    }
    // Vegetable patches: cabbages and carrots in rows.
    for comp in bed_tiles(b, Tile::Veg) {
        b.put(network_stroke(G_BEDS, &comp, 62.0, pal::BED_EDGE, (0.0, 0.0)));
        b.put(network_stroke(G_BEDS, &comp, 56.0, pal::VEG_SOIL, (0.0, 0.0)));
        for &p in &comp {
            let x0 = p.x as f32 * TILE;
            let y0 = p.y as f32 * TILE;
            // Furrows.
            let mut furrow = PathData::new();
            for k in 0..2 {
                let y = y0 + 20.0 + k as f32 * 26.0;
                furrow.move_to(x0 + 6.0, y).line_to(x0 + TILE - 6.0, y);
            }
            b.put(Element::stroke_path(G_BEDS, furrow, StrokeStyle::new(pal::VEG_SOIL.shade(-0.25), 3.0)));
            let carrots = (p.x + p.y) % 3 == 0;
            for k in 0..2 {
                let x = x0 + 18.0 + k as f32 * 28.0 + b.jitter(3.0);
                for r in 0..2 {
                    let y = y0 + 18.0 + r as f32 * 26.0 + b.jitter(2.0);
                    if carrots {
                        b.put(Element::fill_path(G_BEDS, PathData::ellipse(x, y + 3.0, 4.5, 3.0), pal::CARROT));
                        let mut tops = PathData::new();
                        for s in [-1.0f32, 0.0, 1.0] {
                            tops.move_to(x, y + 1.0).quad_to(x + s * 6.0, y - 6.0, x + s * 7.0, y - 11.0);
                        }
                        b.put(Element::stroke_path(G_BEDS, tops, StrokeStyle::new(pal::LEAF.shade(0.1), 2.4)));
                    } else {
                        let t = Affine([0.8, 0.0, 0.0, 0.8, x, y]);
                        b.put(Element::Use {
                            group: G_BEDS,
                            symbol: SYM_CABBAGE,
                            t: Some(Affine([0.9, 0.0, 0.0, 0.9, x, y + 1.5])),
                            fill: Fill::Solid(pal::CABBAGE_DARK),
                        });
                        b.put(Element::Use {
                            group: G_BEDS,
                            symbol: SYM_CABBAGE,
                            t: Some(t),
                            fill: Fill::Solid(pal::CABBAGE),
                        });
                        let mut vein = PathData::new();
                        vein.move_to(x - 4.0, y - 3.0).quad_to(x, y + 1.0, x + 4.0, y - 3.0);
                        vein.move_to(x, y - 6.0).line_to(x, y + 2.0);
                        b.put(Element::stroke_path(G_BEDS, vein, StrokeStyle::new(pal::CABBAGE_DARK, 1.4)));
                    }
                }
            }
        }
    }
}

fn water(b: &mut Builder) {
    for comp in bed_tiles(b, Tile::Water) {
        b.put(network_stroke(G_BEDS, &comp, 68.0, pal::SOIL_EDGE, (0.0, 0.0)));
        b.put(network_stroke(G_BEDS, &comp, 62.0, pal::WATER_RIM, (0.0, 0.0)));
        b.put(network_stroke(G_BEDS, &comp, 55.0, pal::WATER, (0.0, 0.0)));
        // Deeper water away from the banks.
        let inner: Vec<TilePos> = comp
            .iter()
            .copied()
            .filter(|&p| Dir::ALL.iter().all(|&d| b.grid.get(p.step(d)) == Tile::Water))
            .collect();
        if !inner.is_empty() {
            b.put(network_stroke(G_BEDS, &inner, 40.0, pal::WATER_DEEP.with_alpha(130), (0.0, 0.0)));
        }
        for &p in &comp {
            let x = p.x as f32 * TILE + 32.0 + b.jitter(12.0);
            let y = p.y as f32 * TILE + 32.0 + b.jitter(12.0);
            if b.rng.random_bool(0.45) {
                let rot = b.rng.random_range(0.0..std::f32::consts::TAU);
                let s = b.rng.random_range(0.9..1.5);
                b.put(Element::Use {
                    group: G_BEDS,
                    symbol: SYM_LILY,
                    t: Some(Affine::rotate(rot).then(Affine::scale(s, s)).then(Affine::translate(x, y))),
                    fill: Fill::Solid(pal::LILY),
                });
                if b.rng.random_bool(0.3) {
                    b.put(Element::Use {
                        group: G_BEDS,
                        symbol: SYM_CIRCLE,
                        t: Some(Affine::scale(3.0, 3.0).then(Affine::translate(x + 2.0, y - 2.0))),
                        fill: Fill::Solid(Color::hex(0xf8bbd0)),
                    });
                }
            } else {
                let mut ripple = PathData::new();
                ripple.move_to(x - 9.0, y).quad_to(x, y - 4.0, x + 9.0, y);
                b.put(Element::stroke_path(G_BEDS, ripple, StrokeStyle::new(pal::WATER_SHINE.with_alpha(200), 2.0)));
            }
        }
    }
}

fn bounds(tiles: &[TilePos]) -> (f32, f32, f32, f32) {
    let x0 = tiles.iter().map(|p| p.x).min().unwrap_or(0) as f32 * TILE;
    let y0 = tiles.iter().map(|p| p.y).min().unwrap_or(0) as f32 * TILE;
    let x1 = (tiles.iter().map(|p| p.x).max().unwrap_or(0) + 1) as f32 * TILE;
    let y1 = (tiles.iter().map(|p| p.y).max().unwrap_or(0) + 1) as f32 * TILE;
    (x0, y0, x1, y1)
}

fn hedges(b: &mut Builder) {
    let tiles = b.tiles_of(|t| t == Tile::Hedge);
    if tiles.is_empty() {
        return;
    }
    let mut blobs = Vec::new();
    for &p in &tiles {
        for k in 0..4 {
            let x = p.x as f32 * TILE + 16.0 + (k % 2) as f32 * 32.0 + b.jitter(5.0);
            let y = p.y as f32 * TILE + 16.0 + (k / 2) as f32 * 32.0 + b.jitter(5.0);
            let r = b.rng.random_range(20.0..25.5);
            blobs.push((x, y, r));
        }
    }
    // A soft shadow under the whole hedge, then outline, body and light
    // layers of overlapping round bushes. Each layer is drawn complete
    // before the next, so the bushes merge into one hedge.
    b.put(network_stroke(G_WALLS, &tiles, 70.0, pal::SHADOW, (2.0, 5.0)));
    let layer = |b: &mut Builder, grow: f32, dx: f32, dy: f32, scale: f32, color: Color| {
        for &(x, y, r) in &blobs {
            let rr = r * scale + grow;
            b.put(Element::Use {
                group: G_WALLS,
                symbol: SYM_CIRCLE,
                t: Some(Affine([rr, 0.0, 0.0, rr, x + dx, y + dy])),
                fill: Fill::Solid(color),
            });
        }
    };
    layer(b, 3.5, 0.0, 0.0, 1.0, pal::HEDGE_INK);
    layer(b, 0.0, 0.0, 0.0, 1.0, pal::HEDGE);
    layer(b, 0.0, -3.0, -5.0, 0.72, pal::HEDGE_LIGHT);
    // Leaves catching the light.
    for &(x, y, r) in blobs.iter().step_by(2) {
        let rot = b.rng.random_range(3.6..5.0f32);
        let t = Affine::rotate(rot).then(Affine::scale(0.8, 0.8)).then(Affine::translate(x - r * 0.35, y - r * 0.4));
        b.put(Element::Use {
            group: G_WALLS,
            symbol: SYM_LEAF,
            t: Some(t),
            fill: Fill::Solid(pal::HEDGE_SHINE),
        });
    }
}

fn pickets(b: &mut Builder) {
    let tiles = b.tiles_of(|t| t == Tile::Picket);
    let is = |p: TilePos| b.grid.get(p) == Tile::Picket;
    let mut rails = PathData::new();
    let mut boards = PathData::new();
    let mut posts = PathData::new();
    for &p in &tiles {
        let x0 = p.x as f32 * TILE;
        let y0 = p.y as f32 * TILE;
        let h = is(TilePos::new(p.x - 1, p.y)) || is(TilePos::new(p.x + 1, p.y));
        let v = is(TilePos::new(p.x, p.y - 1)) || is(TilePos::new(p.x, p.y + 1));
        if h {
            // A run seen from the front: rails and pointed boards.
            rails.extend(&PathData::rect(x0, y0 + 26.0, TILE, 6.0));
            rails.extend(&PathData::rect(x0, y0 + 44.0, TILE, 6.0));
            for k in 0..4 {
                let bx = x0 + 3.0 + k as f32 * 16.0;
                boards.extend(&PathData::polygon(&[
                    (bx, y0 + 58.0),
                    (bx, y0 + 16.0),
                    (bx + 5.0, y0 + 10.0),
                    (bx + 10.0, y0 + 16.0),
                    (bx + 10.0, y0 + 58.0),
                ]));
            }
        }
        if v {
            // A run seen from above: a rail and the tops of the boards.
            rails.extend(&PathData::rect(x0 + 27.0, y0, 10.0, TILE));
            for k in 0..4 {
                let by = y0 + 3.0 + k as f32 * 16.0;
                posts.extend(&PathData::round_rect(x0 + 24.0, by, 16.0, 10.0, 3.0));
            }
        }
        if !h && !v {
            posts.extend(&PathData::round_rect(x0 + 20.0, y0 + 20.0, 24.0, 24.0, 5.0));
        }
    }
    let ink = StrokeStyle::new(pal::PICKET_INK, 2.2);
    let shadow = rails.transformed(Affine::translate(2.0, 4.0));
    b.put(Element::fill_path(G_WALLS, shadow, pal::SHADOW));
    b.put(Element::Path {
        group: G_WALLS,
        d: rails,
        fill: Fill::Solid(pal::PICKET_SHADE),
        even_odd: false,
        stroke: Some(ink.clone()),
        t: None,
    });
    b.put(Element::fill_path(G_WALLS, boards.transformed(Affine::translate(2.0, 3.0)), pal::SHADOW));
    b.put(Element::Path {
        group: G_WALLS,
        d: boards,
        fill: Fill::Solid(pal::PICKET),
        even_odd: false,
        stroke: Some(ink.clone()),
        t: None,
    });
    b.put(Element::Path {
        group: G_WALLS,
        d: posts,
        fill: Fill::Solid(pal::PICKET),
        even_odd: false,
        stroke: Some(ink),
        t: None,
    });
}

fn stones(b: &mut Builder) {
    let tiles = b.tiles_of(|t| t == Tile::Stone);
    if tiles.is_empty() {
        return;
    }
    let mut dark = PathData::new();
    let mut light = PathData::new();
    for &p in &tiles {
        let x0 = p.x as f32 * TILE;
        let y0 = p.y as f32 * TILE;
        for row in 0..3 {
            let off = if row % 2 == 0 { 0.0 } else { 11.0 };
            let mut x = x0 - off + 2.0;
            while x < x0 + TILE - 4.0 {
                let w = b.rng.random_range(16.0..24.0f32).min(x0 + TILE - 2.0 - x.max(x0 + 2.0));
                let sx = x.max(x0 + 2.0);
                let y = y0 + 3.0 + row as f32 * 20.0 + b.jitter(1.5);
                if w > 6.0 {
                    let stone = PathData::round_rect(sx, y, w - 2.0, 17.0, 6.0);
                    if b.rng.random_bool(0.35) {
                        dark.extend(&stone);
                    } else {
                        light.extend(&stone);
                    }
                }
                x += w;
            }
        }
    }
    let ink = StrokeStyle::new(pal::STONE_INK, 2.0);
    b.put(network_stroke(G_WALLS, &tiles, 66.0, pal::SHADOW, (2.0, 4.0)));
    for (d, c) in [(light, pal::STONE), (dark, pal::STONE_DARK)] {
        b.put(Element::Path {
            group: G_WALLS,
            d,
            fill: Fill::Solid(c),
            even_odd: false,
            stroke: Some(ink.clone()),
            t: None,
        });
    }
}

fn trees(b: &mut Builder) {
    for comp in bed_tiles(b, Tile::Tree) {
        let (x0, y0, x1, y1) = bounds(&comp);
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        let rx = (x1 - x0) / 2.0;
        let ry = (y1 - y0) / 2.0;
        // Shade on the ground around the trunk: hawks cannot see into it.
        b.put(Element::fill_path(
            G_TERRAIN,
            PathData::ellipse(cx + 8.0, cy + 12.0, rx + 40.0, ry + 36.0),
            Color::rgba(25, 60, 20, 55),
        ));
        // Roots and trunk.
        let mut roots = PathData::new();
        for k in 0..5 {
            let a = k as f32 * 1.26 + 0.3;
            roots.move_to(cx, cy).quad_to(cx + a.cos() * 18.0, cy + a.sin() * 10.0, cx + a.cos() * 30.0, cy + a.sin() * 22.0);
        }
        b.put(Element::stroke_path(G_WALLS, roots, StrokeStyle::new(pal::BARK_DARK, 7.0)));
        b.put(Element::fill_path(G_WALLS, PathData::circle(cx, cy, 17.0), pal::BARK_DARK));
        b.put(Element::fill_path(G_WALLS, PathData::circle(cx - 1.0, cy - 1.0, 13.0), pal::BARK));
        // The crown, above everything.
        let n = 9 + comp.len() * 2;
        let mut puffs = Vec::new();
        for k in 0..n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU + b.jitter(0.2);
            let rr = b.rng.random_range(0.55..0.85);
            let x = cx + a.cos() * rx * rr;
            let y = cy + a.sin() * ry * rr - 6.0;
            puffs.push((x, y, b.rng.random_range(22.0..30.0) + rx.min(ry) * 0.18));
        }
        puffs.push((cx, cy - 6.0, rx.min(ry) * 0.8));
        let layer = |dx: f32, dy: f32, s: f32, grow: f32| {
            let mut d = PathData::new();
            for &(x, y, r) in &puffs {
                d.extend(&PathData::circle(x + dx, y + dy, r * s + grow));
            }
            d
        };
        b.put_canopy(Element::fill_path(G_WALLS, layer(6.0, 10.0, 1.0, 2.0), Color::rgba(20, 45, 15, 60)));
        b.put_canopy(Element::fill_path(G_WALLS, layer(0.0, 0.0, 1.0, 3.5), pal::HEDGE_INK.with_alpha(235)));
        b.put_canopy(Element::fill_path(G_WALLS, layer(0.0, 0.0, 1.0, 0.0), pal::CANOPY.with_alpha(240)));
        b.put_canopy(Element::fill_path(G_WALLS, layer(-5.0, -7.0, 0.7, 0.0), pal::CANOPY_LIGHT.with_alpha(240)));
        for &(x, y, r) in puffs.iter().step_by(2) {
            let mut glint = PathData::new();
            glint.move_to(x - r * 0.5, y - r * 0.15).quad_to(x - r * 0.3, y - r * 0.55, x + r * 0.1, y - r * 0.6);
            b.put_canopy(Element::stroke_path(G_WALLS, glint, StrokeStyle::new(pal::HEDGE_SHINE.with_alpha(220), 3.0)));
        }
        // A few apples on orchard trees.
        if comp.len() <= 2 || b.rng.random_bool(0.6) {
            for _ in 0..3 {
                let x = cx + b.jitter(rx * 0.7);
                let y = cy + b.jitter(ry * 0.6);
                b.put_canopy(Element::fill_path(G_WALLS, PathData::circle(x, y, 4.5), Color::hex(0xe5483b)));
                b.put_canopy(Element::fill_path(G_WALLS, PathData::circle(x - 1.5, y - 1.5, 1.4), Color::hex(0xffb3a8)));
            }
        }
    }
}

fn coop(b: &mut Builder) {
    for comp in bed_tiles(b, Tile::Coop) {
        let (x0, y0, x1, y1) = bounds(&comp);
        let w = x1 - x0;
        let h = y1 - y0;
        let wall_top = y0 + h * 0.42;
        b.put(Element::fill_path(G_COOP, PathData::rect(x0 + 6.0, y0 + 10.0, w, h), pal::SHADOW));
        // Front wall with planks.
        b.put(Element::Path {
            group: G_COOP,
            d: PathData::rect(x0 + 4.0, wall_top, w - 8.0, y1 - wall_top - 2.0),
            fill: Fill::Solid(pal::COOP),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::COOP_DARK.shade(-0.3), 3.0)),
            t: None,
        });
        let mut planks = PathData::new();
        let mut x = x0 + 16.0;
        while x < x1 - 8.0 {
            planks.move_to(x, wall_top + 4.0).line_to(x, y1 - 6.0);
            x += 14.0;
        }
        b.put(Element::stroke_path(G_COOP, planks, StrokeStyle::new(pal::COOP_DARK, 2.0)));
        // White corner trim.
        for tx in [x0 + 4.0, x1 - 12.0] {
            b.put(Element::fill_path(G_COOP, PathData::rect(tx, wall_top, 8.0, y1 - wall_top - 2.0), pal::TRIM));
        }
        // Round window.
        let wx = x0 + w * 0.3;
        let wy = wall_top + (y1 - wall_top) * 0.42;
        b.put(Element::Path {
            group: G_COOP,
            d: PathData::circle(wx, wy, 11.0),
            fill: Fill::Solid(Color::hex(0xfff3c4)),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::TRIM, 4.0)),
            t: None,
        });
        let mut cross = PathData::new();
        cross.move_to(wx - 11.0, wy).line_to(wx + 11.0, wy).move_to(wx, wy - 11.0).line_to(wx, wy + 11.0);
        b.put(Element::stroke_path(G_COOP, cross, StrokeStyle::new(pal::TRIM, 2.5)));
        // Roof, with rows of shingles; drawn above passing creatures.
        let roof = PathData::polygon(&[
            (x0 - 6.0, wall_top + 6.0),
            (x0 + 10.0, y0 + 4.0),
            (x1 - 10.0, y0 + 4.0),
            (x1 + 6.0, wall_top + 6.0),
        ]);
        b.put_canopy(Element::Path {
            group: G_WALLS,
            d: roof,
            fill: Fill::Solid(pal::ROOF),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::ROOF_DARK.shade(-0.3), 3.0)),
            t: None,
        });
        let mut shingles = PathData::new();
        let rows = 3;
        for r in 1..=rows {
            let y = y0 + 4.0 + (wall_top + 6.0 - y0 - 4.0) * r as f32 / (rows as f32 + 0.6);
            let inset = 10.0 * (1.0 - r as f32 / (rows as f32 + 1.0));
            let mut x = x0 + inset;
            shingles.move_to(x, y);
            while x < x1 - inset - 1.0 {
                let nx = (x + 12.0).min(x1 - inset);
                shingles.quad_to((x + nx) / 2.0, y + 6.0, nx, y);
                x = nx;
            }
        }
        b.put_canopy(Element::stroke_path(G_WALLS, shingles, StrokeStyle::new(pal::ROOF_DARK, 2.2)));
        let mut ridge = PathData::new();
        ridge.move_to(x0 + 10.0, y0 + 5.0).line_to(x1 - 10.0, y0 + 5.0);
        b.put_canopy(Element::stroke_path(G_WALLS, ridge, StrokeStyle::new(pal::TRIM, 4.0)));
        // A heart on the gable.
        let hx = x0 + w * 0.5;
        let hy = y0 + (wall_top - y0) * 0.45;
        let mut heart = PathData::new();
        heart
            .move_to(hx, hy + 6.0)
            .cubic_to(hx - 10.0, hy - 1.0, hx - 6.0, hy - 9.0, hx, hy - 4.0)
            .cubic_to(hx + 6.0, hy - 9.0, hx + 10.0, hy - 1.0, hx, hy + 6.0)
            .close();
        b.put_canopy(Element::fill_path(G_WALLS, heart, Color::hex(0xff8a80)));
    }
    for &d in &b.tiles_of(|t| t == Tile::Door) {
        let x = d.x as f32 * TILE;
        let y = d.y as f32 * TILE;
        // Which way the door opens: towards walkable ground.
        let out = Dir::ALL
            .into_iter()
            .find(|&dir| b.grid.get(d.step(dir)).is_ground() || b.grid.get(d.step(dir)).is_gate())
            .unwrap_or(Dir::Down);
        let (ramp, arch) = match out {
            Dir::Down => (PathData::rect(x + 16.0, y + 34.0, 32.0, 32.0), PathData::round_rect(x + 12.0, y + 4.0, 40.0, 38.0, 16.0)),
            Dir::Up => (PathData::rect(x + 16.0, y - 2.0, 32.0, 30.0), PathData::round_rect(x + 12.0, y + 22.0, 40.0, 38.0, 16.0)),
            Dir::Left => (PathData::rect(x - 2.0, y + 18.0, 30.0, 28.0), PathData::round_rect(x + 22.0, y + 10.0, 38.0, 44.0, 16.0)),
            Dir::Right => (PathData::rect(x + 36.0, y + 18.0, 30.0, 28.0), PathData::round_rect(x + 4.0, y + 10.0, 38.0, 44.0, 16.0)),
        };
        b.put(Element::Path {
            group: G_COOP,
            d: arch,
            fill: Fill::Solid(pal::DOORWAY),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::TRIM, 4.0)),
            t: None,
        });
        b.put(Element::Path {
            group: G_COOP,
            d: ramp.clone(),
            fill: Fill::Solid(pal::RAMP),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::WOOD_DARK, 2.0)),
            t: None,
        });
        if let Some((rx0, ry0, rx1, ry1)) = ramp.bounds() {
            let mut slats = PathData::new();
            if out.is_horizontal() {
                let mut sx = rx0 + 6.0;
                while sx < rx1 - 2.0 {
                    slats.move_to(sx, ry0 + 2.0).line_to(sx, ry1 - 2.0);
                    sx += 8.0;
                }
            } else {
                let mut sy = ry0 + 6.0;
                while sy < ry1 - 2.0 {
                    slats.move_to(rx0 + 2.0, sy).line_to(rx1 - 2.0, sy);
                    sy += 8.0;
                }
            }
            b.put(Element::stroke_path(G_COOP, slats, StrokeStyle::new(pal::WOOD_DARK, 1.6)));
        }
    }
}

fn dens_and_pipes(b: &mut Builder) {
    for &d in &b.tiles_of(|t| t == Tile::Den) {
        let x = d.x as f32 * TILE;
        let y = d.y as f32 * TILE;
        // A hollow log lying across the tile, open towards the yard.
        b.put(Element::fill_path(G_COOP, PathData::ellipse(x + 34.0, y + 46.0, 30.0, 10.0), pal::SHADOW));
        b.put(Element::Path {
            group: G_COOP,
            d: PathData::round_rect(x + 2.0, y + 14.0, 60.0, 34.0, 16.0),
            fill: Fill::Solid(pal::LOG),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::LOG_DARK, 3.0)),
            t: None,
        });
        let mut bark = PathData::new();
        for k in 0..3 {
            let yy = y + 22.0 + k as f32 * 9.0;
            bark.move_to(x + 14.0, yy).quad_to(x + 30.0, yy + 3.0, x + 46.0, yy);
        }
        b.put(Element::stroke_path(G_COOP, bark, StrokeStyle::new(pal::LOG_DARK, 2.0)));
        b.put(Element::fill_path(G_COOP, PathData::ellipse(x + 50.0, y + 31.0, 11.0, 16.0), pal::LOG_RING));
        b.put(Element::fill_path(G_COOP, PathData::ellipse(x + 51.0, y + 31.0, 7.0, 11.0), pal::DOORWAY));
    }
    let pipes = b.tiles_of(|t| t == Tile::Pipe);
    let is = |p: TilePos| b.grid.get(p) == Tile::Pipe;
    for &p in &pipes {
        let x = p.x as f32 * TILE;
        let y = p.y as f32 * TILE;
        let horiz = is(TilePos::new(p.x - 1, p.y))
            || is(TilePos::new(p.x + 1, p.y))
            || (!is(TilePos::new(p.x, p.y - 1)) && !is(TilePos::new(p.x, p.y + 1)) && b.grid.get(TilePos::new(p.x - 1, p.y)).is_ground());
        let (body, ribs) = if horiz {
            let mut r = PathData::new();
            for k in 0..4 {
                let rx = x + 8.0 + k as f32 * 16.0;
                r.move_to(rx, y + 18.0).line_to(rx, y + 46.0);
            }
            (PathData::round_rect(x - 1.0, y + 16.0, TILE + 2.0, 32.0, 6.0), r)
        } else {
            let mut r = PathData::new();
            for k in 0..4 {
                let ry = y + 8.0 + k as f32 * 16.0;
                r.move_to(x + 18.0, ry).line_to(x + 46.0, ry);
            }
            (PathData::round_rect(x + 16.0, y - 1.0, 32.0, TILE + 2.0, 6.0), r)
        };
        b.put(Element::fill_path(G_BEDS, body.transformed(Affine::translate(2.0, 4.0)), pal::SHADOW));
        b.put(Element::Path {
            group: G_BEDS,
            d: body,
            fill: Fill::Solid(pal::PIPE),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::PIPE_DARK, 2.5)),
            t: None,
        });
        b.put(Element::stroke_path(G_BEDS, ribs, StrokeStyle::new(pal::PIPE_DARK.with_alpha(160), 2.0)));
        // Open ends where the pipe meets the yard.
        for d in Dir::ALL {
            let q = p.step(d);
            if b.grid.get(q).is_ground() {
                let (ex, ey) = match d {
                    Dir::Left => (x + 4.0, y + 32.0),
                    Dir::Right => (x + 60.0, y + 32.0),
                    Dir::Up => (x + 32.0, y + 4.0),
                    Dir::Down => (x + 32.0, y + 60.0),
                };
                let (rx, ry) = if d.is_horizontal() { (7.0, 14.0) } else { (14.0, 7.0) };
                b.put(Element::fill_path(G_BEDS, PathData::ellipse(ex, ey, rx, ry), pal::DOORWAY));
            }
        }
    }
}

fn cover_canopy(b: &mut Builder) {
    let cover = b.tiles_of(|t| t == Tile::Cover);
    for &p in &cover {
        for k in 0..7 {
            let x = p.x as f32 * TILE + 6.0 + (k as f32 * 9.0) + b.jitter(3.0);
            let y = p.y as f32 * TILE + 26.0 + (k % 3) as f32 * 15.0 + b.jitter(4.0);
            let lean = b.jitter(0.35);
            let s = b.rng.random_range(0.8..1.15);
            let shade = b.rng.random_range(-0.15..0.15);
            let t = Affine::rotate(lean).then(Affine::scale(s, s)).then(Affine::translate(x, y));
            b.put_canopy(Element::Use {
                group: G_TERRAIN,
                symbol: SYM_FROND,
                t: Some(t),
                fill: Fill::Solid(pal::CANOPY_LIGHT.shade(shade).with_alpha(215)),
            });
        }
    }
}

/// Which way a corridor runs through a tile: true when it runs left-right.
pub fn corridor_horizontal(topo: &Topology, p: TilePos) -> bool {
    let open = |d: Dir| {
        let q = p.step(d);
        topo.grid.get(q).is_ground() || topo.grid.get(q).is_gate() || topo.grid.get(q) == Tile::Door
    };
    let h = open(Dir::Left) || open(Dir::Right);
    let v = open(Dir::Up) || open(Dir::Down);
    h || !v
}

/// Records for one gate at a given swing (0 open, 1 shut).
pub fn gate_elements(gate: &Gate, index: usize, topo: &Topology, swing: f32) -> Vec<(u32, Element)> {
    let p = gate.tile;
    let x = p.x as f32 * TILE;
    let y = p.y as f32 * TILE;
    let base = GATE_BASE + index as u32 * 16;
    let paint = if gate.kind == GateKind::Latch { pal::LATCH_PAINT } else { pal::GATE_PAINT };
    let horiz = corridor_horizontal(topo, p);
    // Posts on the two sides of the passage; the leaf hinges on the first.
    let (hinge, other) = if horiz {
        ((x + 32.0, y + 4.0), (x + 32.0, y + 60.0))
    } else {
        ((x + 4.0, y + 32.0), (x + 60.0, y + 32.0))
    };
    let mut out = Vec::new();
    let mut posts = PathData::circle(hinge.0, hinge.1, 7.0);
    posts.extend(&PathData::circle(other.0, other.1, 7.0));
    // The leaf: 52 units long from the hinge, rotated open by up to 90°.
    let len = 52.0;
    let closed_angle = if horiz { std::f32::consts::FRAC_PI_2 } else { 0.0 };
    let angle = closed_angle - (1.0 - swing.clamp(0.0, 1.0)) * std::f32::consts::FRAC_PI_2 * 0.95;
    let t = Affine::rotate(angle).then(Affine::translate(hinge.0, hinge.1));
    let mut leaf = PathData::round_rect(4.0, -7.0, len, 14.0, 4.0);
    let mut slats = PathData::new();
    for k in 1..4 {
        let sx = 4.0 + k as f32 * len / 4.0;
        slats.move_to(sx, -6.0).line_to(sx, 6.0);
    }
    slats.move_to(6.0, 5.0).line_to(len, -5.0);
    leaf = leaf.transformed(t);
    slats = slats.transformed(t);
    out.push((base, Element::fill_path(G_GATES, leaf.transformed(Affine::translate(2.0, 4.0)), pal::SHADOW)));
    out.push((
        base + 1,
        Element::Path {
            group: G_GATES,
            d: leaf,
            fill: Fill::Solid(paint),
            even_odd: false,
            stroke: Some(StrokeStyle::new(paint.shade(-0.45), 2.5)),
            t: None,
        },
    ));
    out.push((base + 2, Element::stroke_path(G_GATES, slats, StrokeStyle::new(paint.shade(-0.3), 2.0))));
    out.push((
        base + 3,
        Element::Path {
            group: G_GATES,
            d: posts,
            fill: Fill::Solid(pal::WOOD),
            even_odd: false,
            stroke: Some(StrokeStyle::new(pal::WOOD_DARK, 2.5)),
            t: None,
        },
    ));
    if gate.kind == GateKind::Latch {
        let (lx, ly) = t.apply(len - 2.0, 0.0);
        out.push((base + 4, Element::fill_path(G_GATES, PathData::circle(lx, ly, 4.5), pal::BRASS)));
    } else if gate.warning && !gate.closed {
        // A little red flag before a timed gate swings shut.
        let mut flag = PathData::polygon(&[(0.0, 0.0), (14.0, 5.0), (0.0, 10.0)]);
        flag = flag.transformed(Affine::translate(other.0 + 4.0, other.1 - 20.0));
        out.push((base + 4, Element::fill_path(G_GATES, flag, Color::hex(0xe53935))));
    } else {
        out.push((base + 4, Element::fill_path(G_GATES, PathData::new(), Fill::None)));
    }
    out
}

/// Number of element ids one fence tile uses.
pub const FENCE_IDS_PER_TILE: u32 = 4;

pub fn fence_tile_id(topo: &Topology, p: TilePos) -> u32 {
    FENCE_BASE + (p.y as u32 * topo.width() as u32 + p.x as u32) * FENCE_IDS_PER_TILE
}

/// Records for one fenced tile: a rustic hurdle across the path, greying
/// and sagging as it wears.
pub fn fence_tile_elements(topo: &Topology, p: TilePos, wear: f32) -> Vec<(u32, Element)> {
    fence_elements_at(p, corridor_horizontal(topo, p), wear, fence_tile_id(topo, p))
}

/// A hurdle on tile `p`; across a left-right corridor (`horiz_corridor`)
/// it stands up and down.
pub fn fence_elements_at(p: TilePos, horiz_corridor: bool, wear: f32, base: u32) -> Vec<(u32, Element)> {
    let x = p.x as f32 * TILE;
    let y = p.y as f32 * TILE;
    let wood = pal::WOOD.mix(Color::hex(0xb7ab98), wear * 0.8);
    let dark = pal::WOOD_DARK.mix(Color::hex(0x7d7468), wear * 0.8);
    let sag = wear * 5.0;
    let (a, bpt) = if horiz_corridor { ((x + 32.0, y + 2.0), (x + 32.0, y + 62.0)) } else { ((x + 2.0, y + 32.0), (x + 62.0, y + 32.0)) };
    // A woven hurdle: three rails between two stout posts, with stakes.
    let mut rails = PathData::new();
    for k in 0..3 {
        let o = (k as f32 - 1.0) * 8.0;
        if horiz_corridor {
            rails.move_to(a.0 + o, a.1).quad_to(a.0 + o + sag, (a.1 + bpt.1) / 2.0, bpt.0 + o, bpt.1);
        } else {
            rails.move_to(a.0, a.1 + o).quad_to((a.0 + bpt.0) / 2.0, a.1 + o + sag, bpt.0, bpt.1 + o);
        }
    }
    for k in 1..4 {
        let t = k as f32 / 4.0;
        let (sx, sy) = (a.0 + (bpt.0 - a.0) * t, a.1 + (bpt.1 - a.1) * t);
        if horiz_corridor {
            rails.move_to(sx - 11.0, sy).line_to(sx + 11.0, sy);
        } else {
            rails.move_to(sx, sy - 11.0).line_to(sx, sy + 11.0);
        }
    }
    let mut posts = PathData::round_rect(a.0 - 8.0, a.1 - 8.0, 16.0, 16.0, 4.0);
    posts.extend(&PathData::round_rect(bpt.0 - 8.0, bpt.1 - 8.0, 16.0, 16.0, 4.0));
    let mut rail_style = StrokeStyle::new(wood, 6.0);
    if wear > 0.66 {
        rail_style.dash = vec![14.0, 6.0];
    }
    vec![
        (base, Element::stroke_path(G_FENCES, rails.transformed(Affine::translate(2.0, 4.0)), StrokeStyle::new(pal::SHADOW, 8.0))),
        (base + 1, Element::stroke_path(G_FENCES, rails.clone(), StrokeStyle::new(dark, 9.5))),
        (base + 2, Element::stroke_path(G_FENCES, rails, rail_style)),
        (
            base + 3,
            Element::Path {
                group: G_FENCES,
                d: posts,
                fill: Fill::Solid(wood),
                even_odd: false,
                stroke: Some(StrokeStyle::new(dark, 2.5)),
                t: None,
            },
        ),
    ]
}

/// The wear bucket a fence is drawn at (so redraws happen in a few steps,
/// not every tick).
pub fn wear_stage(f: &Fence) -> u8 {
    let w = f.wear();
    if w < 0.5 {
        0
    } else if w < 0.8 {
        1
    } else {
        2
    }
}

pub fn wear_of_stage(stage: u8) -> f32 {
    match stage {
        0 => 0.0,
        1 => 0.6,
        _ => 0.9,
    }
}

/// Whether the hen could stand here (used to decide gate orientation in
/// tests and art).
pub fn walkable(topo: &Topology, p: TilePos) -> bool {
    topo.passable(p, Mover::Hen)
}
