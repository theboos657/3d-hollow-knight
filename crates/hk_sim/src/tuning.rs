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
    pub combat: CombatTuning,
    pub enemies: EnemyTuning,
    pub camera: CameraTuning,
    pub bosses: BossTuning,
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
    /// Brief invulnerability at the start of a dash: dash *through* attacks.
    pub dash_iframes_ms: f32,

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
            dash_iframes_ms: 120.0,
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
    pub fn dash_iframes_ticks(&self) -> u32 {
        ms_to_ticks(self.dash_iframes_ms)
    }
    pub fn wall_lock_ticks(&self) -> u32 {
        ms_to_ticks(self.wall_lock_ms)
    }
    pub fn drop_through_ticks(&self) -> u32 {
        ms_to_ticks(self.drop_through_ms)
    }
}

impl Tuning {
    /// Loads `player.ron`, `combat.ron`, `enemies.ron`, `camera.ron` and `bosses.ron` from
    /// `dir`. A missing or broken file falls back to the built-in default for
    /// that group and is reported in the returned warnings, so a typo while
    /// tuning never stops the game from starting.
    pub fn load_dir(dir: &std::path::Path) -> (Tuning, Vec<String>) {
        fn load<T: serde::de::DeserializeOwned + Default>(
            dir: &std::path::Path,
            file: &str,
            warnings: &mut Vec<String>,
        ) -> T {
            let path = dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(text) => match ron::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(format!("{}: {e} (using defaults)", path.display()));
                        T::default()
                    }
                },
                Err(e) => {
                    warnings.push(format!("{}: {e} (using defaults)", path.display()));
                    T::default()
                }
            }
        }
        let mut w = Vec::new();
        let t = Tuning {
            player: load(dir, "player.ron", &mut w),
            combat: load(dir, "combat.ron", &mut w),
            enemies: load(dir, "enemies.ron", &mut w),
            camera: load(dir, "camera.ron", &mut w),
            bosses: load(dir, "bosses.ron", &mut w),
        };
        (t, w)
    }
}

/// Nail, pogo, damage, soul and spell numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CombatTuning {
    // ---- nail ----
    pub nail_damage: i32,
    pub nail_startup_ms: f32,
    pub nail_active_ms: f32,
    /// Time from one swing's start to the next swing being allowed.
    pub nail_cooldown_ms: f32,
    pub attack_buffer_ms: f32,
    /// Slash box sizes as (half_w, half_h); it starts at the player's edge.
    pub nail_forward: (f32, f32),
    pub nail_vertical: (f32, f32),
    /// Vertical speed after a successful down-slash pogo.
    pub pogo_speed: f32,
    /// Horizontal push back on the player after a forward hit.
    pub nail_recoil_speed: f32,
    pub nail_recoil_ms: f32,

    // ---- feedback ----
    pub hitstop_nail_ms: f32,
    pub hitstop_hurt_ms: f32,
    pub hitstop_block_ms: f32,
    /// Push back on the player when a swing is blocked by a shield.
    pub block_recoil_speed: f32,

    // ---- player health ----
    pub max_masks: i32,
    pub iframes_ms: f32,
    pub stun_ms: f32,
    pub hurt_knock_vx: f32,
    pub hurt_knock_vy: f32,
    pub respawn_delay_ms: f32,
    /// Player hurtbox half extents (a little smaller than the movement box).
    pub hurtbox: (f32, f32),
    /// Being grounded this long makes the spot eligible as safe ground.
    pub safe_ground_ms: f32,

    // ---- soul ----
    pub soul_max: i32,
    pub soul_per_hit: i32,
    pub focus_cost: i32,
    pub focus_ms: f32,
    pub focus_heal: i32,

    // ---- spell (Ember Bolt) ----
    pub spell_cost: i32,
    pub spell_damage: i32,
    pub spell_speed: f32,
    pub spell_lifetime_ms: f32,
    pub spell_lock_ms: f32,
    pub spell_cooldown_ms: f32,
    pub spell_half: (f32, f32),

    // ---- enemies being hit ----
    pub enemy_knock_speed: f32,
    pub enemy_knock_ms: f32,
}

