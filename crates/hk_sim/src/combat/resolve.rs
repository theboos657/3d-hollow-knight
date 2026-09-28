//! Applies each `Hit`: damage, i-frames, knockback, soul, pogo, hitstop.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::*;
use crate::components::{SimPos, Velocity};
use crate::player::{Motor, Player};
use crate::tuning::Tuning;
pub fn resolve_hits(
    mut commands: Commands,
    mut hits: MessageReader<Hit>,
    tuning: Res<Tuning>,
    mut hitstop: ResMut<HitStop>,
    mut died: MessageWriter<PlayerDied>,
    mut enemy_died: MessageWriter<EnemyDied>,
    mut players: Query<
        (
            &mut Health,
            &mut Soul,
            &mut CombatState,
            &mut Velocity,
            &mut SimPos,
            &mut Motor,
            &SafeGround,
        ),
        With<Player>,
    >,
    mut enemies: Query<(&mut Health, Option<&Poise>, &SimPos, Option<&SpawnTag>), Without<Player>>,
    pogoable: Query<(), With<Pogoable>>,
) {
    let c = &tuning.combat;

    for h in hits.read() {
        // Enemy projectiles are spent when they hit.
        if h.kind == HitKind::Projectile {
            commands.entity(h.hitbox).despawn();
        }

        // ------------------------------------------------ the victim ------
        match h.victim_team {
            Team::Player => {
                let Ok((mut hp, _, mut cs, mut vel, mut pos, mut motor, safe)) =
                    players.get_mut(h.victim)
                else {
                    continue;
                };
                if cs.dead {
                    continue;
                }
                hp.hp = (hp.hp - h.damage).max(0);
                // Timers set here are ticked down once more by this same
                // tick's Status pass, hence the +1 (so the player really gets
                // `stun_ticks` / `iframes_ticks` full ticks after the hit).
                cs.stun = c.stun_ticks() + 1;
                cs.control_lock = 0;
                cs.cast_lock = 0;
                cs.focusing = false;
                cs.focus_ticks = 0;
                if let Some(att) = cs.attack.take() {
                    if let Some(hb) = att.hitbox {
                        commands.entity(hb).despawn();
                    }
                }
                motor.dash_ticks_left = 0;
                motor.jumping = false;
                vel.0 = Vec2::new(h.dir as f32 * c.hurt_knock_vx, c.hurt_knock_vy);
                if h.kind == HitKind::Hazard {
                    // Spikes: back to the last stable spot instead of a shove.
                    pos.0 = safe.pos;
                    vel.0 = Vec2::ZERO;
                }
                commands
                    .entity(h.victim)
                    .insert(Invulnerable(c.iframes_ticks() + 1));
                hitstop.0 = hitstop.0.max(c.hitstop_hurt_ticks());
                if hp.hp == 0 {
                    cs.dead = true;
                    cs.dead_ticks = c.respawn_delay_ticks();
                    vel.0 = Vec2::ZERO;
                    died.write(PlayerDied);
                }
            }
            Team::Enemy => {
                if let Ok((mut hp, poise, pos, tag)) = enemies.get_mut(h.victim) {
                    if hp.hp > 0 {
                        hp.hp -= h.damage;
                        if hp.hp <= 0 {
                            enemy_died.write(EnemyDied {
                                entity: h.victim,
                                pos: pos.0,
                                tag: tag.map(|t| t.0),
                            });
                            commands.entity(h.victim).despawn();
                        } else {
                            let p = poise.map_or(1.0, |p| p.0);
                            if p > 0.0 && h.attack_dir == AttackDir::Forward {
                                let total = c.enemy_knock_ticks();
                                commands.entity(h.victim).insert(Knockback {
                                    vel: Vec2::new(h.dir as f32 * c.enemy_knock_speed / p, 0.0),
                                    ticks: total,
                                    total,
                                });
                            }
                        }
                    }
                }
            }
            Team::Hazard => {}
        }

        // ------------------------------------------------ the attacker -----
        if h.kind == HitKind::Nail && h.victim_team != Team::Player {
            hitstop.0 = hitstop.0.max(c.hitstop_nail_ticks());
            if let Ok((_, mut soul, mut cs, mut vel, _, mut motor, _)) = players.get_mut(h.source) {
                if h.victim_team == Team::Enemy {
                    soul.value = (soul.value + c.soul_per_hit).min(soul.max);
                }
                if h.attack_dir == AttackDir::Down && pogoable.contains(h.victim) {
                    vel.y = c.pogo_speed;
                    motor.air_dash_ready = true;
                    motor.jumping = false;
                    motor.coyote = 0;
                } else if h.attack_dir == AttackDir::Forward && h.victim_team == Team::Enemy {
                    // Small push back off the thing we hit.
                    vel.x = -(h.dir as f32) * c.nail_recoil_speed;
                    cs.control_lock = c.nail_recoil_ticks() + 1;
                }
            }
        }
    }
}

/// Shield blocks: small freeze and a push back, but no damage and no soul.
pub fn resolve_blocks(
    mut commands: Commands,
    mut blocked: MessageReader<Blocked>,
    tuning: Res<Tuning>,
    mut hitstop: ResMut<HitStop>,
    mut players: Query<(&mut Velocity, &mut CombatState), With<Player>>,
) {
    let c = &tuning.combat;
    for b in blocked.read() {
        hitstop.0 = hitstop.0.max(c.hitstop_block_ticks());
        match b.kind {
            HitKind::Nail => {
                if let Ok((mut vel, mut cs)) = players.get_mut(b.source) {
                    vel.x = -(b.dir as f32) * c.block_recoil_speed;
                    cs.control_lock = c.nail_recoil_ticks() + 1;
                }
            }
            // A bolt stops dead against a shield.
            HitKind::Spell => commands.entity(b.hitbox).despawn(),
            _ => {}
        }
    }
}
