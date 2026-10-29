# Performance

What the game costs to run and to send, how it was measured, and what the
numbers do and do not mean.

All figures here were taken on an Apple M4 Max with a release build, kitty
0.48.2 and Silkie 0.49.2 on macOS. Each table says how to reproduce it.

## Clocks

The game measures with two clocks and never mixes them (see
[TECHNICAL.md](TECHNICAL.md#measurements-and-their-clocks)):

* **our clock**: simulation and rendering costs, when `write` returns (the
  bytes have left the game, nothing more), and round trips we time
  ourselves;
* **the terminal's clock**: Silkie's presentation reports say when a frame
  was handed to the display system. That is not when it was lit on screen
  (the next refresh, or later), and terminal timestamps are only compared
  with each other.

## Simulation

`silkie-scramble headless --seed 3 --seconds 300`: the autopilot plays every
level without a terminal.

| level | outcome | home | lost | time (s) | score | mean tick (µs) | max tick (µs) |
|---|---|---|---|---|---|---|---|
| 1 Morning Meadow | cleared | 6/7 | 1 | 41 | 3245 | 1.8 | 57.4 |
| 2 The Orchard | cleared | 6/8 | 2 | 47 | 3175 | 1.9 | 88.1 |
| 3 Kitchen Garden | cleared | 7/8 | 1 | 33 | 3755 | 2.4 | 14.9 |
| 4 Lily Pond | cleared | 7/9 | 2 | 34 | 3510 | 1.8 | 23.8 |
| 5 The Hedge Maze | cleared | 7/9 | 2 | 65 | 3285 | 2.4 | 108.0 |
| 6 Dusk | too few home | 3/10 | 7 | 53 | 710 | 3.3 | 135.3 |

A whole tick (flock, predators, navigation, fences, scoring) costs a few
microseconds against a budget of 16.7 ms. The maximum includes the first
ticks of a level, when distance fields are built. (The table also shows the
difficulty climbing: the simple autopilot, which never uses cover and only
drops fences in a pinch, usually fails Dusk.)

`silkie-scramble bench`, flocking alone, with the whole brood following the
hen:

| chicks | mean flock update (µs) | worst (µs) |
|---|---|---|
| 8 | 0.3 | 26.7 |
| 32 | 1.4 | 64.1 |
| 64 | 2.9 | 94.2 |
| 200 | 12.2 | 435.6 |

Neighbour queries are all-pairs; the cost grows with the square of the
brood but stays tiny at any size a level uses (ten chicks at most), and still
well within budget at two hundred. A spatial index would not pay for itself.

## What is sent to the terminal

`silkie-scramble bench` renders one autopilot game on level 1 (1800 frames)
into memory with each renderer configuration, for a 200x56 window of 18x36
pixel cells, then resizes it by two columns, and then to 240x64.

| configuration | first frame | mean steady frame | p95 steady frame | fence placed | small resize | big resize | big resize work | build per frame |
|---|---|---|---|---|---|---|---|---|
| kitty, inline PNG (remote) | 2305.5 KB | 674 B | 1252 B | 2132 B | 0.7 KB | 2693.5 KB | 515 ms | 0.26 ms |
| kitty, temp files (local) | 2.3 KB | 632 B | 1248 B | 2132 B | 0.7 KB | 2.2 KB | 318 ms | 0.16 ms |
| Silkie, vector scenery (remote) | 1442.7 KB | 778 B | 1775 B | 3024 B | 0.7 KB | 1558.1 KB | 286 ms | 0.15 ms |

Reading the table:

* **Steady play costs about 650 bytes a frame** on kitty and about 780 on
  Silkie: a dozen placement updates (one per moving creature, about 70
  bytes each, no image data), the occasional status line. Silkie's extra
  is the HDR glints: each sparkle, star and heart has a second placement
  that moves with it, and the glint overlay itself (14 KB inline, once per
  sprite size). At 60 frames a second that is 40 to 47 KB/s.
* **The first frame of a level** is dominated by images. Locally they go
  through temporary files (the 2.3 KB is just the commands). Over a link,
  kitty needs the sprite sheets plus the garden as bitmaps (2.3 MB, sent once
  per level); Silkie needs the sprite sheets plus the garden as vector
  records, about 90 KB compressed where kitty needs close to a megabyte of
  PNG (1.4 MB in all).
* **A small resize costs 0.7 KB** everywhere: sprites are drawn at steps
  about 11% apart, so a window resized a little re-sends nothing; images are
  only placed anew. Silkie re-rasterizes its vector garden itself; kitty
  scales the bitmap.
* **A big resize** (to a new sprite step) re-sends the sprite sheets; on
  kitty the garden bitmap too. The "work" column is the time to paint and
  encode them.
* **A fence** going up is a vector patch of four path records per tile on
  Silkie (redrawn by Silkie only within the rectangle they cover), and a
  placement of a pre-rendered panel on kitty; the frame's other content
  (dust puffs, creatures) makes up the rest of the 2 KB.

How the numbers came down, for the record: the first remote frame was 6.4
MB (kitty) and 4.2 MB (Silkie) with sprite sheets resampled from the PNG
masters and compressed for speed. Painting the sheets at the exact size
(resampling makes them compress twice as badly), default PNG compression,
sending sheets only when first needed, and a reduced-resolution garden for
remote kitty brought them to the figures above.

## Live, in the terminals

The diagnostics panel (F2) shows these while playing. Typical values on
level 1, in a 1500x900 point window:

| | kitty, local | Silkie, local | Silkie, 80 ms round trip, 2 Mbit/s |
|---|---|---|---|
| scenery | raster | vector `f=200` | vector `f=200` |
| image uploads | temp file | temp file | inline (1.7 MB at level start) |
| pacing | timer, 16.67 ms | strict, by reports | pipelined, by reports |
| frames presented (terminal clock) | (not reported) | every 15.5 ms | every 16.7 ms |
| link round trip (our clock) | | 0.1 ms | 82 ms |
| write to report (our clock) | | 15.1 ms | (not attributable, see below) |
| frames on the way | | 1 | 10 of at most 12 |
| report timeouts | | 6 in 700 frames | 0 in 1000 frames |
| bytes per frame (with the panel open) | 972 B | 967 B | 812 B |
| frame build / write (our clock) | 0.11 / 0.04 ms | 0.08 / 0.02 ms | 0.11 / 0.02 ms |

* Silkie advertised a 120 Hz display (period 8333 µs) and presented the
  game's frames at about 60 Hz; the game paces to what is presented, not to
  the nominal rate.
* Locally, "write to report" (about one refresh) is a real per-frame
  measurement: pacing is strict, one frame in flight. Over the link, reports
  come back while newer frames are on their way and do not say which write
  they cover, so per-frame latency cannot be attributed; the link round trip
  is measured separately with an OSC 777 query instead.
* The panel itself is rewritten four times a second, which accounts for part
  of the difference from the bench's steady-frame figures (other parts: a
  different window size, and what happens to be moving).

### Slow links

`netsim` (`src/bin/netsim.rs`) runs the game on a pseudo-terminal behind a
simulated link: a one-way delay in each direction, a bandwidth limit on the
game's output, and a 256 KB buffer so the game feels backpressure as it
would from a TCP socket.

```sh
cargo run --release --bin netsim -- --delay-ms 40 --kbps 2000 -- \
    target/release/silkie-scramble --no-tempfile --diagnostics
```

At 80 ms round trip and 2 Mbit/s:

* The status lines and a "planting the garden" banner appear at once. The
  first level's images (1.7 MB on Silkie, 2.2 MB on kitty, as counted by the
  diagnostics panel) need about 7 s and 9 s at 2 Mbit/s; on kitty the title
  was complete when looked at after 17 s. They then stay for the whole
  level.
* Once loaded, play is smooth in both: about 40 KB/s of placement updates.
  On Silkie, frames are pipelined against the measured round trip and
  presented every 16.7 ms with no timeouts; without presentation reports
  (kitty) frames go out on a timer and the link absorbs them.
* Every key press takes the round trip to take effect, as with any remote
  program. Pen strokes on Silkie are the exception: with terminal-side ink
  the line appears under the pen at once, wherever the game runs.

## Limitations

* No figure here is the time until light leaves the screen. Presentation
  reports are when a frame was handed to the display system.
* Keys carry no timestamp, so key latency is measured only on our side
  (arrival to write).
* Pen-to-presentation latency (terminal clock) is measured when a pen is in
  use without terminal ink; no pen was available for these measurements.
* Haptics were not observed: how a tap feels is not something a screenshot
  shows. (HDR was: see the end of [TECHNICAL.md](TECHNICAL.md#testing).)
* `netsim` models delay, bandwidth and buffering, not loss, jitter or
  reordering.
