//! Combat: hitboxes vs hurtboxes, the nail (with pogo), damage, soul, focus,
//! the Ember Bolt spell, hitstop, i-frames, death and respawn.
//!
//! Flow of one tick:
//! `Intent`     player input -> start swings / casts / focus
//! `Motion`     projectiles + knockback move, slash boxes follow their owner
//! `HitDetect`  every hitbox is tested against every hurtbox -> `Hit` messages
//! `HitResolve` each `Hit` applies damage, soul, pogo, knockback, hitstop
//! `Status`     timers tick, death/respawn, safe-ground tracking
//! `Cleanup`    expired hitboxes and projectiles are despawned

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

pub mod attack;
pub mod detect;
pub mod resolve;
pub mod status;

/// Which side an entity fights for. Hazards (spikes, etc.) hurt the player
/// and can be pogoed off, but are never damaged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
    Hazard,
}

impl Team {
    /// Can a hitbox of `self` hit a hurtbox of `other`?
    pub fn can_hit(self, other: Team) -> bool {
        matches!(
            (self, other),
            (Team::Player, Team::Enemy | Team::Hazard) | (Team::Enemy | Team::Hazard, Team::Player)
        )
    }
}

/// Hit points: masks for the player, damage units for enemies.
#[derive(Component, Clone, Copy, Debug)]
pub struct Health {
    pub hp: i32,
    pub max: i32,
}

impl Health {
    pub fn full(max: i32) -> Self {
        Self { hp: max, max }
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Soul {
    pub value: i32,
    pub max: i32,
}

/// Where damage can land. Centred on `SimPos`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Hurtbox {
    pub half: Vec2,
    pub team: Team,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Nail,
    Spell,
    /// Body contact from an enemy.
    Contact,
    /// Spikes and similar: damage, then respawn at safe ground.
    Hazard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AttackDir {
    #[default]
    Forward,
    Up,
    Down,
}

/// A damaging region. Centred on the entity's `SimPos`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Hitbox {
    pub half: Vec2,
    pub team: Team,
    pub damage: i32,
    pub kind: HitKind,
    pub attack_dir: AttackDir,
    /// Hits each victim at most once (swings, projectiles). Persistent boxes
    /// (contact, hazards) rely on the victim's i-frames instead.
    pub once: bool,
    /// The creature that created this box; never hits itself.
    pub owner: Entity,
}

/// Victims already struck by a `once` hitbox.
#[derive(Component, Default, Debug)]
pub struct AlreadyHit(pub Vec<Entity>);

/// A hitbox that tracks its owner: `SimPos = owner.SimPos + rel`.
#[derive(Component, Clone, Copy, Debug)]
pub struct HitboxFollow {
    pub owner: Entity,
    pub rel: Vec2,
}

/// Despawned when it reaches zero.
#[derive(Component, Clone, Copy, Debug)]
pub struct Lifetime(pub u32);

/// Hitting this with a down-slash bounces the player.
#[derive(Component, Default)]
pub struct Pogoable;

/// Cannot be hit while > 0 (ticks).
#[derive(Component, Clone, Copy, Debug)]
pub struct Invulnerable(pub u32);

/// Scales knockback taken: 1 = normal, 0 = unstaggerable.
#[derive(Component, Clone, Copy, Debug)]
pub struct Poise(pub f32);

/// Decaying push applied to a hit enemy (moved with collision).
#[derive(Component, Clone, Copy, Debug)]
pub struct Knockback {
    pub vel: Vec2,
    pub ticks: u32,
    pub total: u32,
}

/// Player projectile.
#[derive(Component, Default)]
pub struct Projectile;

/// Last stable standing spot; spikes respawn the player here.
#[derive(Component, Clone, Copy, Debug)]
pub struct SafeGround {
    pub pos: Vec2,
    pub stable_ticks: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Attack {
    pub dir: AttackDir,
    /// Facing locked in when the swing started: +1 right, -1 left.
    pub facing: i8,
    /// Ticks since the swing started (0 on the start tick).
    pub age: u32,
    pub hitbox: Option<Entity>,
}

/// Player combat bookkeeping. All timers in ticks.
#[derive(Component, Default, Debug)]
pub struct CombatState {
    pub attack: Option<Attack>,
    pub attack_cooldown: u32,
    /// Hurt: no actions, input control lost.
    pub stun: u32,
    /// Horizontal control lost (recoil), actions still allowed.
    pub control_lock: u32,
    /// Casting: horizontal control lost.
    pub cast_lock: u32,
    pub cast_cooldown: u32,
    pub focusing: bool,
    pub focus_ticks: u32,
    pub dead: bool,
    /// Counts down while dead; respawns at zero.
    pub dead_ticks: u32,
}

/// Global freeze frames: while > 0 the sim is frozen (input still latches).
#[derive(Resource, Default, Debug)]
pub struct HitStop(pub u32);

/// True for the ticks in which hitstop freezes gameplay.
#[derive(Resource, Default, Debug)]
pub struct SimFrozen(pub bool);

/// Where the player comes back after dying (set by benches).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RespawnPoint(pub Vec2);

pub fn not_frozen(frozen: Res<SimFrozen>) -> bool {
    !frozen.0
}

/// One hitbox struck one hurtbox. Every consequence (damage, soul, pogo,
/// knockback, hitstop, VFX, audio, shake) is driven from this message.
#[derive(Message, Clone, Copy, Debug)]
pub struct Hit {
    pub hitbox: Entity,
    pub source: Entity,
    pub victim: Entity,
    pub victim_team: Team,
    pub damage: i32,
    pub kind: HitKind,
    pub attack_dir: AttackDir,
    /// Horizontal direction the victim is pushed: +1 right, -1 left.
    pub dir: i8,
    /// Approximate contact point (for sparks).
    pub pos: Vec2,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct PlayerDied;

#[derive(Message, Clone, Copy, Debug)]
pub struct PlayerRespawned;

#[derive(Message, Clone, Copy, Debug)]
pub struct EnemyDied {
    pub entity: Entity,
    pub pos: Vec2,
}
