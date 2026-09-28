//! Reachability analysis: where can the player actually get to?
//!
//! Instead of a hand-written model of jumps, this runs the **real player
//! controller** ([`player_movement`]) on a room's tile grid, executing a set of
//! scripted "moves" (walk, jump at several heights, edge jumps, dashes, wall
//! climbs, drop-throughs) from every standing spot it finds, breadth first.
//! Whatever those moves can reach, a player can reach; if the moves can't
//! reach a place, a level designer should assume the player can't either.
//!
//! On top of that, [`analyse_world`] links rooms through their exits and hands
//! out abilities (pickups and boss rewards) stage by stage, which proves that
//! the map can be completed in the intended order and that ability gates really
//! are gates.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;
use bevy_math::Vec2;

use crate::combat::CombatState;
use crate::components::{SimPos, Velocity};
use crate::input::{apply_bits, bit, Action, InputState};
use crate::player::{player_movement, Abilities, Motor, PlayerBundle};
use crate::tuning::Tuning;
use crate::world::grid::{Tile, TileGrid, SKIN};
use crate::world::progress::boss_reward;
use crate::world::room::{Ability, RoomDef, RoomLibrary, SpawnKind};
use crate::SimTick;

// ------------------------------------------------------------------ mover --

/// The player controller and nothing else: a tiny world that runs only
/// [`player_movement`], so millions of ticks cost seconds.
pub struct Mover {
    world: World,
    schedule: Schedule,
    player: Entity,
    half: Vec2,
    grid: TileGrid,
}

impl Mover {
    pub fn new(grid: TileGrid, tuning: &Tuning, abilities: Abilities) -> Self {
        let mut world = World::new();
        world.insert_resource(grid.clone());
        world.insert_resource(tuning.clone());
        world.insert_resource(InputState::default());
        world.insert_resource(SimTick::default());
        let bundle = PlayerBundle::new(Vec2::ZERO, tuning, abilities);
        let half = bundle.aabb.half;
        let player = world.spawn(bundle).id();
        let mut schedule = Schedule::default();
        // One tiny system per run: the multi-threaded executor (which other
        // crates in the workspace switch on) costs far more than the work.
        schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        schedule.add_systems(player_movement);
        Self {
            world,
            schedule,
            player,
            half,
            grid,
        }
    }

    /// Stands the player (feet at `feet`) at rest on the ground, all timers
    /// fresh, no buttons held.
    pub fn place(&mut self, feet: Vec2) {
        let ticks = self.world.resource::<Tuning>().player.coyote_ticks();
        let pos = Vec2::new(feet.x, feet.y + self.half.y + SKIN);
        let mut e = self.world.entity_mut(self.player);
        e.remove::<crate::combat::Invulnerable>();
        e.insert((
            SimPos(pos),
            Velocity(Vec2::ZERO),
            CombatState::default(),
            Motor {
                grounded: true,
                coyote: ticks,
                air_dash_ready: true,
                ..Motor::default()
            },
            crate::player::Facing(1),
        ));
        *self.world.resource_mut::<InputState>() = InputState::default();
    }

    /// One tick with `bits` held (see [`bit`]).
    pub fn step(&mut self, bits: u16) {
        let next = self.world.resource::<SimTick>().0;
        let mut input = std::mem::take(&mut *self.world.resource_mut::<InputState>());
        apply_bits(&mut input, bits, &SimTick(next));
        *self.world.resource_mut::<InputState>() = input;
        self.world.resource_mut::<SimTick>().0 = next + 1;
        self.schedule.run(&mut self.world);
    }

    pub fn center(&self) -> Vec2 {
        self.world.get::<SimPos>(self.player).unwrap().0
    }

    pub fn feet(&self) -> Vec2 {
        self.center() - Vec2::new(0.0, self.half.y + SKIN)
    }

    pub fn vel(&self) -> Vec2 {
        self.world.get::<Velocity>(self.player).unwrap().0
    }

