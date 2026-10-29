# Asset provenance

Everything the game shows or plays is listed here.

## Sprites (`assets/sprites/*.png`, `manifest.json`)

Original artwork made for this game. The sheets are painted by the code in
`src/art/` (shapes, gradients and strokes drawn with tiny-skia from a fixed
seed) and exported with `cargo run --release --bin artgen`. Running it
again reproduces the same files.

* `hen`: the Silkie hen (walking in four directions, idling, blinking,
  pecking, fluffing up, startled, celebrating, turning, dizzy).
* `chick`: Silkie chicks in four plumage colours (walking, pecking, scared,
  hiding, happy, carried).
* `fox`, `weasel`, `hawk`: the predators, and the hawk's shadow.
* `items`: worm, snail, corn cob, golden egg, butterfly, ladybird.
* `fx`: emote bubbles, feathers, dust, sparkles, stars, the fence ink dot,
  the refused-fence cross, a tick and a heart.

At run time the game paints the same sheets at exactly the size a tile has
on screen (sharper, and smaller to send than resampling the files). With
`--png-sprites` it loads these PNG files instead, which is how replacement
artwork can be tried without code changes.

The characters, garden and presentation were designed to be original. They
do not reproduce the characters, mazes, animations or look of any existing
game.

## Garden scenery

Drawn by `src/gfx/scenery.rs` from each level's map: vector shapes for
lawns, paths, hedges, beds, the pond, trees, the coop, gates and fences.
Original.

## Typeface

`assets/fonts/LilitaOne-Regular.ttf`: Lilita One by Juan Montoreano,
licensed under the SIL Open Font License 1.1 (`assets/fonts/OFL.txt`),
obtained from the Google Fonts repository. Used for the title lettering and
the score pop-ups. "Lilita" is a Reserved Font Name; the font is used
unmodified.

## Levels

`assets/levels/*.txt`: original layouts made for this game. Level 1 was laid
out by hand; levels 2 to 6 were composed with a small layout script (level 5
from a seeded maze carver with extra loops) and then adjusted by hand. None
reproduces an existing game's maze.

## Sound

There is none: a terminal has no audio channel, and playing sound on the
machine running the game would be wrong over SSH.
