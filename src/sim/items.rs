//! Treats that turn up in the yard: worth points to the hen, and
//! irresistible to a curious chick (which earns the points too).

use super::rules::*;
use super::{Game, SimEvent};
use crate::world::{Mover, TilePos};
use glam::Vec2;
use rand::RngExt;
use serde::{Deserialize, Serialize};

/// Design rule: at most this many treats lie about at once.
pub const MAX_ITEMS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Worm,
    Corn,
    Snail,
    GoldenEgg,
}

impl ItemKind {
    pub fn points(self) -> u32 {
        match self {
            ItemKind::Worm => 50,
            ItemKind::Snail => 75,
            ItemKind::Corn => 100,
            ItemKind::GoldenEgg => 500,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u32,
    pub kind: ItemKind,
    pub pos: Vec2,
    pub age: f32,
}

pub fn update(game: &mut Game) {
    game.item_timer -= DT;
    if game.item_timer <= 0.0 {
        game.item_timer = ITEM_INTERVAL;
        if game.items.len() < MAX_ITEMS {
            spawn(game);
        }
    }
    let mut expired = Vec::new();
    for it in &mut game.items {
        it.age += DT;
        if it.age > ITEM_LIFETIME {
            expired.push(it.id);
        }
    }
    for id in expired {
        game.items.retain(|i| i.id != id);
        game.emit(SimEvent::ItemExpired { item: id });
    }
    if game.hen.alive {
        let hen = game.hen.pos();
        if let Some(i) = game.items.iter().position(|it| it.pos.distance(hen) < ITEM_PICK_DIST) {
            let it = game.items.remove(i);
            collected(game, it, false);
            game.hen.peck = 0.0;
        }
    }
}

fn collected(game: &mut Game, it: Item, by_chick: bool) {
    let points = it.kind.points();
    game.add_score(points);
    if it.kind == ItemKind::GoldenEgg {
        game.fence_supply = game.level.fence_supply as f32;
    }
    game.emit(SimEvent::ItemCollected {
        item: it.id,
        points,
        pos: it.pos,
        by_chick,
    });
}

pub fn collect_by_chick(game: &mut Game, item: u32, chick: usize) {
    if let Some(i) = game.items.iter().position(|it| it.id == item) {
        let it = game.items.remove(i);
        collected(game, it, true);
        if let Some(c) = game.chicks.get_mut(chick) {
            c.joy = 1.0;
        }
    }
}

fn spawn(game: &mut Game) {
    let hen = game.hen.pos();
    let taken: Vec<Vec2> = game.items.iter().map(|i| i.pos).collect();
    let far_enough = |p: TilePos| p.center().distance(hen) >= 4.0 && !taken.iter().any(|t| t.distance(p.center()) < 1.0);
    let mut spots: Vec<TilePos> = game.level.item_spawns.iter().copied().filter(|&p| far_enough(p)).collect();
    if spots.is_empty() {
        spots = game
            .topo
            .grid
            .positions()
            .filter(|&p| game.topo.grid.get(p).is_ground() && game.topo.passable(p, Mover::Hen) && far_enough(p))
            .collect();
    }
    if spots.is_empty() {
        return;
    }
    let at = spots[game.rng.random_range(0..spots.len())];
    let roll: f32 = game.rng.random_range(0.0..1.0);
    let kind = if roll < 0.45 {
        ItemKind::Worm
    } else if roll < 0.75 {
        ItemKind::Corn
    } else if roll < 0.95 {
        ItemKind::Snail
    } else {
        ItemKind::GoldenEgg
    };
    let id = game.alloc_id();
    let pos = at.center();
    game.items.push(Item { id, kind, pos, age: 0.0 });
    game.emit(SimEvent::ItemSpawned { item: id, pos });
}