impl Default for CombatTuning {
    fn default() -> Self {
        Self {
            nail_damage: 5,
            nail_startup_ms: 30.0,
            nail_active_ms: 90.0,
            nail_cooldown_ms: 350.0,
            attack_buffer_ms: 100.0,
            nail_forward: (1.1, 0.7),
            nail_vertical: (0.8, 1.1),
            pogo_speed: 16.0,
            nail_recoil_speed: 4.0,
            nail_recoil_ms: 80.0,
            hitstop_nail_ms: 50.0,
            hitstop_hurt_ms: 120.0,
            hitstop_block_ms: 40.0,
            block_recoil_speed: 6.0,
            max_masks: 5,
            iframes_ms: 1300.0,
            stun_ms: 200.0,
            hurt_knock_vx: 12.0,
            hurt_knock_vy: 8.0,
            respawn_delay_ms: 800.0,
            hurtbox: (0.3, 0.65),
            safe_ground_ms: 100.0,
            soul_max: 99,
            soul_per_hit: 11,
            focus_cost: 33,
            focus_ms: 1000.0,
            focus_heal: 1,
            spell_cost: 33,
            spell_damage: 15,
            spell_speed: 16.0,
            spell_lifetime_ms: 2000.0,
            spell_lock_ms: 150.0,
            spell_cooldown_ms: 300.0,
            spell_half: (0.5, 0.35),
            enemy_knock_speed: 9.0,
            enemy_knock_ms: 150.0,
        }
    }
}

impl CombatTuning {
    pub fn nail_startup_ticks(&self) -> u32 {
        ms_to_ticks(self.nail_startup_ms)
    }
    pub fn nail_active_ticks(&self) -> u32 {
        ms_to_ticks(self.nail_active_ms)
    }
    pub fn nail_cooldown_ticks(&self) -> u32 {
        ms_to_ticks(self.nail_cooldown_ms)
    }
    pub fn attack_buffer_ticks(&self) -> u32 {
        ms_to_ticks(self.attack_buffer_ms)
    }
    pub fn nail_recoil_ticks(&self) -> u32 {
        ms_to_ticks(self.nail_recoil_ms)
    }
    pub fn hitstop_nail_ticks(&self) -> u32 {
        ms_to_ticks(self.hitstop_nail_ms)
    }
    pub fn hitstop_hurt_ticks(&self) -> u32 {
        ms_to_ticks(self.hitstop_hurt_ms)
    }
    pub fn hitstop_block_ticks(&self) -> u32 {
        ms_to_ticks(self.hitstop_block_ms)
    }
    pub fn iframes_ticks(&self) -> u32 {
        ms_to_ticks(self.iframes_ms)
    }
    pub fn stun_ticks(&self) -> u32 {
        ms_to_ticks(self.stun_ms)
    }
    pub fn respawn_delay_ticks(&self) -> u32 {
        ms_to_ticks(self.respawn_delay_ms)
    }
    pub fn safe_ground_ticks(&self) -> u32 {
        ms_to_ticks(self.safe_ground_ms)
    }
    pub fn focus_ticks(&self) -> u32 {
        ms_to_ticks(self.focus_ms)
    }
    pub fn spell_lifetime_ticks(&self) -> u32 {
        ms_to_ticks(self.spell_lifetime_ms)
    }
    pub fn spell_lock_ticks(&self) -> u32 {
        ms_to_ticks(self.spell_lock_ms)
    }
    pub fn spell_cooldown_ticks(&self) -> u32 {
        ms_to_ticks(self.spell_cooldown_ms)
    }
    pub fn enemy_knock_ticks(&self) -> u32 {
        ms_to_ticks(self.enemy_knock_ms)
    }
}

/// Shared enemy physics plus the four enemy types' numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnemyTuning {
    pub gravity: f32,
    pub terminal_speed: f32,
    pub contact_damage: i32,
    pub husk: HuskTuning,
    pub wisp: WispTuning,
    pub shield: ShieldTuning,
    pub spitter: SpitterTuning,
}

