//! Per-tick bookkeeping: hitstop, timers, death/respawn, safe ground.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::*;
use crate::components::{SimPos, Velocity};
use crate::ms_to_ticks;
use crate::player::{Motor, Player};
use crate::tuning::Tuning;

/// Decides whether this tick is frozen by hitstop. Runs in `SimSet::Input`,
/// which is never gated, so input keeps latching during freeze frames.
pub fn advance_hitstop(mut hs: ResMut<HitStop>, mut frozen: ResMut<SimFrozen>) {
    if hs.0 > 0 {
        hs.0 -= 1;
        frozen.0 = true;
    } else {
        frozen.0 = false;
    }
}

/// Short i-frames after coming back from death.
const RESPAWN_IFRAMES_MS: f32 = 500.0;

/// Safe ground is never recorded this close to a hazard (world units), so a
/// spike respawn can't drop the player right back onto the spikes.
const HAZARD_MARGIN: f32 = 1.5;

#[allow(clippy::type_complexity)]
pub fn player_status(
    mut commands: Commands,
    tuning: Res<Tuning>,
    respawn: Res<RespawnPoint>,
    mut respawned: MessageWriter<PlayerRespawned>,
    hazards: Query<(&SimPos, &Hitbox), Without<Player>>,
    mut q: Query<
        (
            Entity,
            &mut Health,
            &mut Soul,
            &mut CombatState,
            &mut SimPos,
            &mut Velocity,
            &mut Motor,
            &mut SafeGround,
        ),
        With<Player>,
    >,
) {
    let c = &tuning.combat;
    for (entity, mut hp, mut soul, mut cs, mut pos, mut vel, mut motor, mut safe) in &mut q {
        if cs.dead {
            cs.dead_ticks = cs.dead_ticks.saturating_sub(1);
            if cs.dead_ticks == 0 {
                hp.hp = hp.max;
                soul.value = 0;
                pos.0 = respawn.0;
                vel.0 = Vec2::ZERO;
                *motor = Motor::default();
                *cs = CombatState::default();
                safe.pos = respawn.0;
                safe.stable_ticks = 0;
                commands
                    .entity(entity)
                    .insert(Invulnerable(ms_to_ticks(RESPAWN_IFRAMES_MS)));
                respawned.write(PlayerRespawned);
            }
            continue;
        }

        cs.stun = cs.stun.saturating_sub(1);
        cs.control_lock = cs.control_lock.saturating_sub(1);
        cs.cast_lock = cs.cast_lock.saturating_sub(1);

        // Track the latest spot we have stood on for a moment, away from spikes.
        if motor.grounded {
            safe.stable_ticks += 1;
            let near_hazard = hazards.iter().any(|(hp_, hb)| {
                hb.kind == HitKind::Hazard
                    && (hp_.0.x - pos.0.x).abs() < hb.half.x + 0.4 + HAZARD_MARGIN
                    && (hp_.0.y - pos.0.y).abs() < hb.half.y + 0.75 + HAZARD_MARGIN
            });
            if safe.stable_ticks >= c.safe_ground_ticks() && !near_hazard {
                safe.pos = pos.0;
            }
        } else {
            safe.stable_ticks = 0;
        }
    }
}

/// I-frames and lifetimes count down.
pub fn tick_timers(
    mut commands: Commands,
    mut inv: Query<(Entity, &mut Invulnerable)>,
    mut life: Query<&mut Lifetime>,
) {
    for (e, mut i) in &mut inv {
        i.0 = i.0.saturating_sub(1);
        if i.0 == 0 {
            commands.entity(e).remove::<Invulnerable>();
        }
    }
    for mut l in &mut life {
        l.0 = l.0.saturating_sub(1);
    }
}

pub fn cleanup_expired(mut commands: Commands, q: Query<(Entity, &Lifetime)>) {
    for (e, l) in &q {
        if l.0 == 0 {
            commands.entity(e).despawn();
        }
    }
}
