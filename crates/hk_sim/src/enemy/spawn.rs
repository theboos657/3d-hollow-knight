//! Building enemies from tuning.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::{Brain, Enemy, EnemyKind, Flying};
use crate::combat::{AttackDir, Guard, Health, HitKind, Hitbox, Hurtbox, Pogoable, Poise, Team};
use crate::components::{Aabb, PrevPos, SimPos, Velocity};
use crate::tuning::Tuning;

/// Spawns an enemy of `kind` at `pos` (box centre) using current tuning.
pub fn spawn_enemy(world: &mut World, kind: EnemyKind, pos: Vec2) -> Entity {
    let t = world.resource::<Tuning>().enemies.clone();
    let (hp, half, poise) = match kind {
        EnemyKind::Husk => (t.husk.hp, t.husk.half, t.husk.poise),
        EnemyKind::Wisp => (t.wisp.hp, t.wisp.half, t.wisp.poise),
        EnemyKind::Shieldbearer => (t.shield.hp, t.shield.half, t.shield.poise),
        EnemyKind::Spitter => (t.spitter.hp, t.spitter.half, t.spitter.poise),
    };
    let half = Vec2::new(half.0, half.1);

    let e = world.spawn_empty().id();
    let mut ent = world.entity_mut(e);
    ent.insert((
        Enemy,
        Brain::new(kind, pos),
        SimPos(pos),
        PrevPos(pos),
        Velocity::default(),
        Aabb { half },
        Hurtbox {
            half,
            team: Team::Enemy,
        },
        Health::full(hp),
        Pogoable,
        Poise(poise),
        // Touching the body hurts.
        Hitbox {
            half,
            team: Team::Enemy,
            damage: t.contact_damage,
            kind: HitKind::Contact,
            attack_dir: AttackDir::Forward,
            once: false,
            owner: e,
        },
    ));
    match kind {
        EnemyKind::Wisp => {
            ent.insert(Flying);
        }
        EnemyKind::Shieldbearer => {
            ent.insert(Guard { facing: 1 });
        }
        _ => {}
    }
    e
}