impl Default for EnemyTuning {
    fn default() -> Self {
        Self {
            gravity: 60.0,
            terminal_speed: 24.0,
            contact_damage: 1,
            husk: HuskTuning::default(),
            wisp: WispTuning::default(),
            shield: ShieldTuning::default(),
            spitter: SpitterTuning::default(),
        }
    }
}

/// Gutter Husk: ground tracker. Patrol -> notice -> chase -> windup -> lunge -> recover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HuskTuning {
    pub hp: i32,
    pub half: (f32, f32),
    pub poise: f32,
    pub patrol_range: f32,
    pub patrol_speed: f32,
    pub aggro_radius: f32,
    /// Max vertical distance at which the player is noticed.
    pub aggro_dy: f32,
    /// Chase is abandoned beyond aggro_radius * leash_mult.
    pub leash_mult: f32,
    pub notice_ms: f32,
    pub chase_speed: f32,
    pub attack_range: f32,
    pub windup_ms: f32,
    pub lunge_speed: f32,
    pub lunge_ms: f32,
    pub recover_ms: f32,
}

impl Default for HuskTuning {
    fn default() -> Self {
        Self {
            hp: 15,
            half: (0.5, 0.6),
            poise: 1.0,
            patrol_range: 4.0,
            patrol_speed: 2.0,
            aggro_radius: 9.0,
            aggro_dy: 3.5,
            leash_mult: 1.6,
            notice_ms: 300.0,
            chase_speed: 4.5,
            attack_range: 2.2,
            windup_ms: 350.0,
            lunge_speed: 12.0,
            lunge_ms: 250.0,
            recover_ms: 500.0,
        }
    }
}

impl HuskTuning {
    pub fn notice_ticks(&self) -> u32 {
        ms_to_ticks(self.notice_ms)
    }
    pub fn windup_ticks(&self) -> u32 {
        ms_to_ticks(self.windup_ms)
    }
    pub fn lunge_ticks(&self) -> u32 {
        ms_to_ticks(self.lunge_ms)
    }
    pub fn recover_ticks(&self) -> u32 {
        ms_to_ticks(self.recover_ms)
    }
}

/// Wisp: flying tracker. Hover -> notice -> chase above -> windup -> dive -> recover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WispTuning {
    pub hp: i32,
    pub half: (f32, f32),
    pub poise: f32,
    pub aggro_radius: f32,
    pub notice_ms: f32,
    pub chase_speed: f32,
    /// Height it tries to hold above the player while chasing.
    pub hover_height: f32,
    /// Horizontal distance under which it commits to a dive.
    pub dive_dx: f32,
    pub windup_ms: f32,
    pub dive_speed: f32,
    pub dive_ms: f32,
    pub recover_ms: f32,
    pub bob_amp: f32,
    pub bob_period_ms: f32,
}

impl Default for WispTuning {
    fn default() -> Self {
        Self {
            hp: 10,
            half: (0.45, 0.45),
            poise: 0.7,
            aggro_radius: 10.0,
            notice_ms: 300.0,
            chase_speed: 3.5,
            hover_height: 2.5,
            dive_dx: 1.2,
            windup_ms: 400.0,
            dive_speed: 11.0,
            dive_ms: 400.0,
            recover_ms: 600.0,
            bob_amp: 0.4,
            bob_period_ms: 2000.0,
        }
    }
}

impl WispTuning {
    pub fn notice_ticks(&self) -> u32 {
        ms_to_ticks(self.notice_ms)
    }
    pub fn windup_ticks(&self) -> u32 {
        ms_to_ticks(self.windup_ms)
    }
    pub fn dive_ticks(&self) -> u32 {
        ms_to_ticks(self.dive_ms)
    }
    pub fn recover_ticks(&self) -> u32 {
        ms_to_ticks(self.recover_ms)
    }
    pub fn bob_period_ticks(&self) -> u32 {
        ms_to_ticks(self.bob_period_ms).max(1)
    }
}

