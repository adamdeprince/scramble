# Silkie Scramble: technical notes

How the game is put together: the simulation and its creatures, the
renderer, and how each kitty and Silkie protocol is used.

## Map of the code

| module | what it does |
|---|---|
| `world` | tiles, level files, passability per kind of creature, gates and fences as dynamic blockers, navigation (distance fields, A*, line of sight, connected regions) |
| `sim` | the deterministic simulation: the hen, chicks, predators, hawk, fences, gates, treats, scoring, phases, recordings, an autopilot |
| `art` | procedural painting of the sprite sheets |
| `gfx` | the vector scene model (shared by both renderers), the garden scenery built from a level, sprite sheets, lettering |
| `render` | layout, the diffing renderer, uploads, status lines and overlays |
| `term` | capability discovery, input decoding, kitty graphics and OSC 777 encoding, terminal lifecycle |
| `app` | the main loop, screens, input handling, presentation effects, pacing, diagnostics |
| `bench` | measurements without a terminal |

There is one simulation. kitty and Silkie differ only in how the renderer
draws a frame and in what input arrives.

## The simulation

`sim::Game::step` advances the yard by one tick of 1/60 s. Rendering is
independent: it reads the state and the previous tick's positions and
interpolates between them, at whatever rate the terminal presents.

Everything random comes from a ChaCha8 generator seeded per game and level,
and the state is updated in a fixed order with no hash-map iteration. Given
the same level, seed and inputs, two runs produce the same state:
`Recording` stores the seed and every change of input, `silkie-scramble
replay` re-runs one and compares a hash of the final state, and the tests do
the same. Presentation effects (feathers, sparkles) use their own generator,
so drawing never perturbs the game.

A level goes through `Ready` (a countdown), `Playing`, then `Caught` (the
hen was caught: the brood scatters, a life is lost, the hen respawns
briefly invulnerable), `Cleared` or `Failed`, or `GameOver`.

### Movement

The hen, foxes and weasels move along the centre lines of corridors
(`sim/mover.rs`). A direction pressed slightly early or late still turns at
a junction (a tolerance window of a third of a tile); the leftover offset is
worked off over the next few ticks, cutting the corner. Reversing is
immediate. The hen keeps going until she can turn the way the player asked,
so key releases are not needed, and the game plays the same with or without
the kitty keyboard protocol.

A brood slows the hen a little (1.2% per chick, down to 84%): mothers wait.

### The flock

Chicks (`sim/chick.rs`) are free-moving agents with steering, not followers
of the hen's trail. Each has a personality drawn at the start (bravery,
curiosity, pace, plumage) and a state:

* **Wild**: potters about near home, pecking, until the hen walks up.
* **Following**: the procession is ordered by each chick's continuous
  path distance to the hen (distance field from the hen, refined by
  position within the tile), and each chick follows the one ahead of it, or
  the hen. When it can see its leader it pursues a point just behind it with
  velocity feed-forward (matching the leader's speed along the line between
  them, which keeps it off walls at corners). When a wall hides the leader
  it follows the hen's flow field, with string pulling: it heads for the
  furthest point along the field it can walk to in a straight line, so it
  cuts corners like an animal rather than zig-zagging tile by tile.
* **Frightened**: a predator within its fear radius (scaled by bravery,
  smaller near the hen), in sight (hedges block the view), or a hawk shadow
  over it in the open. It picks one of three responses: run to the hen and
  huddle under her (when she is close and not the one in danger), run for
  cover (tall grass or a tree's shade, by a flow field from all cover, as
  long as the way there does not lead towards the threat), or simply flee
  (choosing among sixteen directions the one that gets furthest from the
  threat without hitting a wall, with a preference for keeping its
  heading). Panic spreads: a frightened chick may frighten calm neighbours
  next tick.
* **Hiding**: crouched in cover until the danger has been gone a while. A
  hiding chick is invisible to predators unless they nearly step on it.
