//! Level files: a small header of `key: value` lines followed by `map:` and
//! the yard drawn in characters. See `assets/levels/README.md` for the
//! legend.

use super::nav;
use super::{Grid, Mover, Tile, TilePos, Topology};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    pub tagline: String,
    pub grid: Grid,
    pub hen_start: TilePos,
    pub chick_spawns: Vec<TilePos>,
    pub chick_count: u32,
    pub fox_dens: Vec<TilePos>,
    pub fox_count: u32,
    pub weasel_burrows: Vec<TilePos>,
    pub weasel_count: u32,
    pub hawk: bool,
    pub item_spawns: Vec<TilePos>,
    pub fence_supply: u32,
    /// How cleverly predators hunt, 1 (dozy) to 5 (cunning).
    pub cunning: u32,
    /// Seconds a timed gate stays in each state.
    pub gate_period: f32,
    /// Fraction of the brood that must reach the coop to move on.
    pub quota: f32,
    pub doors: Vec<TilePos>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelError(pub String);

impl fmt::Display for LevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LevelError {}

fn err<T>(msg: impl Into<String>) -> Result<T, LevelError> {
    Err(LevelError(msg.into()))
}

/// The levels shipped with the game, in play order.
pub const BUILTIN: &[(&str, &str)] = &[
    ("meadow", include_str!("../../assets/levels/01-meadow.txt")),
    ("orchard", include_str!("../../assets/levels/02-orchard.txt")),
    ("kitchen-garden", include_str!("../../assets/levels/03-kitchen-garden.txt")),
    ("pond", include_str!("../../assets/levels/04-pond.txt")),
    ("hedge-maze", include_str!("../../assets/levels/05-hedge-maze.txt")),
    ("dusk", include_str!("../../assets/levels/06-dusk.txt")),
];

pub fn builtin(index: usize) -> Option<Level> {
    BUILTIN
        .get(index)
        .map(|(name, text)| Level::parse(text).unwrap_or_else(|e| panic!("built-in level {name}: {e}")))
}