/// Shieldbearer: slow, guarded from the front (nail and bolt), weak to
/// pogo and to attacks from behind. Turns slowly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShieldTuning {
    pub hp: i32,
    pub half: (f32, f32),
    pub poise: f32,
    pub patrol_range: f32,
    pub patrol_speed: f32,
    pub aggro_radius: f32,
    pub aggro_dy: f32,
    pub notice_ms: f32,
    pub speed: f32,
    /// Time the player must stay behind it before it turns around.
    pub turn_delay_ms: f32,
    pub bash_range: f32,
    pub windup_ms: f32,
    pub bash_speed: f32,
    pub bash_ms: f32,
    pub recover_ms: f32,
}

impl Default for ShieldTuning {
    fn default() -> Self {
        Self {
            hp: 25,
            half: (0.55, 0.75),
            poise: 0.5,
            patrol_range: 3.0,
            patrol_speed: 1.5,
            aggro_radius: 9.0,
            aggro_dy: 3.5,
            notice_ms: 400.0,
            speed: 2.5,
            turn_delay_ms: 600.0,
            bash_range: 2.4,
            windup_ms: 500.0,
            bash_speed: 8.0,
            bash_ms: 300.0,
            recover_ms: 800.0,
        }
    }
}

impl ShieldTuning {
    pub fn notice_ticks(&self) -> u32 {
        ms_to_ticks(self.notice_ms)
    }
    pub fn turn_delay_ticks(&self) -> u32 {
        ms_to_ticks(self.turn_delay_ms)
    }
    pub fn windup_ticks(&self) -> u32 {
        ms_to_ticks(self.windup_ms)
    }
    pub fn bash_ticks(&self) -> u32 {
        ms_to_ticks(self.bash_ms)
    }
    pub fn recover_ticks(&self) -> u32 {
        ms_to_ticks(self.recover_ms)
    }
}

/// Spitter: keeps its distance and lobs a slow blob after a clear tell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpitterTuning {
    pub hp: i32,
    pub half: (f32, f32),
    pub poise: f32,
    pub aggro_radius: f32,
    pub aggro_dy: f32,
    /// Retreats when the player is closer than this.
    pub min_range: f32,
    /// Advances when the player is farther than this.
    pub max_range: f32,
    pub retreat_speed: f32,
    pub advance_speed: f32,
    pub notice_ms: f32,
    pub windup_ms: f32,
    pub recover_ms: f32,
    pub shot_speed: f32,
    pub shot_lifetime_ms: f32,
    pub shot_half: (f32, f32),
}

impl Default for SpitterTuning {
    fn default() -> Self {
        Self {
            hp: 15,
            half: (0.5, 0.6),
            poise: 1.0,
            aggro_radius: 12.0,
            aggro_dy: 3.5,
            min_range: 5.0,
            max_range: 11.0,
            retreat_speed: 3.5,
            advance_speed: 2.0,
            notice_ms: 300.0,
            windup_ms: 500.0,
            recover_ms: 1200.0,
            shot_speed: 9.0,
            shot_lifetime_ms: 2000.0,
            shot_half: (0.25, 0.25),
        }
    }
}

impl SpitterTuning {
    pub fn notice_ticks(&self) -> u32 {
        ms_to_ticks(self.notice_ms)
    }
    pub fn windup_ticks(&self) -> u32 {
        ms_to_ticks(self.windup_ms)
    }
    pub fn recover_ticks(&self) -> u32 {
        ms_to_ticks(self.recover_ms)
    }
    pub fn shot_lifetime_ticks(&self) -> u32 {
        ms_to_ticks(self.shot_lifetime_ms)
    }
}

/// Camera rig feel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraTuning {
    pub fov_deg: f32,
    /// Distance from the z = 0 gameplay plane.
    pub distance: f32,
    /// Smoothing times (larger = lazier).
    pub follow_x_ms: f32,
    pub follow_y_ms: f32,
    /// While airborne the camera ignores vertical movement inside +-this.
    pub deadzone_half_y: f32,
    /// Horizontal lead in the movement direction at full run speed.
    pub lookahead: f32,
    /// Moving the opposite way this long flips the lookahead.
    pub lookahead_flip_ms: f32,
    /// Holding up/down this long pans the camera.
    pub look_hold_ms: f32,
    pub look_dist: f32,
    pub look_ease_ms: f32,
    /// Falling faster than this starts pulling the view down.
    pub fall_look_start: f32,
    pub fall_look_max: f32,
    pub shake_max: f32,
    /// Trauma lost per second.
    pub trauma_decay: f32,
    /// Time to blend to a new room's bounds.
    pub bounds_blend_ms: f32,
}