* **Rejoining**: makes its way back to the hen once calm.
* **Distracted**: now and then a calm, following chick spots a butterfly, a
  beetle or a treat and wanders off to peck at it (at most one at a time,
  never with danger near or on the way into the coop). A chick that pecks a
  treat earns its points.
* **Homing**, **Delivered**, **Carried**, **Lost**.

A chick left too far behind for a few seconds (more than 13 tiles by path)
gets a "?" over its head and then goes wild again where it stands.

On top of each state's goal velocity act separation (from siblings and from
the hen's feathers), a little alignment and cohesion within the procession,
repulsion from nearby walls, and a personal wobble. Velocities are limited
in acceleration and speed and integrated, then the body (a circle) is pushed
out of any impassable tile and its velocity into the wall removed.

All neighbour queries are all-pairs: with 200 chicks a whole flock update
takes about 10 µs.

### Predators

`sim/predator.rs`. A predator perceives:

* by sight: within its vision range, inside a forward cone, with line of
  sight through the tile grid (hedges, stone, trees, the coop and closed
  gates block it; flower beds, ponds and pickets do not), or anything
  within a tile and a half regardless;
* by hearing: the brood's peeping around the hen, louder the bigger it is,
  and the shrieks of panicking chicks.

Quarry it could never reach (a chick in the coop doorway, the hen behind a
closed gate) is ignored, using connected-region labels of the yard for that
creature, so predators do not camp at the coop door. It gets bored of a
spot it has just sniffed around and will not be lured back by the same
noise at once.

Targets are chosen by utility: the hen, or a chick (more attractive when it
has strayed from the hen, less when it is right by her), divided by
distance. A fox that spots something freezes with a "!" (the player's
warning, shorter for cunning foxes) and then chases. It chases where it
last saw its quarry; when that grows stale it goes to sniff about there and
then goes back to prowling, at times towards where wild chicks live.

Cunning shapes the chase: a fox of cunning 2 or more aims some tiles ahead
of the hen along her direction. On cunning 3 and up, a second fox flanks
further ahead, or, when the hen is bringing three or more chicks home, lies
in wait on her route to the coop.

A fox cut off by fences finds the route that would exist without them, and
if a fence tile is next on it, chews through (about two and a half seconds,
with an angry mark). A predator that grabs a chick carries it home at
reduced speed and cannot bite the hen; if she reaches it first it drops the
chick and is stunned. Weasels prefer stragglers, use pipes at high speed and
flee from the hen.

The hawk (`sim/hawk.rs`) waits, picks the chick furthest from the hen out in
the open, and follows it with a growing shadow for two seconds before it
stoops. It takes a chick still in the open, and stuns the hen if she is
caught in the open.

### Fences and fairness

A drawn stroke becomes the chain of tiles it crosses. Each tile is accepted
in order only if it is open ground with nobody on it, there is supply, the
hen can still reach the coop and every chick she has yet to save, and every
predator keeps at least 60 tiles of yard (or all it had): fences redirect,
they never seal. Everything else is refused and shown with a cross. Fences
last twelve seconds, weathering visibly, and foxes chew through them.

## Rendering

Each frame the app builds a `FrameModel`: the level to show, the gates and
fences, sprites (with positions interpolated between ticks), strokes being
drawn, two status lines and any overlays. The renderer keeps a model of
what the terminal holds (uploaded images, every placement with its
position, source rectangle and stacking order, the text of each status line
and overlay) and writes only the difference. When nothing changed it writes
nothing. A frame is wrapped in a synchronized update (mode 2026) when the
terminal supports it.

### Sprites

Sprites are placements of uploaded sprite sheets. Each sheet is uploaded
once; animating or moving a sprite replaces its placement (`a=p` with the
same image and placement id, which kitty moves without flicker), choosing
the frame by source rectangle. A sprite placement costs about 70 bytes and
no upload. Placements use `C=1` so the cursor never moves, and pixel offsets
within the starting cell (`X`, `Y`) for sub-cell positions. Sprites are
sorted by the screen height of their feet through the `z` key.

