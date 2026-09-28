//! Progress: benches (rest, heal, set the respawn point), ability pickups,
//! boss rewards, respawning at the last bench, and the save file format.

use std::collections::HashSet;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use super::room::*;
use crate::boss::{ArenaLock, BossDefeated};
use crate::combat::{CombatState, Health, RespawnPoint, Soul};
use crate::components::{Aabb, SimPos};
use crate::input::{Action, InputState};
use crate::player::{spawn_player, Abilities, Facing, Motor, Player};
use crate::SimTick;

/// Where the player comes back after dying: the last bench, or the start.
/// Empty `room` = no checkpoint yet (the player just respawns in place).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Checkpoint {
    pub room: String,
    /// Feet position.
    pub pos: Vec2,
    pub facing: i8,
}

impl Checkpoint {
    pub fn is_set(&self) -> bool {
        !self.room.is_empty()
    }
}

/// How the run is going (shown on the end screen, kept in the save).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    pub deaths: u32,
    /// Simulation ticks played (hitstop and room fades excluded).
    pub ticks: u64,
}

impl RunStats {
    pub fn seconds(&self) -> f32 {
        self.ticks as f32 / crate::TICK_HZ as f32
    }
}

/// Counts deaths and time. Runs in `SimSet::Status`, so it pauses with the sim.
pub fn track_stats(
    mut stats: ResMut<RunStats>,
    mut died: MessageReader<crate::combat::PlayerDied>,
) {
    stats.ticks += 1;
    stats.deaths += died.read().count() as u32;
}

/// The player sat down at a bench (healed, respawn point set).
#[derive(Message, Clone, Copy, Debug)]
pub struct BenchRested;

/// The player learned something new.
#[derive(Message, Clone, Copy, Debug)]
pub struct AbilityGained {
    pub ability: Ability,
}

/// Which ability a boss hands over when defeated.
pub fn boss_reward(boss_id: &str) -> Option<Ability> {
    match boss_id {
        "matron" => Some(Ability::Dash),
        _ => None,
    }
}

/// Ticks a press of Up stays valid for sitting down.
const REST_BUFFER: u32 = 6;

/// Standing at a bench and pressing Up: sit. Heals, refills soul, sets the
/// respawn point, and reloads the room (enemies come back) behind a fade.
pub fn bench_rest(
    tick: Res<SimTick>,
    lock: Res<ArenaLock>,
    current: Res<CurrentRoom>,
    mut input: ResMut<InputState>,
    mut checkpoint: ResMut<Checkpoint>,
    mut respawn: ResMut<RespawnPoint>,
    mut tr: ResMut<Transition>,
    mut rested: MessageWriter<BenchRested>,
    benches: Query<(&SimPos, &Bench)>,
    mut players: Query<
        (
            &SimPos,
            &Aabb,
            &Motor,
            &Facing,
            &CombatState,
            &mut Health,
            &mut Soul,
        ),
        With<Player>,
    >,
) {
    if tr.active() || lock.0 {
        return;
    }
    for (pos, aabb, motor, facing, cs, mut hp, mut soul) in &mut players {
        if cs.dead || cs.stun > 0 || !motor.grounded {
            continue;
        }
        let Some((_, b)) = benches.iter().find(|(bp, b)| {
            let d = (pos.0 - bp.0).abs();
            // A generous reach: the bench is about a body wide.
            d.x < aabb.half.x + b.half.x + 0.4 && d.y < aabb.half.y + b.half.y + 0.2
        }) else {
            continue;
        };
        if !input.consume(Action::Up, tick.0, REST_BUFFER) {
            continue;
        }
        hp.hp = hp.max;
        soul.value = soul.max;
        let feet = b.base;
        *checkpoint = Checkpoint {
            room: current.id.clone(),
            pos: feet,
            facing: facing.0,
        };
        respawn.0 = pos.0;
        rested.write(BenchRested);
        tr.phase = Phase::Out;
        tr.ticks = FADE_TICKS;
        tr.to = current.id.clone();
        tr.entry = String::new();
        tr.at = Some((feet, facing.0));
        return;
    }
}