impl Default for CameraTuning {
    fn default() -> Self {
        Self {
            fov_deg: 38.0,
            distance: 19.5,
            follow_x_ms: 120.0,
            follow_y_ms: 250.0,
            deadzone_half_y: 1.25,
            lookahead: 3.5,
            lookahead_flip_ms: 250.0,
            look_hold_ms: 500.0,
            look_dist: 4.0,
            look_ease_ms: 300.0,
            fall_look_start: 8.0,
            fall_look_max: 3.5,
            shake_max: 0.35,
            trauma_decay: 1.5,
            bounds_blend_ms: 400.0,
        }
    }
}

impl CameraTuning {
    /// Half the visible height at the gameplay plane.
    pub fn half_view_height(&self) -> f32 {
        self.distance * (self.fov_deg.to_radians() * 0.5).tan()
    }
}

// ------------------------------------------------------------------ bosses --

/// What an attack *does*. The shared boss state machine handles the timing
/// (telegraph -> active -> recover); the kind decides the behaviour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttackKind {
    /// Leap at the player, slam down, and send a shockwave along the floor each way.
    Slam {
        leap_vy: f32,
        /// Fastest horizontal speed of the leap (limits how far it can reach).
        max_leap_speed: f32,
        shock_speed: f32,
        shock_ms: f32,
    },
    /// Rush across the arena. Hitting a wall staggers the boss (longer recovery).
    Charge { speed: f32, wall_recover_mult: f32 },
    /// Warning glyphs appear on the floor, then bells drop on them.
    Bells {
        count: u32,
        warn_ms: f32,
        spread: f32,
    },
    /// Two melee arcs in quick succession (the second has its own short tell).
    Sweep {
        reach: (f32, f32),
        second_gap_ms: f32,
    },
    /// Swinging bells hang from the ceiling for a while: hazards you can pogo off.
    Pendulums {
        count: u32,
        amp: f32,
        period_ms: f32,
        life_ms: f32,
        /// Height of the bells' lowest point above the floor.
        hang_height: f32,
    },
    /// Repeated shockwaves outward from the boss: jump each one.
    Toll {
        waves: u32,
        interval_ms: f32,
        speed: f32,
    },
}

