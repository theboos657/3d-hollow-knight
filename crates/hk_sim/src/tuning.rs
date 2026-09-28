//! Data-driven feel numbers. Authored in natural units (units, u/s, ms) in
//! `assets/tuning/*.ron`; converted to ticks here so changing the tick rate
//! never changes feel.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ms_to_ticks;

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub player: PlayerTuning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerTuning {
    /// Movement box half extents (world units).
    pub half_w: f32,
    pub half_h: f32,

    pub run_speed: f32,
    /// Time to reach full run speed from rest.
    pub ground_accel_ms: f32,
    /// Time to stop from full run speed.
    pub ground_decel_ms: f32,
    pub air_accel_ms: f32,
    pub air_decel_ms: f32,

    /// Full-jump apex height and time-to-apex; gravity and launch speed are
    /// derived: g = 2h/t^2, v0 = 2h/t.
    pub jump_height: f32,
    pub jump_time_ms: f32,
    pub fall_gravity_mult: f32,
    pub terminal_speed: f32,
    /// Releasing jump while rising multiplies vertical speed by this.
    pub jump_cut_mult: f32,
    /// Gravity multiplier near the apex while jump is held.
    pub hang_speed: f32,
    pub hang_gravity_mult: f32,

    pub coyote_ms: f32,
    pub jump_buffer_ms: f32,
    pub dash_buffer_ms: f32,

    pub dash_speed: f32,
    pub dash_ms: f32,
    pub dash_cooldown_ms: f32,
    /// Horizontal speed multiplier (of run speed) when jumping out of a dash.
    pub dash_jump_boost: f32,

    pub wall_slide_speed: f32,
    pub wall_jump_vx: f32,
    pub wall_jump_vy: f32,
    pub wall_lock_ms: f32,

    pub drop_through_ms: f32,
    /// Max horizontal nudge to slip past a ceiling corner (world units).
    pub corner_correction: f32,
}

impl Default for PlayerTuning {
    fn default() -> Self {
        Self {
            half_w: 0.4,
            half_h: 0.75,
            run_speed: 9.0,
            ground_accel_ms: 50.0,
            ground_decel_ms: 40.0,
            air_accel_ms: 80.0,
            air_decel_ms: 120.0,
            jump_height: 3.6,
            jump_time_ms: 360.0,
            fall_gravity_mult: 1.6,
            terminal_speed: 24.0,
            jump_cut_mult: 0.4,
            hang_speed: 2.0,
            hang_gravity_mult: 0.6,
            coyote_ms: 80.0,
            jump_buffer_ms: 100.0,
            dash_buffer_ms: 100.0,
            dash_speed: 24.0,
            dash_ms: 170.0,
            dash_cooldown_ms: 350.0,
            dash_jump_boost: 1.35,
            wall_slide_speed: 3.5,
            wall_jump_vx: 9.0,
            wall_jump_vy: 18.0,
            wall_lock_ms: 120.0,
            drop_through_ms: 150.0,
            corner_correction: 0.25,
        }
    }
}

impl PlayerTuning {
    /// Upward gravity (u/s^2), derived from jump height and time.
    pub fn gravity_up(&self) -> f32 {
        let t = self.jump_time_ms * 0.001;
        2.0 * self.jump_height / (t * t)
    }

    /// Launch speed (u/s), derived from jump height and time.
    pub fn jump_velocity(&self) -> f32 {
        2.0 * self.jump_height / (self.jump_time_ms * 0.001)
    }

    pub fn ground_accel(&self) -> f32 {
        self.run_speed / (self.ground_accel_ms * 0.001)
    }
    pub fn ground_decel(&self) -> f32 {
        self.run_speed / (self.ground_decel_ms * 0.001)
    }
    pub fn air_accel(&self) -> f32 {
        self.run_speed / (self.air_accel_ms * 0.001)
    }
    pub fn air_decel(&self) -> f32 {
        self.run_speed / (self.air_decel_ms * 0.001)
    }

    pub fn coyote_ticks(&self) -> u32 {
        ms_to_ticks(self.coyote_ms)
    }
    pub fn jump_buffer_ticks(&self) -> u32 {
        ms_to_ticks(self.jump_buffer_ms)
    }
    pub fn dash_buffer_ticks(&self) -> u32 {
        ms_to_ticks(self.dash_buffer_ms)
    }
    pub fn dash_ticks(&self) -> u32 {
        ms_to_ticks(self.dash_ms)
    }
    pub fn dash_cooldown_ticks(&self) -> u32 {
        ms_to_ticks(self.dash_cooldown_ms)
    }
    pub fn wall_lock_ticks(&self) -> u32 {
        ms_to_ticks(self.wall_lock_ms)
    }
    pub fn drop_through_ticks(&self) -> u32 {
        ms_to_ticks(self.drop_through_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_jump_physics_match_plan() {
        let p = PlayerTuning::default();
        assert!((p.gravity_up() - 55.555).abs() < 0.01);
        assert!((p.jump_velocity() - 20.0).abs() < 1e-4);
    }

    #[test]
    fn tick_conversions_match_plan() {
        let p = PlayerTuning::default();
        assert_eq!(p.coyote_ticks(), 10);
        assert_eq!(p.jump_buffer_ticks(), 12);
        assert_eq!(p.dash_ticks(), 20);
    }

    /// The shipped RON must parse and equal the in-code defaults, so the file
    /// and the code can never silently drift apart.
    #[test]
    fn shipped_player_ron_matches_defaults() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/tuning/player.ron");
        let text = std::fs::read_to_string(path).expect("assets/tuning/player.ron exists");
        let parsed: PlayerTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, PlayerTuning::default());
    }
}
