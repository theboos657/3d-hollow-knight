//! Player-driven combat actions (nail swings, Ember Bolt, focus) plus the
//! movement of things those actions create (slash boxes, projectiles, knockback).

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::*;
use crate::components::{Aabb, PrevPos, SimPos, Velocity};
use crate::input::{Action, InputState};
use crate::player::{Facing, Motor, Player};
use crate::tuning::{CombatTuning, Tuning};
use crate::world::grid::{move_body, Tile, TileGrid};
use crate::{SimTick, DT};

/// Slash box for `dir`: (half extents, offset from the player's centre).
pub fn nail_geometry(
    c: &CombatTuning,
    player_half: Vec2,
    dir: AttackDir,
    facing: i8,
) -> (Vec2, Vec2) {
    match dir {
        AttackDir::Forward => (
            Vec2::new(c.nail_forward.0, c.nail_forward.1),
            Vec2::new(facing as f32 * (player_half.x + c.nail_forward.0), 0.1),
        ),
        AttackDir::Up => (
            Vec2::new(c.nail_vertical.0, c.nail_vertical.1),
            Vec2::new(0.0, player_half.y + c.nail_vertical.1),
        ),
        AttackDir::Down => (
            Vec2::new(c.nail_vertical.0, c.nail_vertical.1),
            Vec2::new(0.0, -(player_half.y + c.nail_vertical.1)),
        ),
    }
}

#[allow(clippy::type_complexity)]
pub fn player_combat(
    mut commands: Commands,
    mut input: ResMut<InputState>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut q: Query<
        (
            Entity,
            &SimPos,
            &Aabb,
            &Facing,
            &Motor,
            &mut Health,
            &mut Soul,
            &mut CombatState,
        ),
        With<Player>,
    >,
) {
    let c = &tuning.combat;
    let now = tick.0;

    for (entity, pos, aabb, facing, motor, mut health, mut soul, mut cs) in &mut q {
        cs.attack_cooldown = cs.attack_cooldown.saturating_sub(1);
        cs.cast_cooldown = cs.cast_cooldown.saturating_sub(1);
        if cs.dead {
            continue;
        }
        let can_act = cs.stun == 0;
        let axis_x = input.axis_x();
        let axis_y = input.axis_y();

        // ---- advance the current swing ----
        if let Some(att) = cs.attack.as_mut() {
            att.age += 1;
            if att.age == c.nail_startup_ticks() && att.hitbox.is_none() {
                let (half, rel) = nail_geometry(c, aabb.half, att.dir, att.facing);
                let p = pos.0 + rel;
                let id = commands
                    .spawn((
                        Hitbox {
                            half,
                            team: Team::Player,
                            damage: c.nail_damage,
                            kind: HitKind::Nail,
                            attack_dir: att.dir,
                            once: true,
                            owner: entity,
                        },
                        AlreadyHit::default(),
                        HitboxFollow { owner: entity, rel },
                        SimPos(p),
                        PrevPos(p),
                    ))
                    .id();
                att.hitbox = Some(id);
            }
            if att.age >= c.nail_startup_ticks() + c.nail_active_ticks() {
                if let Some(hb) = att.hitbox {
                    commands.entity(hb).despawn();
                }
                cs.attack = None;
            }
        }

        // ---- start a swing ----
        if can_act
            && cs.attack.is_none()
            && cs.attack_cooldown == 0
            && cs.cast_lock == 0
            && input.consume(Action::Attack, now, c.attack_buffer_ticks())
        {
            let dir = if axis_y > 0 {
                AttackDir::Up
            } else if axis_y < 0 && !motor.grounded {
                AttackDir::Down
            } else {
                AttackDir::Forward
            };
            let f = if axis_x != 0 { axis_x } else { facing.0 };
            cs.attack = Some(Attack {
                dir,
                facing: f,
                age: 0,
                hitbox: None,
            });
            cs.attack_cooldown = c.nail_cooldown_ticks();
        }

        // ---- cast Ember Bolt ----
        if can_act
            && cs.attack.is_none()
            && cs.cast_cooldown == 0
            && soul.value >= c.spell_cost
            && input.consume(Action::Cast, now, c.attack_buffer_ticks())
        {
            let f = if axis_x != 0 { axis_x } else { facing.0 };
            soul.value -= c.spell_cost;
            cs.cast_lock = c.spell_lock_ticks();
            cs.cast_cooldown = c.spell_cooldown_ticks();
            let p = pos.0 + Vec2::new(f as f32 * 0.9, 0.2);
            commands.spawn((
                Projectile,
                Hitbox {
                    half: Vec2::new(c.spell_half.0, c.spell_half.1),
                    team: Team::Player,
                    damage: c.spell_damage,
                    kind: HitKind::Spell,
                    attack_dir: AttackDir::Forward,
                    once: true,
                    owner: entity,
                },
                AlreadyHit::default(),
                SimPos(p),
                PrevPos(p),
                Velocity(Vec2::new(f as f32 * c.spell_speed, 0.0)),
                Lifetime(c.spell_lifetime_ticks()),
            ));
        }

        // ---- focus (hold to heal) ----
        let interrupted = input.buffered(Action::Jump, now, 0)
            || input.buffered(Action::Dash, now, 0)
            || input.buffered(Action::Attack, now, 0)
            || input.buffered(Action::Cast, now, 0);
        let want_focus = input.held(Action::Focus)
            && can_act
            && motor.grounded
            && axis_x == 0
            && cs.attack.is_none()
            && cs.cast_lock == 0
            && soul.value >= c.focus_cost
            && health.hp < health.max
            && !interrupted;
        if want_focus {
            cs.focusing = true;
            cs.focus_ticks += 1;
            if cs.focus_ticks >= c.focus_ticks() {
                soul.value -= c.focus_cost;
                health.hp = (health.hp + c.focus_heal).min(health.max);
                cs.focus_ticks = 0;
            }
        } else {
            cs.focusing = false;
            cs.focus_ticks = 0;
        }
    }
}