    pub fn motor(&self) -> &Motor {
        self.world.get::<Motor>(self.player).unwrap()
    }

    pub fn half(&self) -> Vec2 {
        self.half
    }

    /// Standing on something solid, not moving vertically.
    pub fn standing(&self) -> bool {
        self.motor().grounded && self.vel().y <= 0.0
    }

    /// Overlapping a spike tile (they occupy the lower half of their tile).
    pub fn on_spikes(&self) -> bool {
        let c = self.center();
        let (lo, hi) = (c - self.half, c + self.half);
        for j in (lo.y.floor() as i32)..=(hi.y.floor() as i32) {
            for i in (lo.x.floor() as i32)..=(hi.x.floor() as i32) {
                if self.grid.get(i, j) == Tile::Spike {
                    let (sx0, sx1) = (i as f32, i as f32 + 1.0);
                    let (sy0, sy1) = (j as f32, j as f32 + 0.5);
                    if lo.x < sx1 && hi.x > sx0 && lo.y < sy1 && hi.y > sy0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Does the player's body overlap the rectangle `(x, y, w, h)`?
    pub fn touches(&self, rect: (f32, f32, f32, f32)) -> bool {
        let c = self.center();
        let (rx, ry, rw, rh) = rect;
        let rc = Vec2::new(rx + rw * 0.5, ry + rh * 0.5);
        let d = (c - rc).abs();
        d.x < self.half.x + rw * 0.5 && d.y < self.half.y + rh * 0.5
    }
}

// ------------------------------------------------------------------ moves --

/// Ticks to try before giving up on a move (climbing a tall wall takes a while).
const MAX_TICKS: u32 = 260;
const MAX_TICKS_CLIMB: u32 = 1100;
/// How long the Jump button stays down after a wall jump (a full-height hop).
const WALL_HOLD: i64 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assist {
    None,
    /// After each wall jump, steer back into the same wall (zig-zag up it).
    SameWall,
    /// After each wall jump, steer across to the opposite wall (climb a shaft).
    CrossShaft,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    /// Just walk (falls off ledges, records where it stood).
    Walk,
    /// Jump straight away.
    Jump,
    /// Run to the edge and jump the moment the ground ends (coyote time).
    EdgeJump,
    /// Down + Jump through a one-way platform.
    Drop,
}

#[derive(Clone, Copy, Debug)]
struct Move {
    kind: Kind,
    /// -1, 0 or +1.
    dir: i8,
    /// Ticks the Jump button stays down (short = low hop, long = full jump).
    hold: u32,
    /// Press Dash this many ticks after the jump (None = never).
    dash_at: Option<u32>,
    assist: Assist,
    /// Stop steering after this many ticks in the air (drop straight down).
    steer_for: Option<u32>,
}

fn moves(abilities: Abilities) -> Vec<Move> {
    let mut v = Vec::new();
    for dir in [-1, 1] {
        v.push(Move {
            kind: Kind::Walk,
            dir,
            hold: 0,
            dash_at: None,
            assist: Assist::None,
            steer_for: None,
        });
        // A ground dash along the floor.
        if abilities.dash {
            v.push(Move {
                kind: Kind::Walk,
                dir,
                hold: 0,
                dash_at: Some(0),
                assist: Assist::None,
                steer_for: None,
            });
        }
    }
    let dashes: Vec<Option<u32>> = if abilities.dash {
        vec![None, Some(4), Some(14), Some(26), Some(40)]
    } else {
        vec![None]
    };
    let assists: Vec<Assist> = if abilities.wall_grip {
        vec![Assist::None, Assist::SameWall, Assist::CrossShaft]
    } else {
        vec![Assist::None]
    };
    for kind in [Kind::Jump, Kind::EdgeJump] {
        for dir in [-1, 0, 1] {
            if dir == 0 && matches!(kind, Kind::EdgeJump) {
                continue;
            }
            for hold in [10, 22, 40, 70] {
                for &dash_at in &dashes {
                    for &assist in &assists {
                        if dir == 0 && assist != Assist::None {
                            continue;
                        }
                        if assist != Assist::None && (hold < 40 || dash_at.is_some()) {
                            continue; // wall climbing wants a full jump and no dash
                        }
                        v.push(Move {
                            kind,
                            dir,
                            hold,
                            dash_at,
                            assist,
                            steer_for: None,
                        });
                    }
                }
            }
        }
    }
    // Steer for a moment, then let go: lands close to the take-off spot.
    for dir in [-1, 1] {
        for hold in [22, 70] {
            for steer in [12, 30] {
                v.push(Move {
                    kind: Kind::Jump,
                    dir,
                    hold,
                    dash_at: None,
                    assist: Assist::None,
                    steer_for: Some(steer),
                });
            }
        }
    }
    for dir in [-1, 0, 1] {
        v.push(Move {
            kind: Kind::Drop,
            dir,
            hold: 0,
            dash_at: None,
            assist: Assist::None,
            steer_for: None,
        });
    }
    v
}

/// What one move did.
#[derive(Default)]
struct Outcome {
    /// Feet positions where the player stood (walking) or came down.
    stood: Vec<Vec2>,
    exits: Vec<usize>,
    pickups: Vec<usize>,
    /// Ended by hitting spikes or falling out of the world.
    failed: bool,
}

fn dir_bit(dir: i8) -> u16 {
    match dir {
        -1 => bit(Action::Left),
        1 => bit(Action::Right),
        _ => 0,
    }
}

struct RunCtx<'a> {
    exits: &'a [(f32, f32, f32, f32)],
    pickups: &'a [(f32, f32, f32, f32)],
    /// Falling below this, or wandering further than this outside the room's
    /// width, counts as leaving the world.
    floor_y: f32,
    width: f32,
}

/// Executes `mv` from `start` (feet) and reports where it went.
fn run_move(m: &mut Mover, ctx: &RunCtx, start: Vec2, mv: Move) -> Outcome {
    m.place(start);
    let mut out = Outcome::default();
    let mut jumped_at: Option<u32> = None;
    let mut left_ground = false;
    let mut jump_down = false;
    let mut want_press = false;
    let mut last_wall_jump: i64 = -100;
    let mut wall_dir: i8 = 0;
    let mut dashed = false;
    let mut prev_grounded = true;

    let limit = if mv.assist == Assist::None {
        MAX_TICKS
    } else {
        MAX_TICKS_CLIMB
    };
    for t in 0..limit {
        let grounded = m.motor().grounded;
        let wall = m.motor().wall;
        let mut bits = 0u16;

        // --- when does the jump start? ---
        match mv.kind {
            Kind::Walk => {}
            Kind::Jump | Kind::Drop => {
                if jumped_at.is_none() && t >= 1 {
                    jumped_at = Some(t);
                }
            }
            Kind::EdgeJump => {
                if jumped_at.is_none() && t >= 1 && !grounded && prev_grounded {
                    jumped_at = Some(t);
                }
            }
        }
        let since_jump = jumped_at.map(|j| t - j);

        // --- steering ---
        let mut steer = mv.dir;
        if let (Some(sf), Some(sj)) = (mv.steer_for, since_jump) {
            if sj >= sf {
                steer = 0;
            }
        }
        // --- the jump button ---
        let mut jump = false;
        if let Some(sj) = since_jump {
            jump = sj < mv.hold.max(1);
            if matches!(mv.kind, Kind::Drop) {
                jump = sj < 3;
                bits |= bit(Action::Down);
            }
        }
        // --- wall assistance ---
        if mv.assist != Assist::None && !grounded && wall != 0 && (t as i64 - last_wall_jump) > 8 {
            want_press = true;
            wall_dir = wall;
        }
        if want_press {
            if jump_down {
                jump = false; // release first so the next tick is a fresh press
            } else {
                jump = true;
                want_press = false;
                last_wall_jump = t as i64;
            }
        } else if mv.assist != Assist::None
            && last_wall_jump >= 0
            && (t as i64 - last_wall_jump) < WALL_HOLD
        {
            // Keep the button down for a full-height wall jump, and steer.
            jump = true;
        }
        if mv.assist != Assist::None
            && last_wall_jump >= 0
            && (t as i64 - last_wall_jump) < WALL_HOLD
        {
            steer = match mv.assist {
                Assist::SameWall => wall_dir,
                _ => -wall_dir,
            };
        }
        if jump {
            bits |= bit(Action::Jump);
        }
        bits |= dir_bit(steer);
        // --- dash ---
        if let Some(d) = mv.dash_at {
            let ready = match (mv.kind, since_jump) {
                (Kind::Walk, _) => t == d,
                (_, Some(sj)) => sj == d,
                _ => false,
            };
            if ready && !dashed {
                bits |= bit(Action::Dash);
                dashed = true;
                if mv.dir == 0 {
                    bits |= bit(Action::Right);
                }
            }
        }
        jump_down = bits & bit(Action::Jump) != 0;

        m.step(bits);
        prev_grounded = grounded;

        // --- bookkeeping ---
        for (i, r) in ctx.exits.iter().enumerate() {
            if m.touches(*r) && !out.exits.contains(&i) {
                out.exits.push(i);
            }
        }
        for (i, r) in ctx.pickups.iter().enumerate() {
            if m.touches(*r) && !out.pickups.contains(&i) {
                out.pickups.push(i);
            }
        }
        let c = m.center();
        if m.on_spikes() || c.y < ctx.floor_y || c.x < -3.0 || c.x > ctx.width + 3.0 {
            out.failed = true;
            return out;
        }
        if !m.motor().grounded {
            left_ground = true;
        }
        if m.standing() {
            out.stood.push(m.feet());
            // A jump only ends by coming down again.
            let started = jumped_at.is_some_and(|j| t > j + 1);
            match mv.kind {
                Kind::Walk => {
                    if left_ground {
                        return out;
                    }
                }
                _ => {
                    if started && left_ground {
                        return out;
                    }
                }
            }
        }
        // A walk ends when it has covered its ground; an edge jump that has
        // found no edge nearby is pointless (the walk's own steps are nodes,
        // so a nearer start will find that edge).
        if t >= 150 && matches!(mv.kind, Kind::Walk) && m.standing() {
            return out;
        }
        if t >= 70 && matches!(mv.kind, Kind::EdgeJump) && jumped_at.is_none() {
            return out;
        }
    }
    // Timed out in the air (wedged, or an endless fall): not a place to stand.
    out
}

// ------------------------------------------------------------- room reach --

/// A standing spot found by the search.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub feet: Vec2,
}

/// Everything reachable inside one room from one starting point.
#[derive(Clone, Debug, Default)]
pub struct RoomReach {
    pub nodes: Vec<Node>,
    /// Which of the room's exits (by index) can be touched.
    pub exits: Vec<bool>,
    pub pickups: Vec<bool>,
    pub benches: Vec<bool>,
    /// Standing spots from which no exit can ever be reached again: a
    /// softlock. (Only the first few are kept.)
    pub traps: Vec<Vec2>,
    /// How many traps there are in total.
    pub trap_count: usize,
}

impl RoomReach {
    pub fn can_stand_near(&self, p: Vec2, radius: f32) -> bool {
        self.nodes.iter().any(|n| (n.feet - p).length() <= radius)
    }
}

fn key(feet: Vec2) -> (i32, i32) {
    ((feet.x * 2.0).round() as i32, (feet.y * 4.0).round() as i32)
}

/// Runs every move in `moves` from every start, splitting the starts across the
/// movers in `pool` (one worker thread each). The result is indexed like
/// `starts`. Movers are reused between calls: building one is the slow part.
fn expand(starts: &[Vec2], pool: &mut [Mover], ctx: &RunCtx, moves: &[Move]) -> Vec<Vec<Outcome>> {
    fn work(m: &mut Mover, part: &[Vec2], ctx: &RunCtx, moves: &[Move]) -> Vec<Vec<Outcome>> {
        part.iter()
            .map(|s| moves.iter().map(|mv| run_move(m, ctx, *s, *mv)).collect())
            .collect()
    }
    if pool.len() <= 1 || starts.len() < 4 {
        return work(&mut pool[0], starts, ctx, moves);
    }
    let chunk = starts.len().div_ceil(pool.len());
    std::thread::scope(|scope| {
        let handles: Vec<_> = starts
            .chunks(chunk)
            .zip(pool.iter_mut())
            .map(|(part, m)| scope.spawn(move || work(m, part, ctx, moves)))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("reach worker panicked"))
            .collect()
    })
}

/// Explores `def` (its geometry, exits, pickups and benches; enemies are
/// ignored) from the player standing at `start`.
pub fn analyse_room(
    def: &RoomDef,
    tuning: &Tuning,
    abilities: Abilities,
    start: Vec2,
) -> RoomReach {
    let exits: Vec<(f32, f32, f32, f32)> = def.exits.iter().map(|e| e.rect).collect();
    let pickups: Vec<(f32, f32, f32, f32)> = def
        .pickups
        .iter()
        .map(|p| (p.at.0 - 0.5, p.at.1, 1.0, 1.0))
        .collect();
    let benches: Vec<(f32, f32, f32, f32)> = def
        .benches
        .iter()
        .map(|b| (b.at.0 - 0.6, b.at.1, 1.2, 1.2))
        .collect();
    let ctx = RunCtx {
        exits: &exits,
        pickups: &pickups,
        floor_y: -6.0,
        width: def.width() as f32,
    };
    let all_moves = moves(abilities);

    let mut reach = RoomReach {
        exits: vec![false; exits.len()],
        pickups: vec![false; pickups.len()],
        benches: vec![false; benches.len()],
        ..Default::default()
    };
    let mut index: HashMap<(i32, i32), usize> = HashMap::new();
    // Per node: which exits its moves touched; and which nodes they reached.
    let mut exit_from: Vec<bool> = Vec::new();
    let mut edges: Vec<Vec<usize>> = Vec::new();

    let mut add = |feet: Vec2,
                   reach: &mut RoomReach,
                   next: &mut Vec<usize>,
                   exit_from: &mut Vec<bool>,
                   edges: &mut Vec<Vec<usize>>|
     -> usize {
        let k = key(feet);
        if let Some(&i) = index.get(&k) {
            return i;
        }
        let i = reach.nodes.len();
        index.insert(k, i);
        reach.nodes.push(Node { feet });
        exit_from.push(false);
        edges.push(Vec::new());
        next.push(i);
        i
    };

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let grid = def.grid();
    let mut pool: Vec<Mover> = (0..threads)
        .map(|_| Mover::new(grid.clone(), tuning, abilities))
        .collect();
    let mut frontier: Vec<usize> = Vec::new();
    add(start, &mut reach, &mut frontier, &mut exit_from, &mut edges);
    const NODE_CAP: usize = 4000;
    // Breadth first, one layer at a time. The moves from a node don't depend
    // on anything else, so a layer is expanded in parallel; the results are
    // then merged in a fixed order, which keeps the analysis deterministic.
    while !frontier.is_empty() {
        let starts: Vec<Vec2> = frontier.iter().map(|&i| reach.nodes[i].feet).collect();
        let outcomes = expand(&starts, &mut pool, &ctx, &all_moves);
        let mut next: Vec<usize> = Vec::new();
        for (k, &i) in frontier.iter().enumerate() {
            for out in &outcomes[k] {
                for &e in &out.exits {
                    reach.exits[e] = true;
                    exit_from[i] = true;
                }
                for &p in &out.pickups {
                    reach.pickups[p] = true;
                }
                for (b, r) in benches.iter().enumerate() {
                    // Standing anywhere on the bench counts.
                    if out.stood.iter().any(|f| {
                        f.x > r.0 - 0.4 && f.x < r.0 + r.2 + 0.4 && (f.y - r.1).abs() < 0.3
                    }) {
                        reach.benches[b] = true;
                    }
                }
                if out.failed {
                    continue;
                }
                for &f in &out.stood {
                    if reach.nodes.len() >= NODE_CAP {
                        break;
                    }
                    let j = add(f, &mut reach, &mut next, &mut exit_from, &mut edges);
                    if j != i && !edges[i].contains(&j) {
                        edges[i].push(j);
                    }
                }
            }
        }
        frontier = next;
    }

    // Softlocks: nodes that can never get back to an exit.
    let n = reach.nodes.len();
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, outs) in edges.iter().enumerate() {
        for &b in outs {
            rev[b].push(a);
        }
    }
    let mut ok = vec![false; n];
    let mut stack: Vec<usize> = (0..n).filter(|&i| exit_from[i]).collect();
    for &i in &stack {
        ok[i] = true;
    }
    while let Some(i) = stack.pop() {
        for &a in &rev[i] {
            if !ok[a] {
                ok[a] = true;
                stack.push(a);
            }
        }
    }
    for (i, good) in ok.iter().enumerate() {
        if !good {
            reach.trap_count += 1;
            if reach.traps.len() < 8 {
                reach.traps.push(reach.nodes[i].feet);
            }
        }
    }
    reach
}