impl Level {
    pub fn parse(text: &str) -> Result<Level, LevelError> {
        let mut name = String::from("Unnamed yard");
        let mut tagline = String::new();
        let mut chicks = None;
        let mut foxes = 1;
        let mut weasels = 0;
        let mut hawk = false;
        let mut fence = 8;
        let mut cunning = 1;
        let mut gate_period = 7.0f32;
        let mut quota = 0.5f32;
        let mut rows: Vec<&str> = Vec::new();
        let mut in_map = false;
        for (lineno, raw) in text.lines().enumerate() {
            if in_map {
                let r = raw.trim_end();
                if r.is_empty() {
                    continue;
                }
                rows.push(r);
                continue;
            }
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line == "map:" {
                in_map = true;
                continue;
            }
            let Some((k, v)) = line.split_once(':') else {
                return err(format!("line {}: expected `key: value`", lineno + 1));
            };
            let v = v.trim();
            let num = |what: &str| -> Result<f32, LevelError> {
                v.parse::<f32>()
                    .map_err(|_| LevelError(format!("line {}: {what} must be a number", lineno + 1)))
            };
            match k.trim() {
                "name" => name = v.to_string(),
                "tagline" => tagline = v.to_string(),
                "chicks" => chicks = Some(num("chicks")? as u32),
                "foxes" => foxes = num("foxes")? as u32,
                "weasels" => weasels = num("weasels")? as u32,
                "hawk" => hawk = matches!(v, "yes" | "true" | "1"),
                "fence" => fence = num("fence")? as u32,
                "cunning" => cunning = (num("cunning")? as u32).clamp(1, 5),
                "gate_period" => gate_period = num("gate_period")?.max(1.0),
                "quota" => quota = num("quota")?.clamp(0.0, 1.0),
                other => return err(format!("line {}: unknown key {other}", lineno + 1)),
            }
        }
        if rows.is_empty() {
            return err("level has no map");
        }
        let width = rows[0].chars().count() as i32;
        let height = rows.len() as i32;
        if width < 5 || height < 5 {
            return err("map is too small");
        }
        let mut grid = Grid::new(width, height, Tile::Hedge);
        let mut hen = None;
        let mut chick_spawns = Vec::new();
        let mut dens = Vec::new();
        let mut burrows = Vec::new();
        let mut items = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            if row.chars().count() as i32 != width {
                return err(format!(
                    "map row {} has {} columns, expected {width}",
                    y + 1,
                    row.chars().count()
                ));
            }
            for (x, c) in row.chars().enumerate() {
                let p = TilePos::new(x as i32, y as i32);
                let tile = match c {
                    'H' => {
                        if hen.replace(p).is_some() {
                            return err("map has more than one hen start (H)");
                        }
                        Tile::Path
                    }
                    'c' => {
                        chick_spawns.push(p);
                        Tile::Lawn
                    }
                    'k' => {
                        chick_spawns.push(p);
                        Tile::Cover
                    }
                    '*' => {
                        items.push(p);
                        Tile::Path
                    }
                    'W' => {
                        burrows.push(p);
                        Tile::Pipe
                    }
                    'F' => {
                        dens.push(p);
                        Tile::Den
                    }
                    other => match Tile::from_char(other) {
                        Some(t) => t,
                        None => return err(format!("map row {} has unknown tile {other:?}", y + 1)),
                    },
                };
                grid.set(p, tile);
            }
        }
        // A chick spawn marked on a path takes the ground of the path or
        // lawn around it, so markers leave no holes in either.
        for &c in chick_spawns.iter().chain(items.iter()) {
            if !matches!(grid.get(c), Tile::Lawn | Tile::Path) {
                continue;
            }
            let (mut path, mut lawn) = (0, 0);
            for d in super::Dir::ALL {
                match grid.get(c.step(d)) {
                    Tile::Path | Tile::Den | Tile::Door => path += 1,
                    Tile::Lawn => lawn += 1,
                    _ => {}
                }
            }
            grid.set(c, if path > lawn { Tile::Path } else { Tile::Lawn });
        }
        let Some(hen_start) = hen else {
            return err("map has no hen start (H)");
        };
        for p in grid.positions().collect::<Vec<_>>() {
            let edge = p.x == 0 || p.y == 0 || p.x == width - 1 || p.y == height - 1;
            if edge && !grid.get(p).is_wall() {
                return err(format!("the yard is open at {},{}", p.x, p.y));
            }
        }
        let doors: Vec<TilePos> = grid.positions().filter(|&p| grid.get(p) == Tile::Door).collect();
        if doors.is_empty() {
            return err("map has no coop door (D)");
        }
        if chick_spawns.is_empty() {
            return err("map has no chick spawns (c or k)");
        }
        if foxes > 0 && dens.is_empty() {
            return err("level has foxes but no den (F)");
        }
        if weasels > 0 && burrows.is_empty() {
            return err("level has weasels but no burrow (W)");
        }
        let level = Level {
            name,
            tagline,
            grid,
            hen_start,
            chick_count: chicks.unwrap_or(chick_spawns.len() as u32).max(1),
            chick_spawns,
            fox_dens: dens,
            fox_count: foxes,
            weasel_burrows: burrows,
            weasel_count: weasels,
            hawk,
            item_spawns: items,
            fence_supply: fence,
            cunning,
            gate_period,
            quota,
            doors,
        };
        level.check_playable()?;
        Ok(level)
    }

    /// Every chick, the coop and every den must be reachable with all gates
    /// open; otherwise the level could never be finished.
    pub fn check_playable(&self) -> Result<(), LevelError> {
        let topo = Topology::new(self.grid.clone());
        let hen_field = nav::DistField::compute(&topo, &[self.hen_start], Mover::Hen);
        for &c in &self.chick_spawns {
            if !hen_field.reachable(c) {
                return err(format!("chick spawn {},{} cannot be reached", c.x, c.y));
            }
        }
        if !self.doors.iter().any(|&d| hen_field.reachable(d)) {
            return err("the coop door cannot be reached");
        }
        for &d in &self.fox_dens {
            let f = nav::DistField::compute(&topo, &[d], Mover::Fox);
            let near_hen = f.reachable(self.hen_start) || self.chick_spawns.iter().any(|&c| f.reachable(c));
            if !near_hen {
                return err(format!("the den at {},{} is sealed off", d.x, d.y));
            }
        }
        Ok(())
    }

    pub fn to_ascii(&self) -> String {
        let mut s = String::new();
        for y in 0..self.grid.height {
            for x in 0..self.grid.width {
                s.push(self.grid.get(TilePos::new(x, y)).to_char());
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = "name: Test\nchicks: 2\nfoxes: 1\nmap:\n#######\n#H.c.F#\n#.###.#\n#D..c.#\n#######\n";

    #[test]
    fn parses_small_level() {
        let l = Level::parse(SMALL).unwrap();
        assert_eq!(l.name, "Test");
        assert_eq!(l.grid.width, 7);
        assert_eq!(l.chick_spawns.len(), 2);
        assert_eq!(l.fox_dens, vec![TilePos::new(5, 1)]);
        assert_eq!(l.doors, vec![TilePos::new(1, 3)]);
    }

    #[test]
    fn rejects_open_border() {
        let bad = SMALL.replace("#H.c.F#", ".H.c.F#");
        assert!(Level::parse(&bad).is_err());
    }

    #[test]
    fn rejects_unreachable_chick() {
        let bad = "map:\n#######\n#H...F#\n#####.#\n#Dc#..#\n#######\n";
        let e = Level::parse(bad).unwrap_err();
        assert!(e.0.contains("chick spawn"), "{e}");
    }

    #[test]
    fn builtin_levels_parse_and_are_playable() {
        for i in 0..BUILTIN.len() {
            let l = builtin(i).unwrap();
            assert!(l.chick_count >= 1);
        }
    }
}
