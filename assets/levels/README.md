# Level files

A level is a text file: `key: value` lines, then `map:` and the yard drawn
one character per tile. Lines starting with `;` are comments. The levels the
game ships with are listed in `src/world/level.rs` (`BUILTIN`) and compiled
into the binary.

```
name: Morning Meadow
tagline: Seven chicks are out on the lawn. The fox is still yawning.
chicks: 7
foxes: 1
weasels: 0
hawk: no
fence: 8
cunning: 1
quota: 0.5
map:
pppppppppppppppppppppppppppppppp
p,,,,,,,,,,,,,,,c,,,,,,,,,,,,,,p
...
```

| key | meaning | default |
|---|---|---|
| `name`, `tagline` | shown when the level starts | |
| `chicks` | number of chicks (spawn markers are reused round-robin) | one per marker |
| `foxes`, `weasels` | how many of each (spawning at dens and burrows) | 1, 0 |
| `hawk` | whether a hawk patrols (`yes`/`no`) | no |
| `fence` | fence supply, in tiles | 8 |
| `cunning` | how cleverly predators hunt, 1 to 5 | 1 |
| `gate_period` | seconds a timed gate stays open or shut | 7 |
| `quota` | fraction of the chicks that must reach the coop | 0.5 |

## Tiles

| char | tile | who can pass |
|---|---|---|
| `.` | dirt path | everyone; open to the sky |
| `,` | lawn | everyone; open to the sky |
| `b` | tall grass | everyone; hides chicks from the hawk |
| `#` | hedge | nobody; blocks sight |
| `f` | flower bed | nobody; low, does not block sight |
| `p` | picket fence | nobody; low |
| `w` | pond | nobody; low |
| `v` | vegetable patch | nobody; low |
| `s` | stone wall | nobody; blocks sight |
| `T` | tree | nobody; blocks sight; shades the tiles beside it |
| `C` | the coop | nobody |
| `D` | coop door | the hen and chicks only |
| `=` | drain pipe | weasels only |
| `G` | timed gate | everyone, while open |
| `L` | latched gate | everyone, while open; the hen works it with Space |

Markers, each standing on a tile of its own:

| char | marker | tile underneath |
|---|---|---|
| `H` | the hen's start | path |
| `c` | a chick | path or lawn, whichever surrounds it |
| `k` | a chick hiding in tall grass | tall grass |
| `F` | a fox den | den (walkable) |
| `W` | a weasel burrow | pipe |
| `*` | where treats turn up | path |

## Rules the loader checks

* The map is rectangular and closed: every edge tile is a wall.
* There is exactly one `H`, at least one `D`, at least one chick, and dens or
  burrows for the predators the header asks for.
* With every gate open, the hen can reach every chick and the coop door,
  and every den can reach the hen's start or a chick.

Gates belong in corridors: walls on two opposite sides, open on the other
two. Pipes should join two parts of the yard (or a burrow to the yard). The
built-in levels follow these conventions; `cargo test` parses and checks all
of them.
