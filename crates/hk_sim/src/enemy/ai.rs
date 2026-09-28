//! Enemy state machines and body motion.
//!
//! Timing convention: `Brain::timer` counts ticks spent in the current state
//! (1 on the first tick). A state lasting N ticks moves on when `timer >= N`,
//! and that tick still counts as part of the state, so a 42-tick windup really
//! is 42 ticks long.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::{Brain, Enemy, EnemyKind, EnemyState, Flying};
use crate::combat::{
    AlreadyHit, CombatState, Guard, HitKind, Hitbox, Knockback, Lifetime, Projectile, Team,
};
use crate::components::{Aabb, PrevPos, SimPos, Velocity};
use crate::player::Player;
use crate::tuning::{EnemyTuning, HuskTuning, ShieldTuning, SpitterTuning, Tuning, WispTuning};
use crate::world::grid::{move_body, Tile, TileGrid};
use crate::DT;

/// Read-only view of the world for one enemy's decision.
struct Ctx<'a> {
    pos: Vec2,
    half: Vec2,
    target: Option<Vec2>,
    grid: &'a TileGrid,
}

/// A projectile an enemy wants to spawn this tick.
struct Shot {
    pos: Vec2,
    vel: Vec2,
    half: Vec2,
    life: u32,
    damage: i32,
}

fn sign(v: f32, fallback: i8) -> i8 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        fallback
    }
}

/// Wall in front, or no floor just past the feet (a ledge).
fn wall_or_ledge_ahead(c: &Ctx, dir: i8) -> bool {
    let d = dir as f32;
    let ahead = Vec2::new(c.pos.x + d * (c.half.x + 0.15), c.pos.y);
    let wall = c
        .grid
        .overlaps(ahead, Vec2::new(0.1, c.half.y * 0.8), Tile::Solid);
    let fx = c.pos.x + d * (c.half.x + 0.25);
    let fy = c.pos.y - c.half.y - 0.2;
    let floor = matches!(
        c.grid.get(fx.floor() as i32, fy.floor() as i32),
        Tile::Solid | Tile::OneWay
    );
    wall || !floor
}

/// Walk `dir` at `speed` unless a wall or ledge is in the way.
fn walk(c: &Ctx, vel: &mut Velocity, dir: i8, speed: f32) {
    vel.x = if wall_or_ledge_ahead(c, dir) {
        0.0
    } else {
        dir as f32 * speed
    };
}
pub fn enemy_ai(
    mut commands: Commands,
    tuning: Res<Tuning>,
    grid: Res<TileGrid>,
    players: Query<(&SimPos, &CombatState), With<Player>>,
    mut q: Query<
        (
            Entity,
            &mut Brain,
            &SimPos,
            &mut Velocity,
            &Aabb,
            Has<Knockback>,
            Option<&mut Guard>,
        ),
        With<Enemy>,
    >,
) {
    let t: &EnemyTuning = &tuning.enemies;
    let target = players.iter().find(|(_, cs)| !cs.dead).map(|(p, _)| p.0);

    for (entity, mut b, pos, mut vel, aabb, knocked, guard) in &mut q {
        // A hit that pushes the enemy back pauses its AI until the push ends.
        if knocked {
            if b.state != EnemyState::Stagger {
                b.enter(EnemyState::Stagger);
            }
            vel.0 = Vec2::ZERO;
            continue;
        }
        if b.state == EnemyState::Stagger {
            b.enter(EnemyState::Chase);
        }

        b.timer += 1;
        b.clock += 1;
        let c = Ctx {
            pos: pos.0,
            half: aabb.half,
            target,
            grid: &grid,
        };
        let mut shots: Vec<Shot> = Vec::new();
        match b.kind {
            EnemyKind::Husk => husk_step(&mut b, &mut vel, &c, &t.husk),
            EnemyKind::Wisp => wisp_step(&mut b, &mut vel, &c, &t.wisp),
            EnemyKind::Shieldbearer => shield_step(&mut b, &mut vel, &c, &t.shield),
            EnemyKind::Spitter => spitter_step(&mut b, &mut vel, &c, &t.spitter, &mut shots),
        }
        if let Some(mut g) = guard {
            g.facing = b.facing;
        }
        for s in shots {
            let p = s.pos;
            commands.spawn((
                Projectile,
                Hitbox {
                    half: s.half,
                    team: Team::Enemy,
                    damage: s.damage,
                    kind: HitKind::Projectile,
                    attack_dir: crate::combat::AttackDir::Forward,
                    once: false,
                    owner: entity,
                },
                AlreadyHit::default(),
                SimPos(p),
                PrevPos(p),
                Velocity(s.vel),
                Lifetime(s.life),
            ));
        }
    }
}