// ------------------------------------------------------------ world reach --

/// One step of the intended progression.
#[derive(Clone, Debug)]
pub struct Stage {
    /// What the player can do at this stage.
    pub abilities: Abilities,
    /// Rooms that first became reachable at this stage.
    pub new_rooms: Vec<String>,
    /// Abilities collected during it (which starts the next stage).
    pub gained: Vec<Ability>,
}

#[derive(Clone, Debug, Default)]
pub struct WorldReport {
    pub stages: Vec<Stage>,
    /// Rooms never reached, ignoring any whose id is in `ignore`.
    pub unreachable: Vec<String>,
    /// (room, stage it was first reachable in).
    pub first_stage: HashMap<String, usize>,
    /// Softlock spots: (room, feet).
    pub traps: Vec<(String, Vec2)>,
    /// Exits that lead somewhere but can't be touched even with everything.
    pub dead_exits: Vec<(String, usize)>,
    /// Pickups that can never be collected.
    pub dead_pickups: Vec<(String, usize)>,
    /// Benches that can never be sat on.
    pub dead_benches: Vec<(String, usize)>,
}

impl WorldReport {
    pub fn stage_of(&self, room: &str) -> Option<usize> {
        self.first_stage.get(room).copied()
    }

    pub fn is_clean(&self) -> bool {
        self.unreachable.is_empty()
            && self.traps.is_empty()
            && self.dead_exits.is_empty()
            && self.dead_pickups.is_empty()
            && self.dead_benches.is_empty()
    }
}

