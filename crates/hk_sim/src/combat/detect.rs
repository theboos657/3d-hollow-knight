//! Overlap tests: every hitbox against every hurtbox, once per tick.

use bevy_ecs::prelude::*;

use super::*;
use crate::components::SimPos;
use crate::player::Facing;

#[allow(clippy::type_complexity)]
pub fn detect_hits(
    mut hits: MessageWriter<Hit>,
    mut hitboxes: Query<(Entity, &Hitbox, &SimPos, Option<&mut AlreadyHit>)>,
    hurtboxes: Query<(Entity, &Hurtbox, &SimPos, Option<&Invulnerable>)>,
    positions: Query<&SimPos>,
    facings: Query<&Facing>,
) {
    // Persistent boxes (contact, hazards) may hit a victim at most once per
    // tick, so two overlapping enemies never deal double damage.
    let mut struck_by_persistent: Vec<Entity> = Vec::new();

    for (hb_entity, hb, hb_pos, mut already) in &mut hitboxes {
        for (victim, hu, v_pos, inv) in &hurtboxes {
            if victim == hb.owner || !hb.team.can_hit(hu.team) {
                continue;
            }
            if inv.is_some_and(|i| i.0 > 0) {
                continue;
            }
            let d = (v_pos.0 - hb_pos.0).abs();
            if d.x >= hb.half.x + hu.half.x || d.y >= hb.half.y + hu.half.y {
                continue;
            }
            if hb.once {
                if let Some(a) = already.as_mut() {
                    if a.0.contains(&victim) {
                        continue;
                    }
                    a.0.push(victim);
                }
            } else {
                if struck_by_persistent.contains(&victim) {
                    continue;
                }
                struck_by_persistent.push(victim);
            }

            let src = positions.get(hb.owner).map(|p| p.0).unwrap_or(hb_pos.0);
            let dx = v_pos.0.x - src.x;
            let dir = if dx > 0.0 {
                1
            } else if dx < 0.0 {
                -1
            } else {
                facings.get(hb.owner).map(|f| f.0).unwrap_or(1)
            };
            hits.write(Hit {
                hitbox: hb_entity,
                source: hb.owner,
                victim,
                victim_team: hu.team,
                damage: hb.damage,
                kind: hb.kind,
                attack_dir: hb.attack_dir,
                dir,
                pos: (hb_pos.0 + v_pos.0) * 0.5,
            });
        }
    }
}
