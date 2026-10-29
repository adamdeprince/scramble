//! High scores, kept in the user's data directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The table shows the best ten.
pub const TABLE_SIZE: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    pub score: u32,
    pub level: u32,
    pub chicks: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scores {
    pub entries: Vec<Entry>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Scores {
    pub fn load() -> Scores {
        let path = directories::ProjectDirs::from("com", "goblinreactor", "silkie-scramble").map(|d| d.data_dir().join("scores.json"));
        let mut s: Scores = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.path = path;
        s.entries.sort_by_key(|e| std::cmp::Reverse(e.score));
        s
    }

    pub fn in_memory() -> Scores {
        Scores::default()
    }

    pub fn best(&self) -> u32 {
        self.entries.first().map_or(0, |e| e.score)
    }

    /// Where a score would rank, if it makes the table.
    pub fn rank(&self, score: u32) -> Option<usize> {
        if score == 0 {
            return None;
        }
        let r = self.entries.iter().position(|e| score > e.score).unwrap_or(self.entries.len());
        (r < TABLE_SIZE).then_some(r)
    }

    pub fn add(&mut self, e: Entry) -> std::io::Result<()> {
        let r = self.rank(e.score).unwrap_or(self.entries.len());
        self.entries.insert(r.min(self.entries.len()), e);
        self.entries.truncate(TABLE_SIZE);
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(p, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_keeps_the_best_ten() {
        let mut s = Scores::in_memory();
        for i in 0..15u32 {
            s.add(Entry {
                name: format!("p{i}"),
                score: i * 100 + 50,
                level: 1,
                chicks: i,
            })
            .unwrap();
        }
        assert_eq!(s.entries.len(), TABLE_SIZE);
        assert_eq!(s.best(), 1450);
        assert!(s.entries.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(s.rank(10), None);
        assert_eq!(s.rank(5000), Some(0));
    }
}