/// Plays the map out on paper: start at `(room, entry)` with nothing, follow
/// every exit that can be reached, collect what can be collected, and repeat
/// with the new abilities until nothing changes.
pub fn analyse_world(
    library: &RoomLibrary,
    tuning: &Tuning,
    start: (&str, &str),
    ignore: &[&str],
) -> Result<WorldReport, String> {
    let mut report = WorldReport::default();
    let mut abilities = Abilities::default();
    let mut cache: HashMap<(String, String, bool, bool), RoomReach> = HashMap::new();
    let mut reached: HashSet<String> = HashSet::new();
    // What the final, everything-unlocked pass says about every room.
    let mut last_reach: HashMap<(String, String), RoomReach> = HashMap::new();

    loop {
        let stage = report.stages.len();
        let mut new_rooms = Vec::new();
        let mut gained: Vec<Ability> = Vec::new();
        let mut visited: HashSet<(String, String)> = HashSet::new();
        let mut queue: VecDeque<(String, String)> = VecDeque::new();
        queue.push_back((start.0.to_string(), start.1.to_string()));
        last_reach.clear();

        while let Some((room, entry)) = queue.pop_front() {
            if !visited.insert((room.clone(), entry.clone())) {
                continue;
            }
            let def = library
                .get(&room)
                .ok_or_else(|| format!("unknown room `{room}`"))?;
            let e = def
                .entry(&entry)
                .ok_or_else(|| format!("room `{room}` has no entry `{entry}`"))?;
            if reached.insert(room.clone()) {
                new_rooms.push(room.clone());
                report.first_stage.insert(room.clone(), stage);
            }
            let k = (
                room.clone(),
                entry.clone(),
                abilities.dash,
                abilities.wall_grip,
            );
            let reach = cache
                .entry(k)
                .or_insert_with(|| analyse_room(def, tuning, abilities, Vec2::new(e.at.0, e.at.1)))
                .clone();
            for (i, x) in def.exits.iter().enumerate() {
                if reach.exits[i] {
                    queue.push_back((x.to.clone(), x.entry.clone()));
                }
            }
            for (i, p) in def.pickups.iter().enumerate() {
                if reach.pickups[i] && !abilities.has(p.ability) && !gained.contains(&p.ability) {
                    gained.push(p.ability);
                }
            }
            // A boss you can walk up to is a boss you can beat (the bot tests
            // prove the fights themselves are winnable).
            for s in &def.spawns {
                let id = match s.kind {
                    SpawnKind::Matron => "matron",
                    SpawnKind::Bellwarden => "bellwarden",
                    _ => continue,
                };
                if let Some(a) = boss_reward(id) {
                    if !abilities.has(a)
                        && !gained.contains(&a)
                        && reach.can_stand_near(Vec2::new(s.at.0, s.at.1), 12.0)
                    {
                        gained.push(a);
                    }
                }
            }
            last_reach.insert((room, entry), reach);
        }
        report.stages.push(Stage {
            abilities,
            new_rooms,
            gained: gained.clone(),
        });
        if gained.is_empty() {
            break;
        }
        for a in gained {
            abilities.grant(a);
        }
    }

    // Anything the library has that we never got to.
    for id in library.ids() {
        if !reached.contains(id) && !ignore.contains(&id) {
            report.unreachable.push(id.to_string());
        }
    }
    // Final-state checks (everything unlocked): softlocks and dead props.
    let mut exit_seen: HashMap<(String, usize), bool> = HashMap::new();
    let mut pickup_seen: HashMap<(String, usize), bool> = HashMap::new();
    let mut bench_seen: HashMap<(String, usize), bool> = HashMap::new();
    for ((room, _entry), reach) in &last_reach {
        for (i, ok) in reach.exits.iter().enumerate() {
            *exit_seen.entry((room.clone(), i)).or_default() |= *ok;
        }
        for (i, ok) in reach.pickups.iter().enumerate() {
            *pickup_seen.entry((room.clone(), i)).or_default() |= *ok;
        }
        for (i, ok) in reach.benches.iter().enumerate() {
            *bench_seen.entry((room.clone(), i)).or_default() |= *ok;
        }
        for t in &reach.traps {
            report.traps.push((room.clone(), *t));
        }
    }
    let dead = |seen: &HashMap<(String, usize), bool>| -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = seen
            .iter()
            .filter(|(_, ok)| !**ok)
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    };
    report.dead_exits = dead(&exit_seen);
    report.dead_pickups = dead(&pickup_seen);
    report.dead_benches = dead(&bench_seen);
    report.traps.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.x.total_cmp(&b.1.x))
            .then(a.1.y.total_cmp(&b.1.y))
    });
    report.traps.dedup();
    Ok(report)
}