/// Walking into a pickup teaches the ability.
pub fn collect_pickups(
    mut commands: Commands,
    mut flags: ResMut<WorldFlags>,
    mut gained: MessageWriter<AbilityGained>,
    pickups: Query<(Entity, &SimPos, &Pickup)>,
    mut players: Query<(&SimPos, &Aabb, &mut Abilities), With<Player>>,
) {
    for (p, pa, mut abil) in &mut players {
        for (e, xp, pick) in &pickups {
            let d = (p.0 - xp.0).abs();
            if d.x < pa.half.x + pick.half.x && d.y < pa.half.y + pick.half.y {
                abil.grant(pick.ability);
                flags.collected.insert(pick.tag);
                gained.write(AbilityGained {
                    ability: pick.ability,
                });
                commands.entity(e).despawn();
            }
        }
    }
}

/// Some bosses hand over an ability when they fall.
pub fn boss_rewards(
    mut defeated: MessageReader<BossDefeated>,
    mut gained: MessageWriter<AbilityGained>,
    mut players: Query<&mut Abilities, With<Player>>,
) {
    for d in defeated.read() {
        let Some(ability) = boss_reward(&d.id) else {
            continue;
        };
        for mut abil in &mut players {
            abil.grant(ability);
        }
        gained.write(AbilityGained { ability });
    }
}

/// After dying, wake up at the last bench (the room reloads, bosses reset).
/// `player_status` has already restored health.
pub fn respawn_at_checkpoint(
    mut respawned: MessageReader<crate::combat::PlayerRespawned>,
    checkpoint: Res<Checkpoint>,
    mut tr: ResMut<Transition>,
) {
    if respawned.read().count() == 0 || !checkpoint.is_set() {
        return;
    }
    tr.phase = Phase::Out;
    tr.ticks = FADE_TICKS;
    tr.to = checkpoint.room.clone();
    tr.entry = String::new();
    tr.at = Some((checkpoint.pos, checkpoint.facing));
}

// ------------------------------------------------------------------- save --

pub const SAVE_VERSION: u32 = 1;

/// Everything worth keeping between sessions (RON on disk).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub dash: bool,
    pub wall_grip: bool,
    pub defeated: Vec<u32>,
    pub collected: Vec<u32>,
    pub room: String,
    pub x: f32,
    pub y: f32,
    pub facing: i8,
    #[serde(default)]
    pub deaths: u32,
    #[serde(default)]
    pub play_ticks: u64,
}

impl SaveData {
    /// Snapshots the world: progress flags, abilities and the last bench.
    pub fn capture(world: &mut World) -> Self {
        let abil = world
            .query_filtered::<&Abilities, With<Player>>()
            .iter(world)
            .next()
            .copied()
            .unwrap_or_default();
        let flags = world.resource::<WorldFlags>();
        let mut defeated: Vec<u32> = flags.defeated.iter().copied().collect();
        let mut collected: Vec<u32> = flags.collected.iter().copied().collect();
        defeated.sort_unstable();
        collected.sort_unstable();
        let cp = world.resource::<Checkpoint>().clone();
        let stats = *world.resource::<RunStats>();
        Self {
            version: SAVE_VERSION,
            dash: abil.dash,
            wall_grip: abil.wall_grip,
            defeated,
            collected,
            room: cp.room,
            x: cp.pos.x,
            y: cp.pos.y,
            facing: cp.facing,
            deaths: stats.deaths,
            play_ticks: stats.ticks,
        }
    }

    /// Restores a snapshot: spawns the player if needed and enters the room.
    pub fn apply(&self, world: &mut World) -> Result<(), String> {
        if self.version != SAVE_VERSION {
            return Err(format!("save version {} is not supported", self.version));
        }
        {
            let mut flags = world.resource_mut::<WorldFlags>();
            flags.defeated = self.defeated.iter().copied().collect::<HashSet<_>>();
            flags.collected = self.collected.iter().copied().collect::<HashSet<_>>();
        }
        let abilities = Abilities {
            dash: self.dash,
            wall_grip: self.wall_grip,
        };
        let existing = world
            .query_filtered::<Entity, With<Player>>()
            .iter(world)
            .next();
        match existing {
            Some(p) => {
                world.entity_mut(p).insert(abilities);
            }
            None => {
                spawn_player(world, Vec2::ZERO, abilities);
            }
        }
        *world.resource_mut::<RunStats>() = RunStats {
            deaths: self.deaths,
            ticks: self.play_ticks,
        };
        let feet = Vec2::new(self.x, self.y);
        *world.resource_mut::<Checkpoint>() = Checkpoint {
            room: self.room.clone(),
            pos: feet,
            facing: self.facing,
        };
        enter_room_at(world, &self.room, feet, self.facing)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|e| format!("// could not serialise save: {e}"))
    }

    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }
}
