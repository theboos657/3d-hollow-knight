//! Components shared by every simulated entity.

use std::ops::{Deref, DerefMut};

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

/// Authoritative position in world units (1 unit = 1 tile), on the z = 0 lane.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct SimPos(pub Vec2);

/// Position at the start of the current tick. `hk_game` lerps `PrevPos` ->
/// `SimPos` by the fixed-step overshoot so visuals stay smooth at any refresh
/// rate without adding latency to the simulation.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PrevPos(pub Vec2);

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity(pub Vec2);

// `vel.x` / `vel.y` read better than `vel.0.x` in movement code.
impl Deref for Velocity {
    type Target = Vec2;
    fn deref(&self) -> &Vec2 {
        &self.0
    }
}

impl DerefMut for Velocity {
    fn deref_mut(&mut self) -> &mut Vec2 {
        &mut self.0
    }
}

/// Axis-aligned box, centred on `SimPos`, used for terrain collision.
#[derive(Component, Clone, Copy, Debug)]
pub struct Aabb {
    pub half: Vec2,
}

/// Copies `SimPos` into `PrevPos` at the top of every tick.
pub fn snapshot_prev(mut q: Query<(&SimPos, &mut PrevPos)>) {
    for (pos, mut prev) in &mut q {
        prev.0 = pos.0;
    }
}
