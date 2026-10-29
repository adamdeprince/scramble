# Dependencies

The project prefers maintained crates to custom infrastructure, and adds
its own code where the crates do not reach: the kitty and Silkie protocol
extensions, and the game itself.

## Direct dependencies

| crate | version | licence | used for |
|---|---|---|---|
| crossterm | 0.29 | MIT | raw mode and window size (termios, ioctl) |
| signal-hook | 0.4 | MIT OR Apache-2.0 | SIGWINCH, SIGTERM, SIGTSTP and friends delivered to the main loop |
| libc | 0.2 | MIT OR Apache-2.0 | `isatty`, `raise`, and the pseudo-terminal in `netsim` |
| tiny-skia | 0.12 | BSD-3-Clause | rasterizing sprites, scenery (for kitty), atlases and the logo |
| image | 0.25 | MIT OR Apache-2.0 | PNG encoding and decoding, sprite resampling |
| ttf-parser | 0.25 | MIT OR Apache-2.0 | glyph outlines of the title typeface |
| flate2 | 1.1 | MIT OR Apache-2.0 | zlib compression of image and vector payloads |
| base64 | 0.23 | MIT OR Apache-2.0 | graphics protocol payloads |
| glam | 0.34 | MIT OR Apache-2.0 | 2D vectors in the simulation |
| pathfinding | 4.16 | Apache-2.0/MIT | A* routes for predators |
| rand, rand_chacha | 0.10 | MIT OR Apache-2.0 | seeded, platform-independent random numbers |
| serde, serde_json | 1 | MIT OR Apache-2.0 | recordings, high scores, the sprite manifest |
| clap | 4.6 | MIT OR Apache-2.0 | command line |
| directories | 6 | MIT OR Apache-2.0 | where to keep high scores |
| tracing, tracing-subscriber | 0.1, 0.3 | MIT | logging to a file |
| anyhow | 1 | MIT OR Apache-2.0 | error reporting |

The 113 crates of the full build graph are all under permissive licences:
mostly MIT or Apache-2.0, with BSD-2/3-Clause (tiny-skia and a few small
crates), Zlib (zlib-rs) and one MPL-2.0 crate (`option-ext`, used by
`directories`). Regenerate the list with
`cargo metadata --format-version 1`.

## Choices and trade-offs

**crossterm for the terminal, but not for input.** crossterm sets up raw
mode and reads the window size well. Its event reader, though, parses
standard keys and mouse reports only: kitty graphics replies (APC) and
Silkie's OSC 777 reports would be dropped or misread as keys, and two
readers on one stream would take each other's bytes. So the game reads stdin
itself and decodes everything in one place (`src/term/input.rs`): kitty
keyboard protocol and legacy keys, SGR mouse in cell or pixel units, focus
changes, graphics replies, OSC 777 frames, mode reports, XTGETTCAP. Every
control-string type is framed correctly, so whatever is interleaved with
what, nothing is lost.

**No ECS.** bevy_ecs was considered. The game has five kinds of entity in
small numbers with fixed relationships (a chick may be carried by a
predator), and the simulation must be deterministic and easy to replay and
test. Plain structs in vectors, updated in a fixed order, do that with less
machinery.

**Steering written for the game.** Reynolds-style seek, arrive, pursuit,
separation, alignment, cohesion and wall avoidance are a few lines each
(`src/sim/steer.rs`); no maintained Rust steering crate fits a tile maze
with this mix of grid flow fields and local forces. Navigation uses the
pathfinding crate's A*, and breadth-first distance fields where whole-yard
answers are needed (every chick's way to the hen, the coop or cover).

**State machines, not behaviour trees.** Predators and chicks have a
handful of readable states each. A behaviour-tree crate (bonsai-bt) would add
indirection without making them easier to reason about or tune.

**rstar not needed.** Neighbour queries among chicks are all-pairs; with
200 chicks a tick of flocking costs about 10 µs (see PERFORMANCE.md), so a
spatial index would not pay for itself.

**tiny-skia for every raster.** One rasterizer draws the sprite art, the
scenery for terminals without vector images, and the gate and fence atlases.
The same vector scene model (`src/gfx/vector.rs`) is serialized to Silkie's
records or rasterized with tiny-skia, so both terminals show the same
garden.

**No game engine.** Engines expect a window and a GPU. The game draws
through the kitty graphics protocol and Silkie's extensions instead; its
"engine" is a fixed-step simulation, a scene builder and a diffing renderer.
