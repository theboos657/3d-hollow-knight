//! Rooms: the data format (RON), validation, building a room into the world,
//! and the exit -> fade -> swap -> fade-in transition.
//!
//! A room is authored as a text grid plus placements:
//!
//! ```ron
//! (
//!     id: "A1", name: "The Landing", theme: Ashen,
//!     tiles: ["####", "#..#", "####"],           // top row first
//!     spawns:  [(kind: Husk, at: (12.0, 2.0))],   // `at` = bottom-centre of the body
//!     entries: [(name: "west", at: (3.0, 2.0), facing: 1)],
//!     exits:   [(rect: (0.0, 2.0, 1.0, 4.0), to: "A2", entry: "east")],
//! )
//! ```
//!
//! Tile legend: `#` solid, `=` one-way platform, `^` floor spikes, anything else air.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use crate::combat::{
    AttackDir, CombatState, HitKind, Hitbox, Hurtbox, Pogoable, Projectile, SafeGround, SpawnTag,
    Team,
};
use crate::components::{Aabb, PrevPos, SimPos, Velocity};
use crate::enemy::{spawn_enemy, EnemyKind};
use crate::input::{Action, InputState};
use crate::player::{Facing, Motor, Player};
use crate::world::grid::{Tile, TileGrid, SKIN};

