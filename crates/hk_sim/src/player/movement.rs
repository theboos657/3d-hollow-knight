//! Locomotion: run, jump (variable height, coyote time, buffering), dash,
//! wall slide / wall jump, drop-through. One system, run in `SimSet::Motion`.
//!
//! Vertical motion uses constant-acceleration integration
//! (`dy = (v_old + v_new) / 2 * dt`), which is exact for constant gravity, so
//! the jump apex matches `v0^2 / 2g` to within a fraction of a millimetre.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::{Aabb, Abilities, Facing, Motor, Player, PlayerState};
use crate::combat::CombatState;
use crate::components::{SimPos, Velocity};
use crate::input::{Action, InputState};
use crate::tuning::Tuning;
use crate::world::grid::{move_body, probe, Side, Tile, TileGrid};
use crate::{SimTick, DT};

fn approach(v: f32, target: f32, max_delta: f32) -> f32 {
    if v < target {
        (v + max_delta).min(target)
    } else {
        (v - max_delta).max(target)
    }
}

#[allow(clippy::type_complexity)]
pub fn player_movement(
    mut input: ResMut<InputState>,
    tick: Res<SimTick>,
    grid: Res<TileGrid>,
    tuning: Res<Tuning>,
    mut q: Query<
        (
            &mut SimPos,
            &mut Velocity,
            &Aabb,
            &mut Motor,
            &mut PlayerState,
            &mut Facing,
            &Abilities,
            &CombatState,
        ),
        With<Player>,
    >,
) {
    let p = &tuning.player;
    let now = tick.0;
    let jump_buf = p.jump_buffer_ticks();

    for (mut pos, mut vel, aabb, mut m, mut state, mut facing, abil, cs) in &mut q {
        // Dead: hold still until the respawn logic moves us.
        if cs.dead {
            vel.0 = Vec2::ZERO;
            *state = PlayerState::Dead;
            continue;
        }
        let can_act = cs.stun == 0;
        let half = aabb.half;
        let axis = input.axis_x();

        // ---- timers ----
        m.dash_cooldown = m.dash_cooldown.saturating_sub(1);
        m.wall_lock = m.wall_lock.saturating_sub(1);
        m.drop_through = m.drop_through.saturating_sub(1);

        // ---- drop through a one-way platform (Down + Jump) ----
        if can_act
            && m.grounded
            && input.axis_y() < 0
            && input.buffered(Action::Jump, now, jump_buf)
            && !grid.overlaps(pos.0 - Vec2::new(0.0, 0.01), half, Tile::Solid)
        {
            input.consume(Action::Jump, now, jump_buf);
            m.drop_through = p.drop_through_ticks();
            m.grounded = false;
            m.coyote = 0;
        }

        // ---- start a dash ----
        let dashing_now = m.dash_ticks_left > 0;
        if can_act
            && abil.dash
            && !dashing_now
            && m.dash_cooldown == 0
            && (m.grounded || m.air_dash_ready)
            && input.consume(Action::Dash, now, p.dash_buffer_ticks())
        {
            let dir = if axis != 0 { axis } else { facing.0 };
            m.dash_dir = dir;
            m.dash_ticks_left = p.dash_ticks();
            m.dash_cooldown = p.dash_cooldown_ticks();
            if !m.grounded {
                m.air_dash_ready = false;
            }
            facing.0 = dir;
            m.jumping = false;
        }

        // ---- jump ----
        let can_ground_jump = m.grounded || m.coyote > 0;
        if can_act && input.buffered(Action::Jump, now, jump_buf) {
            if can_ground_jump {
                input.consume(Action::Jump, now, jump_buf);
                vel.y = p.jump_velocity();
                m.jumping = true;
                m.grounded = false;
                m.coyote = 0;
                if m.dash_ticks_left > 0 {
                    // Dash-jump: keep the dash's momentum, boosted.
                    vel.x = m.dash_dir as f32 * p.run_speed * p.dash_jump_boost;
                    m.dash_ticks_left = 0;
                }
            } else if abil.wall_grip && m.wall != 0 && !m.grounded {
                input.consume(Action::Jump, now, jump_buf);
                vel.x = -(m.wall as f32) * p.wall_jump_vx;
                vel.y = p.wall_jump_vy;
                facing.0 = -m.wall;
                m.wall_lock = p.wall_lock_ticks();
                m.jumping = true;
                m.dash_ticks_left = 0;
            }
        }
        if !m.grounded {
            m.coyote = m.coyote.saturating_sub(1);
        }

        // ---- horizontal ----
        let dashing = m.dash_ticks_left > 0;
        if dashing {
            vel.x = m.dash_dir as f32 * p.dash_speed;
        } else if cs.focusing || (cs.cast_lock > 0 && m.grounded) {
            // Planted: focusing or casting on the ground.
            vel.x = approach(vel.x, 0.0, p.ground_decel() * DT);
        } else if cs.stun > 0 {
            // Hurt: keep the knockback, bleed it off slowly.
            vel.x = approach(vel.x, 0.0, p.ground_decel() * 0.25 * DT);
        } else if m.wall_lock == 0 && cs.control_lock == 0 {
            if axis != 0 {
                facing.0 = axis;
            }
            let target = axis as f32 * p.run_speed;
            let (accel, decel) = if m.grounded {
                (p.ground_accel(), p.ground_decel())
            } else {
                (p.air_accel(), p.air_decel())
            };
            let carrying = axis != 0 && vel.x.signum() == axis as f32 && vel.x.abs() > p.run_speed;
            if !carrying {
                let reversing = vel.x != 0.0 && vel.x.signum() != axis as f32;
                let rate = if axis == 0 || reversing { decel } else { accel };
                vel.x = approach(vel.x, target, rate * DT);
            }
        }

        // ---- vertical ----
        let disp_y;
        if dashing {
            vel.y = 0.0;
            disp_y = 0.0;
        } else {
            // Variable jump height: releasing while rising cuts the jump.
            if m.jumping && vel.y > 0.0 && !input.held(Action::Jump) {
                vel.y *= p.jump_cut_mult;
                m.jumping = false;
            }
            if vel.y <= 0.0 {
                m.jumping = false;
            }

            let mut g = p.gravity_up();
            if vel.y <= 0.0 {
                g *= p.fall_gravity_mult;
            }
            if !m.grounded && vel.y.abs() < p.hang_speed && input.held(Action::Jump) {
                g *= p.hang_gravity_mult;
            }

            let mut v_old = vel.y;
            let mut v_new = (v_old - g * DT).max(-p.terminal_speed);
            let sliding =
                abil.wall_grip && !m.grounded && m.wall != 0 && axis == m.wall && v_new < 0.0;
            if sliding {
                v_old = v_old.max(-p.wall_slide_speed);
                v_new = v_new.max(-p.wall_slide_speed);
            }
            disp_y = 0.5 * (v_old + v_new) * DT;
            vel.y = v_new;
        }

        // ---- move & collide ----
        m.dash_ticks_left = m.dash_ticks_left.saturating_sub(1);
        let disp = Vec2::new(vel.x * DT, disp_y);
        let drop = m.drop_through > 0;
        let mut out = move_body(&grid, pos.0, half, disp, drop);

        // Corner correction: slip past a ceiling corner instead of bonking.
        if out.bonked && disp.y > 0.0 && out.blocked_x == 0 && p.corner_correction > 0.0 {
            let steps = (p.corner_correction / 0.05).ceil() as i32;
            'search: for k in 1..=steps {
                for s in [1.0f32, -1.0] {
                    let cand = Vec2::new(pos.0.x + s * k as f32 * 0.05, pos.0.y);
                    if grid.overlaps(cand, half, Tile::Solid) {
                        continue;
                    }
                    let attempt = move_body(&grid, cand, half, disp, drop);
                    if !attempt.bonked {
                        out = attempt;
                        break 'search;
                    }
                }
            }
        }

        pos.0 = out.pos;
        if out.blocked_x != 0 {
            vel.x = 0.0;
        }
        if out.landed && vel.y <= 0.0 {
            vel.y = 0.0;
        }
        if out.bonked {
            vel.y = vel.y.min(0.0);
            m.jumping = false;
        }
        // Dash finished this tick: leave at run speed instead of stopping dead.
        // (Applied after the move so the dash keeps its full length.)
        if dashing && m.dash_ticks_left == 0 {
            vel.x = m.dash_dir as f32 * p.run_speed;
        }

        // ---- post-move probes (used by the next tick) ----
        m.grounded = vel.y <= 0.0 && probe(&grid, pos.0, half, Side::Below, drop);
        m.wall = if probe(&grid, pos.0, half, Side::Right, false) {
            1
        } else if probe(&grid, pos.0, half, Side::Left, false) {
            -1
        } else {
            0
        };
        if m.grounded {
            m.coyote = p.coyote_ticks();
            m.air_dash_ready = true;
            m.jumping = false;
        }

        *state = if cs.stun > 0 {
            PlayerState::Hurt
        } else if cs.focusing {
            PlayerState::Focus
        } else if m.dash_ticks_left > 0 {
            PlayerState::Dash
        } else if m.grounded {
            PlayerState::Grounded
        } else if abil.wall_grip && m.wall != 0 && axis == m.wall && vel.y < 0.0 {
            PlayerState::WallSlide
        } else {
            PlayerState::Airborne
        };
    }
}