fn one_u8() -> u8 {
    1
}
fn max_u8() -> u8 {
    99
}
fn one_f32() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttackDef {
    pub name: String,
    pub kind: AttackKind,
    /// The tell: the boss holds still and flashes. Never skipped.
    pub telegraph_ms: f32,
    pub active_ms: f32,
    /// The punish window after the attack.
    pub recover_ms: f32,
    /// Horizontal distance to the player at which this attack may be chosen.
    pub min_range: f32,
    pub max_range: f32,
    #[serde(default = "one_f32")]
    pub weight: f32,
    #[serde(default = "one_u8")]
    pub min_phase: u8,
    #[serde(default = "max_u8")]
    pub max_phase: u8,
    pub damage: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BossDef {
    pub id: String,
    pub name: String,
    pub hp: i32,
    pub half: (f32, f32),
    pub walk_speed: f32,
    /// Longest the boss walks toward the player before attacking anyway.
    pub approach_ms: f32,
    /// Waking roar at the start of the fight (invulnerable).
    pub intro_ms: f32,
    /// Invulnerable roar between phases.
    pub transition_ms: f32,
    /// Health fractions below which phases 2, 3, ... begin.
    pub phase_thresholds: Vec<f32>,
    /// Multiplier on recovery time per phase (later phases are snappier).
    pub recover_mult: Vec<f32>,
    pub contact_damage: i32,
    pub death_ms: f32,
    pub attacks: Vec<AttackDef>,
}

impl BossDef {
    pub fn phases(&self) -> u8 {
        self.phase_thresholds.len() as u8 + 1
    }

    /// Recovery multiplier for `phase` (1-based).
    pub fn recover_mult_for(&self, phase: u8) -> f32 {
        self.recover_mult
            .get(phase as usize - 1)
            .copied()
            .unwrap_or(1.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BossTuning {
    pub bosses: Vec<BossDef>,
}

impl BossTuning {
    pub fn get(&self, id: &str) -> Option<&BossDef> {
        self.bosses.iter().find(|b| b.id == id)
    }
}

fn attack(
    name: &str,
    kind: AttackKind,
    telegraph_ms: f32,
    active_ms: f32,
    recover_ms: f32,
    range: (f32, f32),
    weight: f32,
    phases: (u8, u8),
) -> AttackDef {
    AttackDef {
        name: name.into(),
        kind,
        telegraph_ms,
        active_ms,
        recover_ms,
        min_range: range.0,
        max_range: range.1,
        weight,
        min_phase: phases.0,
        max_phase: phases.1,
        damage: 1,
    }
}

impl Default for BossTuning {
    fn default() -> Self {
        // (telegraph ms, recover ms, weight, max leap speed, shockwave speed)
        let slam = |tele, rec, w, leap, shock| {
            attack(
                "Toll Slam",
                AttackKind::Slam {
                    leap_vy: 20.0,
                    max_leap_speed: leap,
                    shock_speed: shock,
                    shock_ms: 2600.0,
                },
                tele,
                900.0,
                rec,
                (3.0, 14.0),
                w,
                (1, 99),
            )
        };
        let charge = |speed, tele, rec, w| {
            attack(
                "Warden's Charge",
                AttackKind::Charge {
                    speed,
                    wall_recover_mult: 1.6,
                },
                tele,
                1100.0,
                rec,
                (0.0, 40.0),
                w,
                (1, 99),
            )
        };
        Self {
            bosses: vec![
                // The mid-boss: two attacks, one phase. Defeating it grants Dash.
                BossDef {
                    id: "matron".into(),
                    name: "Gutter Matron".into(),
                    hp: 300,
                    half: (1.1, 1.0),
                    walk_speed: 3.2,
                    approach_ms: 1600.0,
                    intro_ms: 1200.0,
                    transition_ms: 1000.0,
                    phase_thresholds: vec![],
                    recover_mult: vec![1.0],
                    contact_damage: 1,
                    death_ms: 1800.0,
                    attacks: vec![
                        slam(600.0, 800.0, 1.0, 14.0, 10.0),
                        charge(16.0, 500.0, 900.0, 1.0),
                    ],
                },
                // The final boss: three phases.
                BossDef {
                    id: "bellwarden".into(),
                    name: "The Bellwarden".into(),
                    hp: 350,
                    half: (1.5, 1.4),
                    walk_speed: 3.0,
                    approach_ms: 1500.0,
                    intro_ms: 2000.0,
                    transition_ms: 1500.0,
                    phase_thresholds: vec![0.65, 0.30],
                    recover_mult: vec![1.0, 1.0, 0.9],
                    contact_damage: 1,
                    death_ms: 2500.0,
                    attacks: vec![
                        slam(650.0, 900.0, 2.0, 11.0, 9.0),
                        charge(20.0, 550.0, 800.0, 2.0),
                        attack(
                            "Falling Bells",
                            AttackKind::Bells {
                                count: 3,
                                warn_ms: 800.0,
                                spread: 3.6,
                            },
                            800.0,
                            1200.0,
                            700.0,
                            (0.0, 40.0),
                            2.0,
                            (1, 1),
                        ),
                        attack(
                            "Falling Bells II",
                            AttackKind::Bells {
                                count: 5,
                                warn_ms: 800.0,
                                spread: 3.6,
                            },
                            800.0,
                            1200.0,
                            700.0,
                            (0.0, 40.0),
                            2.0,
                            (2, 99),
                        ),
                        attack(
                            "Chain Sweep",
                            AttackKind::Sweep {
                                reach: (3.2, 1.8),
                                second_gap_ms: 400.0,
                            },
                            450.0,
                            900.0,
                            600.0,
                            (0.0, 4.5),
                            3.0,
                            (2, 99),
                        ),
                        attack(
                            "Pendulum Bells",
                            AttackKind::Pendulums {
                                count: 3,
                                amp: 3.5,
                                period_ms: 2600.0,
                                life_ms: 9000.0,
                                hang_height: 3.4,
                            },
                            700.0,
                            300.0,
                            600.0,
                            (0.0, 40.0),
                            1.0,
                            (2, 99),
                        ),
                        attack(
                            "Final Toll",
                            AttackKind::Toll {
                                waves: 3,
                                interval_ms: 600.0,
                                speed: 9.0,
                            },
                            800.0,
                            1800.0,
                            1300.0,
                            (0.0, 40.0),
                            2.0,
                            (3, 99),
                        ),
                    ],
                },
            ],
        }
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

    #[test]
    fn load_dir_reads_the_shipped_files_without_warnings() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tuning");
        let (t, warnings) = Tuning::load_dir(&dir);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(t, Tuning::default());
    }

    #[test]
    fn a_missing_or_broken_file_falls_back_and_warns() {
        let dir = std::env::temp_dir().join(format!("hk_tuning_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("player.ron"), "(run_speed: 12.0)").unwrap(); // partial is fine
        std::fs::write(dir.join("combat.ron"), "(nail_damage: oops)").unwrap(); // broken
                                                                                // enemies.ron and camera.ron are missing
        let (t, warnings) = Tuning::load_dir(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            t.player.run_speed, 12.0,
            "partial files keep other fields at default"
        );
        assert_eq!(t.player.jump_height, PlayerTuning::default().jump_height);
        assert_eq!(t.combat, CombatTuning::default(), "broken file -> defaults");
        assert_eq!(warnings.len(), 4, "{warnings:?}"); // combat broken; enemies, camera, bosses missing
    }

    #[test]
    fn combat_tick_conversions_match_plan() {
        let c = CombatTuning::default();
        assert_eq!(c.nail_startup_ticks(), 4);
        assert_eq!(c.nail_active_ticks(), 11);
        assert_eq!(c.nail_cooldown_ticks(), 42);
        assert_eq!(c.iframes_ticks(), 156);
        assert_eq!(c.focus_ticks(), 120);
    }

    #[test]
    fn shipped_bosses_ron_matches_defaults() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/tuning/bosses.ron"
        );
        let text = std::fs::read_to_string(path).expect(
            "assets/tuning/bosses.ron exists (regenerate: cargo run -p hk_tools --bin dump_tuning -- assets/tuning --force)",
        );
        let parsed: BossTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, BossTuning::default());
    }

    #[test]
    fn shipped_camera_ron_matches_defaults() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/tuning/camera.ron"
        );
        let text = std::fs::read_to_string(path).expect("assets/tuning/camera.ron exists");
        let parsed: CameraTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, CameraTuning::default());
    }

    #[test]
    fn camera_view_is_about_13_and_a_half_units_tall() {
        // Close enough that the characters read at a glance (they were ~68 px
        // tall at 720p when 16 units were visible), far enough to see a jump.
        let h = CameraTuning::default().half_view_height() * 2.0;
        assert!((h - 13.4).abs() < 0.2, "visible height {h}");
    }

    #[test]
    fn shipped_enemies_ron_matches_defaults() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/tuning/enemies.ron"
        );
        let text = std::fs::read_to_string(path).expect("assets/tuning/enemies.ron exists");
        let parsed: EnemyTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, EnemyTuning::default());
    }

    #[test]
    fn shipped_combat_ron_matches_defaults() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/tuning/combat.ron"
        );
        let text = std::fs::read_to_string(path).expect("assets/tuning/combat.ron exists");
        let parsed: CombatTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, CombatTuning::default());
    }

    /// The shipped RON must parse and equal the in-code defaults, so the file
    /// and the code can never silently drift apart.
    #[test]
    fn shipped_player_ron_matches_defaults() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/tuning/player.ron"
        );
        let text = std::fs::read_to_string(path).expect("assets/tuning/player.ron exists");
        let parsed: PlayerTuning = ron::from_str(&text).expect("valid RON");
        assert_eq!(parsed, PlayerTuning::default());
    }
}