// ------------------------------------------------------------------ data --

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    Sandbox,
    Ashen,
    Warrens,
    Cistern,
    Spire,
    Throne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpawnKind {
    /// Passive punching bag (sandbox / training).
    Dummy,
    Husk,
    Wisp,
    Shieldbearer,
    Spitter,
    /// Mid-boss: grants Dash when defeated.
    Matron,
    /// The final boss.
    Bellwarden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ability {
    Dash,
    WallGrip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnDef {
    pub kind: SpawnKind,
    /// Bottom-centre of the body, in world units.
    pub at: (f32, f32),
    /// Stays dead once defeated (bosses); otherwise it respawns with the room.
    #[serde(default)]
    pub persistent: bool,
}

fn one() -> i8 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntryDef {
    pub name: String,
    /// Where the player's feet are placed.
    pub at: (f32, f32),
    #[serde(default = "one")]
    pub facing: i8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExitDef {
    /// (x, y, w, h) in world units; touching it leaves the room.
    pub rect: (f32, f32, f32, f32),
    pub to: String,
    pub entry: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchDef {
    /// Where the bench's base sits.
    pub at: (f32, f32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickupDef {
    pub ability: Ability,
    pub at: (f32, f32),
}

/// A room as authored on disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoomDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub theme: Theme,
    /// Rows of tiles, **top row first**.
    pub tiles: Vec<String>,
    #[serde(default)]
    pub spawns: Vec<SpawnDef>,
    #[serde(default)]
    pub entries: Vec<EntryDef>,
    #[serde(default)]
    pub exits: Vec<ExitDef>,
    #[serde(default)]
    pub benches: Vec<BenchDef>,
    #[serde(default)]
    pub pickups: Vec<PickupDef>,
}

impl RoomDef {
    pub fn width(&self) -> i32 {
        self.tiles.first().map_or(0, |r| r.chars().count() as i32)
    }

    pub fn height(&self) -> i32 {
        self.tiles.len() as i32
    }

    pub fn grid(&self) -> TileGrid {
        let rows: Vec<&str> = self.tiles.iter().map(|s| s.as_str()).collect();
        TileGrid::from_ascii(&rows)
    }

    pub fn entry(&self, name: &str) -> Option<&EntryDef> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Stable tag for spawn `i` (used for respawn and persistence).
    pub fn spawn_tag(&self, i: usize) -> u32 {
        // FNV-1a of the room id, with the spawn index in the low 12 bits.
        let mut h: u32 = 0x811c_9dc5;
        for b in self.id.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h & 0xFFFF_F000) | (i as u32 & 0xFFF)
    }

    /// Self-contained checks (cross-room links are checked by [`RoomLibrary::validate`]).
    pub fn validate(&self) -> Result<(), String> {
        let ctx = |m: String| format!("room `{}`: {m}", self.id);
        if self.tiles.is_empty() {
            return Err(ctx("has no tiles".into()));
        }
        let w = self.width();
        for (i, r) in self.tiles.iter().enumerate() {
            if r.chars().count() as i32 != w {
                return Err(ctx(format!(
                    "row {i} has width {} but row 0 has {w}",
                    r.chars().count()
                )));
            }
        }
        let grid = self.grid();
        let (wf, hf) = (w as f32, self.height() as f32);
        let inside = |x: f32, y: f32| x >= 0.0 && x <= wf && y >= 0.0 && y <= hf;

        for (i, s) in self.spawns.iter().enumerate() {
            if !inside(s.at.0, s.at.1) {
                return Err(ctx(format!("spawn {i} at {:?} is outside the room", s.at)));
            }
            let probe = Vec2::new(s.at.0, s.at.1 + 0.3);
            if grid.overlaps(probe, Vec2::new(0.2, 0.2), Tile::Solid) {
                return Err(ctx(format!("spawn {i} at {:?} is inside solid rock", s.at)));
            }
        }
        for e in &self.entries {
            if !inside(e.at.0, e.at.1) {
                return Err(ctx(format!("entry `{}` is outside the room", e.name)));
            }
            if grid.overlaps(
                Vec2::new(e.at.0, e.at.1 + 0.75),
                Vec2::new(0.3, 0.7),
                Tile::Solid,
            ) {
                return Err(ctx(format!("entry `{}` is inside solid rock", e.name)));
            }
        }
        let names: HashSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        if names.len() != self.entries.len() {
            return Err(ctx("duplicate entry names".into()));
        }
        for x in &self.exits {
            let (rx, ry, rw, rh) = x.rect;
            if rw <= 0.0 || rh <= 0.0 {
                return Err(ctx(format!("exit to `{}` has an empty rect", x.to)));
            }
            if rx + rw < 0.0 || ry + rh < 0.0 || rx > wf || ry > hf {
                return Err(ctx(format!("exit to `{}` is outside the room", x.to)));
            }
            // Arriving inside an exit would bounce the player straight back out.
            for e in &self.entries {
                let inside_exit = e.at.0 > rx
                    && e.at.0 < rx + rw
                    && e.at.1 + 0.75 > ry
                    && e.at.1 + 0.75 < ry + rh;
                if inside_exit {
                    return Err(ctx(format!(
                        "entry `{}` lies inside the exit to `{}`",
                        e.name, x.to
                    )));
                }
            }
        }
        for b in &self.benches {
            if !inside(b.at.0, b.at.1) {
                return Err(ctx("a bench is outside the room".into()));
            }
        }
        Ok(())
    }
}

// --------------------------------------------------------------- library --

#[derive(Resource, Default, Clone, Debug)]
pub struct RoomLibrary {
    rooms: HashMap<String, RoomDef>,
}

impl RoomLibrary {
    pub fn from_defs(defs: Vec<RoomDef>) -> Self {
        Self {
            rooms: defs.into_iter().map(|d| (d.id.clone(), d)).collect(),
        }
    }

    /// Loads every `*.room.ron` in `dir`.
    pub fn load_dir(dir: &Path) -> Result<Self, String> {
        let mut defs = Vec::new();
        let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut paths: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".room.ron") {
                continue;
            }
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let def: RoomDef = ron::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
            defs.push(def);
        }
        Ok(Self::from_defs(defs))
    }

    pub fn get(&self, id: &str) -> Option<&RoomDef> {
        self.rooms.get(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.rooms.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }

    /// Checks every room, and that every exit leads to a real room and entry.
    pub fn validate(&self) -> Result<(), String> {
        for id in self.ids() {
            let r = &self.rooms[id];
            r.validate()?;
            for x in &r.exits {
                let target = self
                    .rooms
                    .get(&x.to)
                    .ok_or_else(|| format!("room `{id}`: exit leads to unknown room `{}`", x.to))?;
                if target.entry(&x.entry).is_none() {
                    return Err(format!(
                        "room `{id}`: exit to `{}` names missing entry `{}`",
                        x.to, x.entry
                    ));
                }
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------ components --

/// Everything spawned for the current room; despawned on room change.
#[derive(Component)]
pub struct RoomEntity;

#[derive(Component, Clone, Debug)]
pub struct RoomExit {
    pub half: Vec2,
    pub to: String,
    pub entry: String,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Bench {
    pub half: Vec2,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Pickup {
    pub ability: Ability,
    pub half: Vec2,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct CurrentRoom {
    pub id: String,
}

/// Progress that survives room changes (persistent spawns already defeated).
#[derive(Resource, Default, Clone, Debug)]
pub struct WorldFlags {
    pub defeated: HashSet<u32>,
}

#[derive(Message, Clone, Debug)]
pub struct RoomEntered {
    pub id: String,
}

// -------------------------------------------------------------- building --

fn kind_of(k: SpawnKind) -> Option<EnemyKind> {
    match k {
        SpawnKind::Dummy | SpawnKind::Matron | SpawnKind::Bellwarden => None,
        SpawnKind::Husk => Some(EnemyKind::Husk),
        SpawnKind::Wisp => Some(EnemyKind::Wisp),
        SpawnKind::Shieldbearer => Some(EnemyKind::Shieldbearer),
        SpawnKind::Spitter => Some(EnemyKind::Spitter),
    }
}

/// Spawns one room spawn point (also used to respawn after a delay).
pub fn spawn_from_def(world: &mut World, def: &RoomDef, index: usize) -> Option<Entity> {
    let s = def.spawns.get(index)?;
    let tag = def.spawn_tag(index);
    let (x, y) = s.at;
    // Bosses are built by the boss module, inside the room's playable area.
    let boss_id = match s.kind {
        SpawnKind::Matron => Some("matron"),
        SpawnKind::Bellwarden => Some("bellwarden"),
        _ => None,
    };
    if let Some(id) = boss_id {
        let arena = (
            Vec2::new(1.0, 1.0),
            Vec2::new(def.width() as f32 - 1.0, def.height() as f32 - 1.0),
        );
        return match crate::boss::spawn_boss(world, id, Vec2::new(x, y), arena) {
            Ok(e) => {
                world.entity_mut(e).insert(SpawnTag(tag));
                Some(e)
            }
            Err(msg) => {
                eprintln!("{msg}");
                None
            }
        };
    }
    match kind_of(s.kind) {
        Some(kind) => {
            // `at` is the bottom of the body; enemies are placed by centre.
            let half_y = {
                let t = &world.resource::<crate::tuning::Tuning>().enemies;
                match kind {
                    EnemyKind::Husk => t.husk.half.1,
                    EnemyKind::Wisp => t.wisp.half.1,
                    EnemyKind::Shieldbearer => t.shield.half.1,
                    EnemyKind::Spitter => t.spitter.half.1,
                }
            };
            let e = spawn_enemy(world, kind, Vec2::new(x, y + half_y + SKIN));
            world.entity_mut(e).insert((SpawnTag(tag), RoomEntity));
            Some(e)
        }
        None => {
            let half = Vec2::new(0.5, 0.6);
            let pos = Vec2::new(x, y + half.y + SKIN);
            Some(
                world
                    .spawn((
                        RoomEntity,
                        SpawnTag(tag),
                        SimPos(pos),
                        PrevPos(pos),
                        Velocity::default(),
                        Aabb { half },
                        Hurtbox {
                            half,
                            team: Team::Enemy,
                        },
                        crate::combat::Health::full(40),
                        Pogoable,
                    ))
                    .id(),
            )
        }
    }
}

fn spawn_spikes(world: &mut World, grid: &TileGrid) {
    // Merge horizontal runs of `^` into one hazard each.
    for j in 0..grid.height() {
        let mut i = 0;
        while i < grid.width() {
            if grid.get(i, j) != Tile::Spike {
                i += 1;
                continue;
            }
            let start = i;
            while i < grid.width() && grid.get(i, j) == Tile::Spike {
                i += 1;
            }
            let w = (i - start) as f32;
            let half = Vec2::new(w * 0.5, 0.25);
            let pos = Vec2::new(start as f32 + w * 0.5, j as f32 + 0.25);
            let e = world.spawn_empty().id();
            world.entity_mut(e).insert((
                RoomEntity,
                SimPos(pos),
                PrevPos(pos),
                Hitbox {
                    half,
                    team: Team::Hazard,
                    damage: 1,
                    kind: HitKind::Hazard,
                    attack_dir: AttackDir::Forward,
                    once: false,
                    owner: e,
                },
                Hurtbox {
                    half,
                    team: Team::Hazard,
                },
                Pogoable,
            ));
        }
    }
}

/// Replaces the current room with `id`, placing the player at `entry`.
pub fn enter_room(world: &mut World, id: &str, entry: &str) -> Result<(), String> {
    let def = world
        .resource::<RoomLibrary>()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown room `{id}`"))?;
    let entry_def = def
        .entry(entry)
        .cloned()
        .ok_or_else(|| format!("room `{id}` has no entry `{entry}`"))?;

    // Tear down the old room and anything in flight.
    let old: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<RoomEntity>, With<Projectile>)>>()
        .iter(world)
        .collect();
    for e in old {
        world.despawn(e);
    }
    // Loose slash boxes follow their (surviving) owner; cancel any swing.
    let hitboxes: Vec<Entity> = world
        .query_filtered::<Entity, (With<crate::combat::HitboxFollow>, Without<RoomEntity>)>()
        .iter(world)
        .collect();
    for e in hitboxes {
        world.despawn(e);
    }

    let grid = def.grid();
    spawn_spikes(world, &grid);
    world.insert_resource(grid);

    let defeated = world.resource::<WorldFlags>().defeated.clone();
    for i in 0..def.spawns.len() {
        if def.spawns[i].persistent && defeated.contains(&def.spawn_tag(i)) {
            continue;
        }
        spawn_from_def(world, &def, i);
    }
    for x in &def.exits {
        let (rx, ry, rw, rh) = x.rect;
        let half = Vec2::new(rw * 0.5, rh * 0.5);
        let pos = Vec2::new(rx + half.x, ry + half.y);
        world.spawn((
            RoomEntity,
            SimPos(pos),
            PrevPos(pos),
            RoomExit {
                half,
                to: x.to.clone(),
                entry: x.entry.clone(),
            },
        ));
    }
    for b in &def.benches {
        let half = Vec2::new(0.6, 0.6);
        let pos = Vec2::new(b.at.0, b.at.1 + half.y);
        world.spawn((RoomEntity, SimPos(pos), PrevPos(pos), Bench { half }));
    }
    for p in &def.pickups {
        let half = Vec2::new(0.5, 0.5);
        let pos = Vec2::new(p.at.0, p.at.1 + half.y);
        world.spawn((
            RoomEntity,
            SimPos(pos),
            PrevPos(pos),
            Pickup {
                ability: p.ability,
                half,
            },
        ));
    }

    // Place the player.
    let player = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .next();
    if let Some(p) = player {
        let half_y = world.get::<Aabb>(p).map_or(0.75, |a| a.half.y);
        let pos = Vec2::new(entry_def.at.0, entry_def.at.1 + half_y + SKIN);
        if let Some(mut c) = world.get_mut::<CombatState>(p) {
            c.attack = None;
            c.focusing = false;
            c.focus_ticks = 0;
        }
        world.entity_mut(p).insert((SimPos(pos), PrevPos(pos)));
        if let Some(mut v) = world.get_mut::<Velocity>(p) {
            v.x = 0.0;
        }
        if let Some(mut m) = world.get_mut::<Motor>(p) {
            m.dash_ticks_left = 0;
            m.wall_lock = 0;
        }
        if let Some(mut f) = world.get_mut::<Facing>(p) {
            f.0 = entry_def.facing;
        }
        if let Some(mut s) = world.get_mut::<SafeGround>(p) {
            s.pos = pos;
            s.stable_ticks = 0;
        }
    }
    {
        let mut input = world.resource_mut::<InputState>();
        for a in [Action::Jump, Action::Attack, Action::Dash, Action::Cast] {
            input.clear_press(a);
        }
    }
    world.resource_mut::<CurrentRoom>().id = def.id.clone();
    world
        .resource_mut::<Messages<RoomEntered>>()
        .write(RoomEntered { id: def.id });
    Ok(())
}

// ------------------------------------------------------------ transition --

/// Ticks of fade-out (and again of fade-in) around a room swap.
pub const FADE_TICKS: u32 = 18;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// Fading to black; gameplay frozen.
    Out,
    /// Fading back in; gameplay frozen.
    In,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct Transition {
    pub phase: Phase,
    pub ticks: u32,
    pub to: String,
    pub entry: String,
}

impl Transition {
    /// 0.0 = fully visible, 1.0 = fully black.
    pub fn fade(&self) -> f32 {
        let f = FADE_TICKS as f32;
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Out => 1.0 - self.ticks as f32 / f,
            Phase::In => self.ticks as f32 / f,
        }
    }

    pub fn active(&self) -> bool {
        self.phase != Phase::Idle
    }
}

/// Player touching an exit starts the transition.
pub fn detect_exits(
    lock: Res<crate::boss::ArenaLock>,
    mut tr: ResMut<Transition>,
    players: Query<(&SimPos, &Aabb), With<Player>>,
    exits: Query<(&SimPos, &RoomExit)>,
) {
    // Exits are sealed while a boss fight is on.
    if tr.active() || lock.0 {
        return;
    }
    for (p, pa) in &players {
        for (xp, x) in &exits {
            let d = (p.0 - xp.0).abs();
            if d.x < pa.half.x + x.half.x && d.y < pa.half.y + x.half.y {
                tr.phase = Phase::Out;
                tr.ticks = FADE_TICKS;
                tr.to = x.to.clone();
                tr.entry = x.entry.clone();
                return;
            }
        }
    }
}

/// Advances the fade; performs the swap at full black. Runs in `SimSet::Input`.
pub fn run_transition(world: &mut World) {
    let (phase, ticks) = {
        let t = world.resource::<Transition>();
        (t.phase, t.ticks)
    };
    match phase {
        Phase::Idle => {}
        Phase::Out => {
            let left = ticks.saturating_sub(1);
            if left == 0 {
                let (to, entry) = {
                    let t = world.resource::<Transition>();
                    (t.to.clone(), t.entry.clone())
                };
                if let Err(e) = enter_room(world, &to, &entry) {
                    // Never strand the player in a black screen.
                    eprintln!("room transition failed: {e}");
                }
                let mut t = world.resource_mut::<Transition>();
                t.phase = Phase::In;
                t.ticks = FADE_TICKS;
            } else {
                world.resource_mut::<Transition>().ticks = left;
            }
        }
        Phase::In => {
            let left = ticks.saturating_sub(1);
            let mut t = world.resource_mut::<Transition>();
            t.ticks = left;
            if left == 0 {
                t.phase = Phase::Idle;
            }
        }
    }
}
