//! Enemies: a shared state-machine framework and four types.
//!
//! Every enemy has a [`Brain`] (kind, state, tick counter) and the same body
//! components (`Hurtbox`, `Health`, contact `Hitbox`, ...). The AI system runs
//! in `SimSet::Intent` and only decides velocity and state; `enemy_motion`
//! applies gravity and collision. A hit that knocks an enemy back staggers it:
//! its AI pauses until the push ends.
//!
//! Each attack is `Windup` (a readable tell) -> `Attack` -> `Recover` (the
//! punish window), with durations from `assets/tuning/enemies.ron`.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

pub mod ai;
pub mod spawn;

pub use spawn::spawn_enemy;

#[derive(Component)]
pub struct Enemy;

/// Ignores gravity.
#[derive(Component)]
pub struct Flying;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyKind {
    Husk,
    Wisp,
    Shieldbearer,
    Spitter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyState {
    /// Patrolling (or hovering/standing) until the player is noticed.
    Idle,
    /// Spotted the player: a short beat before acting ("!").
    Notice,
    /// Actively hunting.
    Chase,
    /// The tell before an attack. Never skipped, always readable.
    Windup,
    Attack,
    /// Helpless-ish gap after an attack: the punish window.
    Recover,
    /// Being pushed back by a hit.
    Stagger,
}

#[derive(Component, Clone, Debug)]
pub struct Brain {
    pub kind: EnemyKind,
    pub state: EnemyState,
    /// Ticks spent in the current state.
    pub timer: u32,
    /// +1 right, -1 left.
    pub facing: i8,
    pub home: Vec2,
    pub patrol_dir: i8,
    /// Attack direction locked in when the windup begins.
    pub aim: Vec2,
    /// Shieldbearer: how long the player has been behind it.
    pub turn_timer: u32,
    /// Free-running tick counter (hover bobbing).
    pub clock: u32,
}

impl Brain {
    pub fn new(kind: EnemyKind, home: Vec2) -> Self {
        Self {
            kind,
            state: EnemyState::Idle,
            timer: 0,
            facing: 1,
            home,
            patrol_dir: 1,
            aim: Vec2::X,
            turn_timer: 0,
            clock: 0,
        }
    }

    pub(crate) fn enter(&mut self, state: EnemyState) {
        self.state = state;
        self.timer = 0;
    }
}
