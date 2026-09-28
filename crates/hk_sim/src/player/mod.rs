//! The player: components, spawning, and the locomotion controller.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use crate::components::{PrevPos, SimPos, Velocity};

pub mod movement;

pub use movement::player_movement;

#[derive(Component, Default)]
pub struct Player;

/// Axis-aligned movement box, centred on `SimPos`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Aabb {
    pub half: Vec2,
}

/// Locomotion state, derived every tick from [`Motor`] (drives animation/VFX).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Grounded,
    Airborne,
    WallSlide,
    Dash,
}

/// Which way the player faces: +1 right, -1 left.
#[derive(Component, Clone, Copy, Debug)]
pub struct Facing(pub i8);

/// Movement abilities unlocked so far (gates in the world check these).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Abilities {
    pub dash: bool,
    pub wall_grip: bool,
}

/// Per-player locomotion bookkeeping. Timers are in ticks.
#[derive(Component, Clone, Debug, Default)]
pub struct Motor {
    /// Touching ground after the last move.
    pub grounded: bool,
    /// Wall touched after the last move: +1 right, -1 left, 0 none.
    pub wall: i8,
    /// Ticks remaining in which a ground jump is still allowed after leaving
    /// the ground.
    pub coyote: u32,
    pub dash_ticks_left: u32,
    pub dash_cooldown: u32,
    pub dash_dir: i8,
    /// An air dash is available (refilled on the ground).
    pub air_dash_ready: bool,
    /// Rising from a jump the player can still cut short.
    pub jumping: bool,
    /// Horizontal input ignored while > 0 (after a wall jump).
    pub wall_lock: u32,
    /// One-way platforms ignored while > 0 (after dropping through).
    pub drop_through: u32,
}

#[derive(Bundle)]
pub struct PlayerBundle {
    pub player: Player,
    pub pos: SimPos,
    pub prev: PrevPos,
    pub vel: Velocity,
    pub aabb: Aabb,
    pub state: PlayerState,
    pub facing: Facing,
    pub abilities: Abilities,
    pub motor: Motor,
}

impl PlayerBundle {
    pub fn new(pos: Vec2, half: Vec2, abilities: Abilities) -> Self {
        Self {
            player: Player,
            pos: SimPos(pos),
            prev: PrevPos(pos),
            vel: Velocity::default(),
            aabb: Aabb { half },
            state: PlayerState::Airborne,
            facing: Facing(1),
            abilities,
            motor: Motor::default(),
        }
    }
}

/// Spawns a player with the default box from tuning.
pub fn spawn_player(world: &mut World, pos: Vec2, abilities: Abilities) -> Entity {
    let t = world.resource::<crate::tuning::Tuning>().player.clone();
    world
        .spawn(PlayerBundle::new(pos, Vec2::new(t.half_w, t.half_h), abilities))
        .id()
}
