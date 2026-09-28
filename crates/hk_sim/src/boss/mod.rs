//! Bosses: one shared state machine, attacks declared as data.
//!
//! A boss is an [`Enemy`](crate::enemy::Enemy) body (gravity and collision come
//! from the enemy motion system) driven by a [`BossBrain`] instead of an enemy
//! `Brain`. Fight flow:
//!
//! ```text
//! Sleeping -> Intro -> Choose <-> Approach
//!                        |
//!                  Telegraph -> Active -> Recover -> Choose ...
//!                        (phase change: Transition)      (0 HP: Dying)
//! ```
//!
//! Fairness is enforced by data lints (see `tests/boss.rs`): every attack has a
//! tell of at least 450 ms before its first hit and a recovery window of at
//! least 400 ms even in the snappiest phase.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use crate::combat::{
    AttackDir, Disarmed, Health, HitKind, Hitbox, Hurtbox, ManualDeath, Pogoable, Poise, SpawnTag,
    Team,
};
use crate::components::{Aabb, PrevPos, SimPos, Velocity};
use crate::enemy::Enemy;
use crate::tuning::Tuning;
use crate::world::room::RoomEntity;

pub mod ai;

#[derive(Component, Clone, Debug)]
pub struct Boss {
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossState {
    /// Waiting for the player to enter the arena.
    Sleeping,
    /// Waking roar (invulnerable).
    Intro,
    /// Deciding what to do next.
    Choose,
    /// Walking toward the player to get in range.
    Approach,
    /// The tell before an attack.
    Telegraph,
    Active,
    /// The punish window.
    Recover,
    /// Roar between phases (invulnerable).
    Transition,
    Dying,
}

#[derive(Component, Clone, Debug)]
pub struct BossBrain {
    pub state: BossState,
    /// Ticks spent in the current state.
    pub timer: u32,
    /// 1-based.
    pub phase: u8,
    /// Index into the definition's attack list while attacking.
    pub attack: Option<usize>,
    /// The last two attacks used (to avoid a third repeat in a row).
    pub history: [Option<usize>; 2],
    pub facing: i8,
    /// Where the player was when the tell began.
    pub aim: Vec2,
    /// Playable area: (min corner, max corner).
    pub arena: (Vec2, Vec2),
    pub was_grounded: bool,
    /// Ticks since the boss started approaching (approach gives up eventually).
    pub approach_timer: u32,
    /// Counters used by multi-part attacks.
    pub waves_done: u32,
    pub sub_timer: u32,
    /// A Charge ended against a wall: stunned, longer recovery.
    pub hit_wall: bool,
    /// The second arc of a Sweep is winding up (for the visual tell).
    pub second_tell: bool,
    /// Length of the current recovery in ticks.
    pub recover_ticks: u32,
}

impl BossBrain {
    pub fn new(arena: (Vec2, Vec2)) -> Self {
        Self {
            state: BossState::Sleeping,
            timer: 0,
            phase: 1,
            attack: None,
            history: [None, None],
            facing: -1,
            aim: Vec2::ZERO,
            arena,
            was_grounded: true,
            approach_timer: 0,
            waves_done: 0,
            sub_timer: 0,
            hit_wall: false,
            second_tell: false,
            recover_ticks: 0,
        }
    }

    pub(crate) fn enter(&mut self, s: BossState) {
        self.state = s;
        self.timer = 0;
    }
}

/// Marks anything an attack created so it can be cleared when the boss dies.
#[derive(Component)]
pub struct BossSpawn;

/// A floor marker warning that a bell will drop here.
#[derive(Component, Clone, Copy, Debug)]
pub struct Glyph {
    pub ticks: u32,
    pub x: f32,
    pub ceiling_y: f32,
    pub bell_half: Vec2,
}

/// A swinging bell hanging from the ceiling on a chain.
#[derive(Component, Clone, Copy, Debug)]
pub struct Pendulum {
    pub pivot: Vec2,
    pub length: f32,
    /// Sideways swing distance at the bottom of the arc.
    pub amp: f32,
    pub period_ticks: u32,
    pub age: u32,
}

/// While a boss fight is underway the exits are sealed.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct ArenaLock(pub bool);

#[derive(Message, Clone, Debug)]
pub struct BossAwoke {
    pub id: String,
}

/// The boss starts a new phase (its roar): camera shake, music change, ...
#[derive(Message, Clone, Debug)]
pub struct BossPhaseChanged {
    pub id: String,
    pub phase: u8,
}

#[derive(Message, Clone, Debug)]
pub struct BossDefeated {
    pub id: String,
    pub tag: Option<u32>,
}

/// Spawns boss `id` standing at `pos` (bottom-centre) inside `arena`.
pub fn spawn_boss(
    world: &mut World,
    id: &str,
    pos: Vec2,
    arena: (Vec2, Vec2),
) -> Result<Entity, String> {
    let def = world
        .resource::<Tuning>()
        .bosses
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown boss `{id}`"))?;
    let half = Vec2::new(def.half.0, def.half.1);
    let center = Vec2::new(pos.x, pos.y + half.y + crate::world::SKIN);

    let e = world.spawn_empty().id();
    world.entity_mut(e).insert((
        Enemy,
        Boss { id: id.into() },
        BossBrain::new(arena),
        SimPos(center),
        PrevPos(center),
        Velocity::default(),
        Aabb { half },
        Hurtbox {
            half,
            team: Team::Enemy,
        },
        Health::full(def.hp),
        Pogoable,
        Poise(0.0),
        ManualDeath,
        Disarmed,
        Hitbox {
            half,
            team: Team::Enemy,
            damage: def.contact_damage,
            kind: HitKind::Contact,
            attack_dir: AttackDir::Forward,
            once: false,
            owner: e,
        },
        RoomEntity,
    ));
    Ok(e)
}

/// Tag helper so rooms can persist a boss's defeat.
pub fn with_tag(world: &mut World, e: Entity, tag: u32) {
    world.entity_mut(e).insert(SpawnTag(tag));
}