// ----------------------------------------------------------------- husk --

fn husk_step(b: &mut Brain, vel: &mut Velocity, c: &Ctx, t: &HuskTuning) {
    let d = c.target.map(|p| p - c.pos);
    let sees = d.is_some_and(|d| d.x.abs() < t.aggro_radius && d.y.abs() < t.aggro_dy);
    let lost =
        d.is_none_or(|d| d.x.abs() > t.aggro_radius * t.leash_mult || d.y.abs() > t.aggro_dy * 2.0);

    match b.state {
        EnemyState::Idle => {
            // Patrol within range of home, turning at ledges and walls.
            let off = c.pos.x - b.home.x;
            if off.abs() > t.patrol_range && off * b.patrol_dir as f32 > 0.0 {
                b.patrol_dir = -b.patrol_dir;
            }
            if wall_or_ledge_ahead(c, b.patrol_dir) {
                b.patrol_dir = -b.patrol_dir;
            }
            b.facing = b.patrol_dir;
            vel.x = b.patrol_dir as f32 * t.patrol_speed;
            if sees {
                b.facing = sign(d.unwrap().x, b.facing);
                vel.x = 0.0;
                b.enter(EnemyState::Notice);
            }
        }
        EnemyState::Notice => {
            vel.x = 0.0;
            if b.timer >= t.notice_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Chase => {
            let Some(d) = d.filter(|_| !lost) else {
                vel.x = 0.0;
                b.enter(EnemyState::Idle);
                return;
            };
            b.facing = sign(d.x, b.facing);
            if d.x.abs() <= t.attack_range && d.y.abs() < 1.5 {
                b.aim = Vec2::new(b.facing as f32, 0.0);
                vel.x = 0.0;
                b.enter(EnemyState::Windup);
                return;
            }
            walk(c, vel, b.facing, t.chase_speed);
        }
        EnemyState::Windup => {
            vel.x = 0.0;
            if b.timer >= t.windup_ticks() {
                b.enter(EnemyState::Attack);
            }
        }
        EnemyState::Attack => {
            vel.x = b.aim.x * t.lunge_speed;
            if wall_or_ledge_ahead(c, b.aim.x as i8) || b.timer >= t.lunge_ticks() {
                b.enter(EnemyState::Recover);
            }
        }
        EnemyState::Recover => {
            vel.x = 0.0;
            if b.timer >= t.recover_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Stagger => {}
    }
}

// ----------------------------------------------------------------- wisp --

fn wisp_step(b: &mut Brain, vel: &mut Velocity, c: &Ctx, t: &WispTuning) {
    let d = c.target.map(|p| p - c.pos);
    let sees = d.is_some_and(|d| d.length() < t.aggro_radius);
    let lost = d.is_none_or(|d| d.length() > t.aggro_radius * 1.6);

    // Gentle vertical bob around a base height.
    let phase = b.clock as f32 / t.bob_period_ticks() as f32 * std::f32::consts::TAU;
    let bob = t.bob_amp * phase.sin();

    match b.state {
        EnemyState::Idle => {
            vel.x = 0.0;
            vel.y = ((b.home.y + bob) - c.pos.y) * 3.0;
            if sees {
                b.enter(EnemyState::Notice);
            }
        }
        EnemyState::Notice => {
            vel.x = 0.0;
            vel.y = ((b.home.y + bob) - c.pos.y) * 3.0;
            if let Some(d) = d {
                b.facing = sign(d.x, b.facing);
            }
            if b.timer >= t.notice_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Chase => {
            let (Some(d), Some(target)) = (d.filter(|_| !lost), c.target) else {
                b.enter(EnemyState::Idle);
                return;
            };
            b.facing = sign(d.x, b.facing);
            let goal = Vec2::new(target.x, target.y + t.hover_height + bob);
            let to = goal - c.pos;
            vel.0 = to.normalize_or_zero() * t.chase_speed.min(to.length() * 4.0);
            // Directly above the player and high enough: commit to a dive.
            if d.x.abs() < t.dive_dx && c.pos.y > target.y + 1.0 {
                b.aim = (target - c.pos).normalize_or_zero();
                vel.0 = Vec2::ZERO;
                b.enter(EnemyState::Windup);
            }
        }
        EnemyState::Windup => {
            vel.0 = Vec2::ZERO;
            if b.timer >= t.windup_ticks() {
                b.enter(EnemyState::Attack);
            }
        }
        EnemyState::Attack => {
            vel.0 = b.aim * t.dive_speed;
            if b.timer >= t.dive_ticks() {
                b.enter(EnemyState::Recover);
            }
        }
        EnemyState::Recover => {
            // Climb back up to hovering height.
            let goal_y = c.target.map_or(b.home.y, |p| p.y + t.hover_height);
            vel.x = 0.0;
            vel.y = if c.pos.y < goal_y { t.chase_speed } else { 0.0 };
            if b.timer >= t.recover_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Stagger => {}
    }
}

// --------------------------------------------------------------- shield --

fn shield_step(b: &mut Brain, vel: &mut Velocity, c: &Ctx, t: &ShieldTuning) {
    let d = c.target.map(|p| p - c.pos);
    let sees = d.is_some_and(|d| d.x.abs() < t.aggro_radius && d.y.abs() < t.aggro_dy);
    let lost = d.is_none_or(|d| d.x.abs() > t.aggro_radius * 1.6 || d.y.abs() > t.aggro_dy * 2.0);

    match b.state {
        EnemyState::Idle => {
            let off = c.pos.x - b.home.x;
            if off.abs() > t.patrol_range && off * b.patrol_dir as f32 > 0.0 {
                b.patrol_dir = -b.patrol_dir;
            }
            if wall_or_ledge_ahead(c, b.patrol_dir) {
                b.patrol_dir = -b.patrol_dir;
            }
            b.facing = b.patrol_dir;
            vel.x = b.patrol_dir as f32 * t.patrol_speed;
            if sees {
                b.facing = sign(d.unwrap().x, b.facing);
                vel.x = 0.0;
                b.enter(EnemyState::Notice);
            }
        }
        EnemyState::Notice => {
            vel.x = 0.0;
            if b.timer >= t.notice_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Chase => {
            let Some(d) = d.filter(|_| !lost) else {
                vel.x = 0.0;
                b.enter(EnemyState::Idle);
                return;
            };
            let want = sign(d.x, b.facing);
            if want != b.facing {
                // The player is behind the shield: it stands still and turns
                // slowly. This is the window to hit it from behind.
                vel.x = 0.0;
                b.turn_timer += 1;
                if b.turn_timer >= t.turn_delay_ticks() {
                    b.facing = want;
                    b.turn_timer = 0;
                }
                return;
            }
            b.turn_timer = 0;
            if d.x.abs() <= t.bash_range && d.y.abs() < 1.5 {
                b.aim = Vec2::new(b.facing as f32, 0.0);
                vel.x = 0.0;
                b.enter(EnemyState::Windup);
                return;
            }
            walk(c, vel, b.facing, t.speed);
        }
        EnemyState::Windup => {
            vel.x = 0.0;
            if b.timer >= t.windup_ticks() {
                b.enter(EnemyState::Attack);
            }
        }
        EnemyState::Attack => {
            vel.x = b.aim.x * t.bash_speed;
            if wall_or_ledge_ahead(c, b.aim.x as i8) || b.timer >= t.bash_ticks() {
                b.enter(EnemyState::Recover);
            }
        }
        EnemyState::Recover => {
            vel.x = 0.0;
            if b.timer >= t.recover_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Stagger => {}
    }
}

// -------------------------------------------------------------- spitter --

fn spitter_step(
    b: &mut Brain,
    vel: &mut Velocity,
    c: &Ctx,
    t: &SpitterTuning,
    shots: &mut Vec<Shot>,
) {
    let d = c.target.map(|p| p - c.pos);
    let sees = d.is_some_and(|d| d.x.abs() < t.aggro_radius && d.y.abs() < t.aggro_dy);
    let lost = d.is_none_or(|d| d.x.abs() > t.aggro_radius * 1.6 || d.y.abs() > t.aggro_dy * 2.0);

    match b.state {
        EnemyState::Idle => {
            vel.x = 0.0;
            if sees {
                b.facing = sign(d.unwrap().x, b.facing);
                b.enter(EnemyState::Notice);
            }
        }
        EnemyState::Notice => {
            vel.x = 0.0;
            if b.timer >= t.notice_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Chase => {
            let (Some(d), Some(target)) = (d.filter(|_| !lost), c.target) else {
                vel.x = 0.0;
                b.enter(EnemyState::Idle);
                return;
            };
            b.facing = sign(d.x, b.facing);
            let dist = d.x.abs();
            if dist < t.min_range {
                // Too close: back away, or shoot anyway if cornered.
                let away = -b.facing;
                if wall_or_ledge_ahead(c, away) {
                    b.aim = (target - c.pos).normalize_or_zero();
                    vel.x = 0.0;
                    b.enter(EnemyState::Windup);
                } else {
                    vel.x = away as f32 * t.retreat_speed;
                }
            } else if dist > t.max_range {
                walk(c, vel, b.facing, t.advance_speed);
            } else {
                b.aim = (target - c.pos).normalize_or_zero();
                vel.x = 0.0;
                b.enter(EnemyState::Windup);
            }
        }
        EnemyState::Windup => {
            vel.x = 0.0;
            if let Some(d) = d {
                b.facing = sign(d.x, b.facing);
            }
            if b.timer >= t.windup_ticks() {
                shots.push(Shot {
                    pos: c.pos + Vec2::new(b.facing as f32 * 0.6, 0.2),
                    vel: b.aim * t.shot_speed,
                    half: Vec2::new(t.shot_half.0, t.shot_half.1),
                    life: t.shot_lifetime_ticks(),
                    damage: 1,
                });
                b.enter(EnemyState::Recover);
            }
        }
        EnemyState::Attack => {
            b.enter(EnemyState::Recover);
        }
        EnemyState::Recover => {
            vel.x = 0.0;
            if b.timer >= t.recover_ticks() {
                b.enter(EnemyState::Chase);
            }
        }
        EnemyState::Stagger => {}
    }
}

// ----------------------------------------------------------------- body --

/// Gravity (ground enemies) and collision for AI-driven motion.
pub fn enemy_motion(
    grid: Res<TileGrid>,
    tuning: Res<Tuning>,
    mut q: Query<(&mut SimPos, &mut Velocity, &Aabb, Has<Flying>), With<Enemy>>,
) {
    let t = &tuning.enemies;
    for (mut pos, mut vel, aabb, flying) in &mut q {
        if !flying {
            vel.y = (vel.y - t.gravity * DT).max(-t.terminal_speed);
        }
        let out = move_body(&grid, pos.0, aabb.half, vel.0 * DT, false);
        pos.0 = out.pos;
        if out.blocked_x != 0 {
            vel.x = 0.0;
        }
        if out.landed || out.bonked {
            vel.y = 0.0;
        }
    }
}