/// Slash boxes track their owner. Runs after the owner has moved this tick.
pub fn hitbox_follow(
    mut commands: Commands,
    owners: Query<&SimPos, Without<HitboxFollow>>,
    mut q: Query<(Entity, &HitboxFollow, &mut SimPos)>,
) {
    for (e, follow, mut pos) in &mut q {
        match owners.get(follow.owner) {
            Ok(owner) => pos.0 = owner.0 + follow.rel,
            Err(_) => commands.entity(e).despawn(),
        }
    }
}

pub fn projectile_motion(
    mut commands: Commands,
    grid: Res<TileGrid>,
    mut q: Query<(Entity, &mut SimPos, &Velocity, &Hitbox), With<Projectile>>,
) {
    for (e, mut pos, vel, hb) in &mut q {
        pos.0 += vel.0 * DT;
        if grid.overlaps(pos.0, hb.half, Tile::Solid) {
            commands.entity(e).despawn();
        }
    }
}

/// Hit enemies slide with a decaying push, colliding with terrain.
pub fn knockback_motion(
    mut commands: Commands,
    grid: Res<TileGrid>,
    mut q: Query<(Entity, &mut Knockback, &mut SimPos, &Aabb)>,
) {
    for (e, mut kb, mut pos, aabb) in &mut q {
        if kb.ticks == 0 {
            commands.entity(e).remove::<Knockback>();
            continue;
        }
        let scale = kb.ticks as f32 / kb.total.max(1) as f32;
        let out = move_body(&grid, pos.0, aabb.half, kb.vel * scale * DT, false);
        pos.0 = out.pos;
        kb.ticks -= 1;
        if kb.ticks == 0 {
            commands.entity(e).remove::<Knockback>();
        }
    }
}
