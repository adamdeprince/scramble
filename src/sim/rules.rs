//! Tuning constants. Distances are in tiles, times in seconds, speeds in
//! tiles per second.

/// Simulation rate. Rendering is independent of it.
pub const TICK_HZ: u32 = 60;
pub const DT: f32 = 1.0 / TICK_HZ as f32;

// The hen.
pub const HEN_SPEED: f32 = 4.4;
/// Each chick in tow slows the hen a little: mothers wait for stragglers.
pub const HEN_BROOD_SLOWDOWN: f32 = 0.012;
pub const HEN_MIN_SPEED_FACTOR: f32 = 0.84;
/// How far from a tile centre a turn may still be taken (pre/post-turn).
pub const TURN_WINDOW: f32 = 0.34;
pub const HEN_RADIUS: f32 = 0.34;
pub const RESPAWN_INVULNERABLE: f32 = 2.5;
pub const STUN_TIME: f32 = 1.2;

// Chicks.
pub const CHICK_RADIUS: f32 = 0.16;
pub const CHICK_ACCEL: f32 = 22.0;
pub const CHICK_SPEED_FACTOR: f32 = 1.32;
pub const CHICK_MAX_SPEED: f32 = 6.2;
pub const CHICK_WANDER_SPEED: f32 = 0.9;
pub const CHICK_FLEE_SPEED: f32 = 4.6;
pub const JOIN_DIST: f32 = 0.95;
pub const FOLLOW_GAP: f32 = 0.6;
pub const HEN_GAP: f32 = 0.72;
pub const SEPARATION_RADIUS: f32 = 0.38;
pub const FEAR_RADIUS: f32 = 2.6;
pub const HAWK_FEAR_RADIUS: f32 = 3.2;
pub const CALM_TIME: f32 = 1.3;
pub const HIDE_MIN_TIME: f32 = 1.0;
pub const PANIC_SPREAD_RADIUS: f32 = 1.3;
pub const PANIC_SPREAD_CHANCE: f64 = 0.55;
pub const STRAY_DIST: u32 = 13;
pub const STRAY_TIME: f32 = 4.0;
pub const DISTRACT_CHANCE_PER_SEC: f64 = 0.045;
pub const DISTRACT_RANGE: f32 = 2.6;
pub const DISTRACT_COOLDOWN: f32 = 9.0;
pub const DISTRACT_PECK_TIME: f32 = 1.4;
pub const DISTRACT_GIVE_UP: f32 = 4.0;
pub const MAX_DISTRACTED: usize = 1;
pub const HOMING_HEN_DIST: f32 = 1.7;
pub const HOMING_CHICK_PATH: u32 = 5;
pub const DELIVER_RADIUS: f32 = 0.38;

// Predators.
pub const CATCH_DIST: f32 = 0.56;
pub const GRAB_DIST: f32 = 0.44;
pub const RESCUE_DIST: f32 = 0.7;
pub const FOX_PATROL_SPEED: f32 = 2.3;
pub const FOX_CHASE_SPEED: f32 = 3.75;
pub const FOX_SPEED_PER_CUNNING: f32 = 0.16;
pub const FOX_CARRY_FACTOR: f32 = 0.72;
pub const FOX_VISION: f32 = 6.0;
pub const FOX_VISION_PER_CUNNING: f32 = 0.6;
pub const FOX_CLOSE_SENSE: f32 = 1.6;
pub const FOX_ALERT_TIME: f32 = 0.65;
pub const FOX_ALERT_PER_CUNNING: f32 = 0.08;
pub const FOX_MEMORY: f32 = 3.0;
pub const FOX_MEMORY_PER_CUNNING: f32 = 0.6;
pub const FOX_SNIFF_TIME: f32 = 2.2;
pub const FOX_STUN_TIME: f32 = 2.4;
pub const FOX_RETREAT_TIME: f32 = 3.5;
pub const FOX_GNAW_TIME: f32 = 2.4;
pub const WEASEL_SPEED: f32 = 4.6;
pub const WEASEL_PIPE_SPEED: f32 = 6.5;
pub const WEASEL_SCARE_DIST: f32 = 2.6;
pub const WEASEL_LURK_TIME: f32 = 3.0;
pub const NOISE_BASE: f32 = 1.4;
pub const NOISE_PER_CHICK: f32 = 0.36;
pub const NOISE_PANIC: f32 = 4.2;

// The hawk.
pub const HAWK_FIRST_DELAY: f32 = 14.0;
pub const HAWK_COOLDOWN_MIN: f32 = 12.0;
pub const HAWK_COOLDOWN_MAX: f32 = 20.0;
pub const HAWK_WARNING: f32 = 2.0;
pub const HAWK_STRIKE_RADIUS: f32 = 0.9;

// Fences.
pub const FENCE_LIFETIME: f32 = 12.0;
pub const FENCE_REGEN_TIME: f32 = 4.0;
/// A predator keeps at least this many tiles of yard when fenced, so fences
/// redirect predators but never pen them in.
pub const FENCE_MIN_PREDATOR_REGION: usize = 60;
pub const FENCE_AGENT_CLEARANCE: f32 = 0.5;

// Gates.
pub const GATE_WARNING: f32 = 1.2;
pub const LATCH_REACH: f32 = 1.25;

// Items.
pub const ITEM_FIRST_DELAY: f32 = 8.0;
pub const ITEM_INTERVAL: f32 = 11.0;
pub const ITEM_LIFETIME: f32 = 14.0;
pub const ITEM_PICK_DIST: f32 = 0.6;

// Scoring.
pub const POINTS_JOIN: u32 = 10;
pub const POINTS_DELIVER: u32 = 100;
/// Each extra chick in one delivery adds this fraction to the multiplier.
pub const BATCH_BONUS: f32 = 0.25;
pub const BATCH_WINDOW: f32 = 2.2;
pub const POINTS_RESCUE: u32 = 200;
pub const POINTS_PERFECT: u32 = 1000;
pub const TIME_BONUS_MAX: u32 = 3000;
pub const TIME_BONUS_PER_SEC: u32 = 15;
pub const EXTRA_LIFE_EVERY: u32 = 10_000;
pub const START_LIVES: u32 = 3;

// Phases.
pub const READY_TIME: f32 = 2.2;
pub const CAUGHT_TIME: f32 = 2.4;
pub const CLEARED_TIME: f32 = 3.5;