Sheets are painted by the art module at the size a tile has on screen,
quantized to steps about 11% apart so that resizing the window a little
sends nothing new; positions stay exact. A sheet is sent the first time a
creature needs it (the hawk's only on levels with a hawk).

Everything the game places sits below `INT32_MIN/2` in `z`, so it is drawn
under cells that have a background colour: menus and banners drawn as text
cover the garden, while ordinary cells let it show.

### Scenery

The garden is one vector description (`gfx/scenery.rs`) built from a level:
a ground scene under the creatures (lawn, paths, beds, hedges, the pond,
the coop) and a canopy scene above them (tree crowns, the tops of tall
grass, the coop's roof), so chicks disappear into the ferns. It is built
from the vector scene model in `gfx/vector.rs`, which mirrors Silkie's vector
format: groups, paths, symbols and uses, pen strokes, gradients, all keyed
by persistent ids. Paths and beds are drawn as thick round-capped strokes
between tile centres, a smooth union at a few bytes per tile; hedges are
uses of a unit-circle symbol.

The scene is either sent to Silkie as vector images or rasterized by the
game with tiny-skia, so both terminals show the same garden.

**On Silkie**, the ground and canopy are `f=200` images transmitted once per
level (about 90 KB of compressed records) and placed `c` cells wide; the
terminal draws them at whatever size they are shown, so a resize costs only
a new placement. Gates are elements in the ground scene; a gate swinging is
a patch (`a=e`) replacing a few path records. A fence going up, weathering
or coming down is a patch of four records per tile. The patches only touch
paths, which Silkie redraws within the rectangle they cover; the game never
changes a group, symbol or gradient during play, which would make Silkie
redraw the whole image. The diagnostics panel counts any edit that would.

**On kitty**, the game rasterizes the ground and canopy at the sprite step
and uploads them, placed `c` cells wide so the terminal scales them; they
are made again only when the step changes. Gates and fences are placements
of small atlases rendered from the same vector elements: every gate in four
swing stages (and with its warning flag), fence panels in two orientations
and three stages of wear. A gate swinging or a fence going up is a
placement change, never an upload.

Images go through a temporary file (`t=t`) when the terminal answered the
probe for it at start-up, which means it shares our filesystem. Otherwise
they are sent inline as PNG (or zlib-compressed RGBA for wide gamut),
chunked to 4096 bytes of base64. Over a slow link the scenery is rasterized
at reduced resolution and enlarged by the terminal, and a banner says that
the garden is being planted while the first images arrive.

### Strokes

While a fence is being drawn:

* on Silkie with a pen, the terminal draws the ink itself (below);
* on Silkie otherwise, the game adds a pen-stroke element (`k`) to the
  ground image and appends each new point (`a id=... pts=...`), a few bytes
  per sample, redrawn by Silkie in the small rectangle around the new piece;
* on kitty, the game places copies of a small ink-dot sprite along the
  stroke.

When the stroke ends the simulation decides what becomes fence; the stroke
is then removed and the fence drawn. A stroke that never leaves a third of
a tile is a tap: on the latched gate the hen can reach (the one Space
would work, `sim::gate::latch_in_reach`) it works the gate instead, with
any terminal ink removed; anywhere else it is a one-tile fence.

### Text

The status lines and overlays are terminal text, rewritten only when they
change. Overlays are boxes of cells with a dark background; on Silkie they
also carry a background alpha (`SGR 70`), so the garden shows through the
pause menu and banners. The title lettering is a vector image on Silkie
(the typeface's outlines as path records with gradient fills) and a bitmap
elsewhere, from the same scene.

## Capability discovery

`term/caps.rs`. Nothing is assumed from the terminal's name or environment.
Queries go out in two rounds, each ending with a primary device attributes
request; every terminal answers that, and replies come in order, so once
it is answered anything unanswered is unsupported. Each round also has a
2.5 s timeout for terminals or links that answer nothing.

Round one: a graphics query (`a=q`), a temporary-file medium query, the
kitty keyboard flags (`CSI ? u`), DECRQM for synchronized output (2026) and
SGR pixel mouse reports (1016), the cell and window size (`CSI 16 t`,
`CSI 14 t`), and Silkie's capability listing (`OSC 777 ; caps`).

Round two: the OSC 777 selectors the game uses, for their details: only
those the listing names, or all of them if there was no listing. Then
probes for vector images (`f=200` with the specification's one-path scene),
HDR images (`f=64` with one pixel) and the wide-gamut key (`G=1`); and a
pointer shape query (`OSC 22 ; ?crosshair,image:1:0:0`). Formats the listing
does not name are not probed.

Locally discovery takes from a couple of milliseconds to a few tens,
depending on how busy the terminal is; over a link, about two round trips.
`--report` prints what came back, as does the title screen's C key.

## The protocols, one by one

### kitty

* **Graphics**: transmission (`a=t`, `f=32`, `f=100`, `o=z`, `t=d`/`t=t`,
  chunking with `m`), placement with ids (`i`, `p`), source rectangles
  (`x,y,w,h`), cell offsets (`X,Y`), scaling (`c`), stacking (`z`), no
  cursor movement (`C=1`), deletion of placements (`d=i`) and images
  (`d=I`), responses suppressed for successes (`q=1`) and errors handled: an
  `ENOENT` (an image evicted under storage pressure) makes the renderer send
  that image again.
* **Keyboard**: flags 1, 2 and 8 are pushed (disambiguated escape codes,
  event types, all keys as escape codes) and popped on exit. Release events
  are ignored by the game logic; repeat events never flood movement.
  Terminals without the protocol fall back to legacy decoding, with a 30 ms
  wait to tell a lone Escape from the start of a sequence.
* **Mouse**: button-event tracking (1002) with SGR reports (1006), and pixel
  positions (1016) when supported, for precise fence strokes.
* **Synchronized output** (2026) around each frame.
* **Focus reports** (1004): the game pauses when the window loses focus.
* **Pointer shapes** (OSC 22): a crosshair while playing; pushed and popped.

### Silkie

* **Extension capabilities** (`caps`): the listing decides which selectors
  are queried. On exit every mode the game turned on is turned off
  explicitly, ink first (it depends on tablet reports).
* **Vector images** (`f=200`): the garden, the logo, live strokes, gates and
  fences, as described above.
* **Terminal-side ink**: when the terminal can ink and reports tools, the
  game sets tablet-pointer mode 1 and gives a brush for the ground image's
  ink group, with a block of a million element ids starting at 10,000,000
  that the game uses for nothing else, the pen, brush, pencil and airbrush
  tools, and widths in the image's units. The brush is sent once the vector
  image exists, read back to see whether it was granted, and sent again
  after a level change or a suspend. A pen `down` whose report carries an
  `ink` id is drawn by the terminal; the game records the stroke's points
  from the reports (converting 1/256-pixel positions into yard
  coordinates), and when the simulation has judged it, deletes the ink
  element with an `x id=` patch and draws the fence. A cancelled stroke's ink
  is removed by the terminal. A pen stroke that starts while not playing has
  its ink deleted at once. A `down` without an `ink` field (ids exhausted,
  ink refused) is drawn by the game instead.
* **Tablet pointer**: mode 1 (tools) when tools are reported, plus 2
  (fingers) only on a touchscreen, since touchpad fingers report positions
  on the pad. Pens and touchscreen fingers draw fences.
* **Frame presentation**: drives pacing (below).
* **Low latency**: on while playing, off in menus and when paused (it costs
  the terminal CPU).
* **Display**: queried at start and its events followed: the refresh period
  paces frames, the pixel scale sizes the pointer image, the gamut enables
  wide-gamut sprites, and an e-ink display gets slow refreshes and no
  particle effects. The headroom is only reported (see HDR).
* **HDR**: when `f=64` is accepted, the white-hot cores of sparkles, stars
  and hearts are sent as an HDR overlay (`hdr_glints` in
  `render/upload.rs`), white at three times SDR white, placed one `z`
  above each such sprite. The game does not wait for headroom: Silkie's
  `hdr` option defaults to `no`, and with `auto` the terminal reports a
  headroom of 1 until something brighter than white is on screen, then
  fades it in over about 0.75 s (1.29, 1.97, 2.89, 4 in display events, 250
  ms apart, on a MacBook Pro). A game that waited for headroom would wait
  forever. Instead the overlay is built to be harmless without headroom:
  it covers only opaque pixels whose every linear component is at least
  0.92, so clipped to SDR white (`T=0`, the default, never `T=1`, whose
  knee would alter colours) it changes them by at most 8% and pure white
  not at all; translucent edges are left out, because white over a pixel
  that already lets the garden through would show. The sprite itself is
  unchanged and keeps its wide-gamut colours, which an extended-sRGB copy
  could not do without values above one. The terminal clips the glints to
  whatever headroom it has, so headroom changes need no new upload.
* **Wide gamut**: on a Display P3 display, when the terminal accepted the
  `G=1` probe, sprite and scenery bitmaps are re-expressed in Display P3
  with a modest boost to colours that were already vivid (greys and pale
  colours, like the hen, are unchanged).
* **Cell background alpha**: translucent menus.
* **Haptics**: `alignment` when a chick joins, `complete` when a brood is
  delivered, `click` for a gate, `generic` for a fence or a catch; at most
  one every 80 ms, and only when the terminal said it can play them.
* **Image pointer**: a little wooden stake as the pointer while playing.

## Pacing and timing

`app/pacer.rs`. The simulation always ticks at 60 Hz from the monotonic
clock; rendering interpolates between the last two ticks.

Without presentation reports, frames go out on a timer at the display
period (60 Hz unless the display protocol says otherwise).

With Silkie's frame reports, a report means that a frame containing new
output from the game was handed to the display. It does not say which write
it held: writes made between two refreshes are presented together, and over
a network the report for a frame arrives while newer frames are in flight.
So the game measures the link separately, every two seconds, with an OSC
777 mode read (answered at once, not at the next presentation), and paces
in one of two ways:

* **strict**, when the round trip is shorter than a display period: write a
  frame, wait for the next report, write the next. The game draws exactly as
  fast as the terminal presents, and the time from a write to its report is
  a real measurement;
* **pipelined**, over slower links: write at the presentation rate, with no
  more than a round trip's worth of frames on the way, and stop writing when
  reports stop coming (a congested link or a hidden window).

A report may never come (a hidden window presents nothing), so waiting
always has a timeout. Frames are drawn no faster than the terminal has been
presenting them: Silkie on a 120 Hz display may present at 60 Hz, and frames
beyond that would be wasted.

## Measurements and their clocks

The diagnostics panel (F2) and `bench` keep clocks apart:

* our monotonic clock: simulation cost per tick, building a frame, the time
  `write` and `flush` take (when the bytes left the game, not when they were
  drawn), the time from a key arriving to its frame being written, the link
  round trip, and the time from a write to its report in strict mode;
* the terminal's clock (the `t` of its reports): the interval between
  presented frames, and the time from a pen sample to the presentation of
  the frame that drew the game's response, both subtracted only from other
  terminal timestamps.

None of these is the moment light leaves the screen: a report is when a
frame was handed to the display system, which shows it at the next refresh
or later. Keys carry no timestamp, so key latency is measured on our side
only.

## Terminal lifecycle

`term/tty.rs`. crossterm puts the terminal in raw mode. Everything the game
enables (the alternate screen, the hidden cursor, the keyboard flags, mouse
modes, focus reports, OSC 777 modes, pointer shapes, every image it
created) is recorded, and cleanup sends exactly the matching disables: on
quitting, on SIGTERM, SIGINT or SIGHUP, and from a panic hook that writes
the same bytes before the panic message.

Ctrl+Z (a key event in raw mode) and SIGTSTP suspend the game: images are
deleted, every mode is turned off, the terminal leaves raw mode and the
process stops. On SIGCONT it re-enters raw mode, turns its modes on again
(a shell prompt in between resets every OSC 777 mode anyway) and redraws
everything. SIGWINCH, debounced, recomputes the layout; images are placed
anew and only re-sent when the sprite resolution step changes. A window
smaller than 20x8 cells has no layout: every placement is deleted (the
images stay), a game in progress pauses, and a centred message asks for a
bigger window, redrawn at each resize until the garden fits again.

stdin is read on its own thread; signals arrive on another; the main loop
waits on a channel with a deadline (the next tick or frame).

## Testing

`cargo test` runs about a hundred tests, none needing a terminal:

* protocol encoding: OSC 777 frames (including the 127-byte limit and the
  largest ink brush), graphics commands, chunking, compression, vector
  records matching the specification's examples;
* input decoding: legacy and kitty keys, SGR mouse in cells and pixels,
  graphics replies, OSC 777 reports with either terminator, DECRPM, window
  sizes, XTGETTCAP, pointer replies, and every possible split of a sequence
  across reads;
* capability negotiation against scripted kitty-like, Silkie-like and silent
  terminals, including timeouts and unknown protocol versions;
* the simulation, headless with fixed seeds: determinism, replay of a
  recording (also through JSON), lane movement and buffered turns,
  gathering, a brood following around a block without overlapping or
  entering walls, delivery and batch scoring, level completion, sight
  blocked by hedges, a fox hunting down a standing hen, a closed gate
  rerouting a fox, fences that may not strand chicks or pen a fox, fences
  wearing out, a frightened chick taking cover, rescuing a chick from a
  fox's mouth, the autopilot clearing a level;
* rendering, by decoding the bytes it writes: uploads only on the first
  frame and none when nothing changed, a moving sprite costing one
  placement, removed sprites deleted, resizes rescaling or not as they
  should, vector scenery sent once and re-placed on resize, fences as
  vector patches on Silkie and atlas placements on kitty, text and overlays
  diffed, teardown deleting every image, evicted images re-sent, HDR
  glints placed over their sprites;
* the terminal restore sequence, pacing, strokes (and taps), half floats,
  HDR glints that leave the art unchanged when clipped to SDR white, the
  P3 conversion, high scores, layout, word wrapping.

The game was also run in kitty 0.48.2 and Silkie 0.49.2 on macOS, locally
and behind `netsim`, with screenshots of the window checked at each step:
discovery, the title, play, mouse-drawn fences in both terminals, the pause
menu, Ctrl+Z and `fg`, resizing and quitting. Unit tests alone are not a
claim of terminal compatibility; these runs are.

What could not be tried here, for want of the hardware: drawing with a pen
(so terminal-side ink has been exercised only through the protocol code and
tests, not with a real tablet), a touchscreen, and how the haptic taps
feel.

HDR was checked on a MacBook Pro display with a ScreenCaptureKit capture of
the game's window in half floats (extended linear sRGB), taken several
times a second during autoplay: with Silkie's `hdr` set to `always` and to
`auto`, the glints showed as a few hundred pixels at exactly 3.0 times SDR
white, with no graphics errors; with `hdr=no` no pixel was above SDR white.
The capture tool was first checked against text at 400% (`SGR 56:400`),
which measured 4.0, the headroom Silkie reported.
