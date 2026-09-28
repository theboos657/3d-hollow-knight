//! Boss state machine, attack selection, and the entities attacks create.

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use super::*;
use crate::combat::{
    AlreadyHit, CombatState, EnemyDied, HitStop, HitboxFollow, Invulnerable, Lifetime, Projectile,
};
use crate::ms_to_ticks;
use crate::player::Player;
use crate::rng::SimRng;
use crate::tuning::{AttackDef, AttackKind, BossDef, Tuning};
use crate::world::grid::{probe, Side, TileGrid};
use crate::world::room::WorldFlags;

/// Boss body gravity (matches the shared enemy gravity).
const GRAVITY: f32 = 60.0;
/// Bells fall this fast.
const BELL_SPEED: f32 = 28.0;
/// Strongest horizontal speed of a slam leap.
const MAX_LEAP_VX: f32 = 14.0;
/// Melee arcs linger this many ticks.
const ARC_TICKS: u32 = 12;

fn sign(v: f32, fallback: i8) -> i8 {
    if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        fallback
    }
}

/// Weighted pick among the attacks available in this phase (and range).
/// Never picks the same attack a third time in a row while there is a choice.
pub fn pick_attack(
    def: &BossDef,
    b: &BossBrain,
    dist: f32,
    pendulums_alive: bool,
    in_range_only: bool,
    rng: &mut SimRng,
) -> Option<usize> {
    let gather = |respect_repeat: bool| -> Vec<(usize, f32)> {
        def.attacks
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                b.phase >= a.min_phase
                    && b.phase <= a.max_phase
                    && (!in_range_only || (a.min_range..=a.max_range).contains(&dist))
                    && !(respect_repeat && b.history == [Some(*i), Some(*i)])
                    && !(pendulums_alive && matches!(a.kind, AttackKind::Pendulums { .. }))
            })
            .map(|(i, a)| (i, a.weight))
            .collect()
    };
    let mut cands = gather(true);
    if cands.is_empty() {
        cands = gather(false);
    }
    let total: f32 = cands.iter().map(|c| c.1).sum();
    if cands.is_empty() || total <= 0.0 {
        return None;
    }
    let mut r = rng.f32() * total;
    for (i, w) in &cands {
        if r < *w {
            return Some(*i);
        }
        r -= *w;
    }
    cands.last().map(|c| c.0)
}

fn spawn_shockwave(
    commands: &mut Commands,
    boss: Entity,
    origin: Vec2,
    dir: i8,
    speed: f32,
    life_ticks: u32,
    damage: i32,
) {
    let half = Vec2::new(0.5, 0.6);
    let pos = Vec2::new(origin.x + dir as f32 * 0.3, origin.y + half.y + 0.01);
    commands.spawn((
        BossSpawn,
        RoomEntity,
        Projectile,
        Hitbox {
            half,
            team: Team::Enemy,
            damage,
            kind: HitKind::Projectile,
            attack_dir: AttackDir::Forward,
            once: false,
            owner: boss,
        },
        AlreadyHit::default(),
        SimPos(pos),
        PrevPos(pos),
        Velocity(Vec2::new(dir as f32 * speed, 0.0)),
        Lifetime(life_ticks),
    ));
}

fn spawn_arc(
    commands: &mut Commands,
    boss: Entity,
    boss_half: Vec2,
    pos: Vec2,
    facing: i8,
    reach: (f32, f32),
    damage: i32,
) {
    let half = Vec2::new(reach.0 * 0.5, reach.1);
    let rel = Vec2::new(facing as f32 * (boss_half.x + half.x), 0.0);
    let p = pos + rel;
    commands.spawn((
        BossSpawn,
        RoomEntity,
        Hitbox {
            half,
            team: Team::Enemy,
            damage,
            kind: HitKind::Contact,
            attack_dir: AttackDir::Forward,
            once: true,
            owner: boss,
        },
        AlreadyHit::default(),
        HitboxFollow { owner: boss, rel },
        SimPos(p),
        PrevPos(p),
        Lifetime(ARC_TICKS),
    ));
}

/// Runs every boss. Boss fights are deterministic given the seed in `SimRng`.
pub fn boss_ai(
    mut commands: Commands,
    tuning: Res<Tuning>,
    grid: Res<TileGrid>,
    mut rng: ResMut<SimRng>,
    mut hitstop: ResMut<HitStop>,
    mut awoke: MessageWriter<BossAwoke>,
    mut phase_msg: MessageWriter<BossPhaseChanged>,
    mut defeated: MessageWriter<BossDefeated>,
    mut enemy_died: MessageWriter<EnemyDied>,
    players: Query<(&SimPos, &CombatState), With<Player>>,
    pendulums: Query<Entity, With<Pendulum>>,
    spawns: Query<Entity, With<BossSpawn>>,
    mut bosses: Query<(
        Entity,
        &Boss,
        &mut BossBrain,
        &mut Velocity,
        &SimPos,
        &Aabb,
        &Health,
        Option<&SpawnTag>,
    )>,
) {
    let target = players.iter().find(|(_, cs)| !cs.dead).map(|(p, _)| p.0);

    for (e, boss, mut b, mut vel, pos, aabb, health, tag) in &mut bosses {
        let Some(def) = tuning.bosses.get(&boss.id) else {
            continue;
        };
        let half = aabb.half;
        let feet = Vec2::new(pos.0.x, pos.0.y - half.y);
        let grounded = vel.y <= 0.0 && probe(&grid, pos.0, half, Side::Below, false);
        b.timer += 1;

        // Death overrides everything.
        if health.hp <= 0 && b.state != BossState::Dying {
            b.enter(BossState::Dying);
            b.attack = None;
            vel.0 = Vec2::ZERO;
            for s in &spawns {
                commands.entity(s).despawn();
            }
            for p in &pendulums {
                commands.entity(p).despawn();
            }
            hitstop.0 = hitstop.0.max(ms_to_ticks(500.0));
            commands
                .entity(e)
                .insert((Disarmed, Invulnerable(ms_to_ticks(def.death_ms) + 20)));
            defeated.write(BossDefeated {
                id: boss.id.clone(),
                tag: tag.map(|t| t.0),
            });
            continue;
        }

        let dist = target.map_or(f32::MAX, |t| (t.x - pos.0.x).abs());
        let pendulums_alive = !pendulums.is_empty();

        match b.state {
            BossState::Sleeping => {
                vel.x = 0.0;
                let (lo, hi) = b.arena;
                if let Some(t) = target {
                    if t.x > lo.x && t.x < hi.x && t.y > lo.y && t.y < hi.y {
                        b.facing = sign(t.x - pos.0.x, b.facing);
                        b.enter(BossState::Intro);
                        commands
                            .entity(e)
                            .insert(Invulnerable(ms_to_ticks(def.intro_ms) + 1));
                        awoke.write(BossAwoke {
                            id: boss.id.clone(),
                        });
                    }
                }
            }
            BossState::Intro => {
                vel.x = 0.0;
                if b.timer >= ms_to_ticks(def.intro_ms) {
                    commands.entity(e).remove::<Disarmed>();
                    b.enter(BossState::Choose);
                }
            }
            BossState::Choose => {
                vel.x = 0.0;
                let hp_frac = health.hp as f32 / health.max.max(1) as f32;
                if b.phase < def.phases() && hp_frac <= def.phase_thresholds[b.phase as usize - 1] {
                    b.phase += 1;
                    b.enter(BossState::Transition);
                    commands
                        .entity(e)
                        .insert((Disarmed, Invulnerable(ms_to_ticks(def.transition_ms) + 1)));
                    phase_msg.write(BossPhaseChanged {
                        id: boss.id.clone(),
                        phase: b.phase,
                    });
                    continue;
                }
                let Some(t) = target else {
                    continue;
                };
                b.facing = sign(t.x - pos.0.x, b.facing);
                match pick_attack(def, &b, dist, pendulums_alive, true, &mut rng) {
                    Some(i) => {
                        start_attack(&mut commands, e, def, &mut b, &mut vel, i, t, feet, half)
                    }
                    None => {
                        b.approach_timer = 0;
                        b.enter(BossState::Approach);
                    }
                }
            }
            BossState::Approach => {
                let Some(t) = target else {
                    vel.x = 0.0;
                    b.enter(BossState::Choose);
                    continue;
                };
                b.approach_timer += 1;
                b.facing = sign(t.x - pos.0.x, b.facing);
                let give_up = b.approach_timer >= ms_to_ticks(def.approach_ms);
                match pick_attack(def, &b, dist, pendulums_alive, !give_up, &mut rng) {
                    Some(i) => {
                        start_attack(&mut commands, e, def, &mut b, &mut vel, i, t, feet, half)
                    }
                    None => {
                        // Walk toward the player, but never into the arena wall.
                        let edge = if b.facing > 0 {
                            b.arena.1.x
                        } else {
                            b.arena.0.x
                        };
                        let at_wall = (pos.0.x + b.facing as f32 * half.x - edge).abs() < 0.3;
                        vel.x = if at_wall {
                            0.0
                        } else {
                            b.facing as f32 * def.walk_speed
                        };
                    }
                }
            }
            BossState::Telegraph => {
                vel.x = 0.0;
                let Some(i) = b.attack else {
                    b.enter(BossState::Choose);
                    continue;
                };
                let atk = &def.attacks[i];
                if b.timer >= ms_to_ticks(atk.telegraph_ms) {
                    b.enter(BossState::Active);
                }
            }
            BossState::Active => {
                let Some(i) = b.attack else {
                    b.enter(BossState::Choose);
                    continue;
                };
                let atk = &def.attacks[i];
                let active = ms_to_ticks(atk.active_ms);
                let done = active_step(
                    &mut commands,
                    e,
                    atk,
                    &mut b,
                    &mut vel,
                    pos.0,
                    half,
                    feet,
                    grounded,
                    &grid,
                    target,
                    &pendulums,
                    active,
                );
                if done {
                    let mult = def.recover_mult_for(b.phase);
                    let wall = match atk.kind {
                        AttackKind::Charge {
                            wall_recover_mult, ..
                        } if b.hit_wall => wall_recover_mult,
                        _ => 1.0,
                    };
                    b.recover_ticks = ms_to_ticks(atk.recover_ms * mult * wall);
                    vel.x = 0.0;
                    b.enter(BossState::Recover);
                }
            }
            BossState::Recover => {
                vel.x = 0.0;
                if b.timer >= b.recover_ticks {
                    b.attack = None;
                    b.enter(BossState::Choose);
                }
            }
            BossState::Transition => {
                vel.x = 0.0;
                if b.timer >= ms_to_ticks(def.transition_ms) {
                    commands.entity(e).remove::<Disarmed>();
                    b.enter(BossState::Choose);
                }
            }
            BossState::Dying => {
                vel.0 = Vec2::ZERO;
                if b.timer >= ms_to_ticks(def.death_ms) {
                    enemy_died.write(EnemyDied {
                        entity: e,
                        pos: pos.0,
                        tag: tag.map(|t| t.0),
                    });
                    commands.entity(e).despawn();
                }
            }
        }
        b.was_grounded = grounded;
    }
}

#[allow(clippy::too_many_arguments)]
fn start_attack(
    commands: &mut Commands,
    _boss: Entity,
    def: &BossDef,
    b: &mut BossBrain,
    vel: &mut Velocity,
    i: usize,
    target: Vec2,
    feet: Vec2,
    _half: Vec2,
) {
    let atk = &def.attacks[i];
    b.attack = Some(i);
    b.history = [b.history[1], Some(i)];
    b.aim = target;
    b.hit_wall = false;
    b.waves_done = 0;
    b.sub_timer = 0;
    b.second_tell = false;
    b.approach_timer = 0;
    vel.x = 0.0;
    b.enter(BossState::Telegraph);

    // Falling bells announce themselves with floor glyphs during the tell.
    if let AttackKind::Bells {
        count,
        warn_ms,
        spread,
    } = atk.kind
    {
        let (lo, hi) = b.arena;
        let mut xs = vec![target.x];
        let mut k = 1;
        while (xs.len() as u32) < count {
            xs.push(target.x + spread * k as f32);
            if (xs.len() as u32) < count {
                xs.push(target.x - spread * k as f32);
            }
            k += 1;
        }
        for x in xs {
            let x = x.clamp(lo.x + 1.0, hi.x - 1.0);
            commands.spawn((
                BossSpawn,
                RoomEntity,
                Glyph {
                    // +1: the glyph is also ticked once in the tick it is created.
                    ticks: ms_to_ticks(warn_ms) + 1,
                    x,
                    ceiling_y: hi.y - 0.8,
                    bell_half: Vec2::new(0.6, 0.6),
                },
                SimPos(Vec2::new(x, feet.y + 0.05)),
                PrevPos(Vec2::new(x, feet.y + 0.05)),
            ));
        }
    }
}

/// One tick of the active phase. Returns true when the attack is over.
#[allow(clippy::too_many_arguments)]
fn active_step(
    commands: &mut Commands,
    boss: Entity,
    atk: &AttackDef,
    b: &mut BossBrain,
    vel: &mut Velocity,
    pos: Vec2,
    half: Vec2,
    feet: Vec2,
    grounded: bool,
    grid: &TileGrid,
    target: Option<Vec2>,
    pendulums: &Query<Entity, With<Pendulum>>,
    active_ticks: u32,
) -> bool {
    let first = b.timer == 1;
    match atk.kind {
        AttackKind::Slam {
            leap_vy,
            shock_speed,
            shock_ms,
        } => {
            // sub_timer: 0 = just launched, 1 = seen in the air, 2 = landed.
            if first {
                let air_time = 2.0 * leap_vy / GRAVITY;
                vel.y = leap_vy;
                vel.x = ((b.aim.x - pos.x) / air_time).clamp(-MAX_LEAP_VX, MAX_LEAP_VX);
                b.sub_timer = 0;
            } else if b.sub_timer == 0 && !grounded {
                b.sub_timer = 1;
            } else if b.sub_timer == 1 && grounded {
                // Landed: shockwaves race along the floor both ways.
                vel.x = 0.0;
                b.sub_timer = 2;
                for dir in [-1i8, 1] {
                    spawn_shockwave(
                        commands,
                        boss,
                        Vec2::new(feet.x + dir as f32 * half.x, feet.y),
                        dir,
                        shock_speed,
                        ms_to_ticks(shock_ms),
                        atk.damage,
                    );
                }
            }
            (b.sub_timer == 2 && b.timer >= active_ticks) || b.timer >= active_ticks * 2
        }
        AttackKind::Charge { speed, .. } => {
            vel.x = b.facing as f32 * speed;
            let edge = if b.facing > 0 {
                b.arena.1.x
            } else {
                b.arena.0.x
            };
            let side = if b.facing > 0 {
                Side::Right
            } else {
                Side::Left
            };
            let at_edge = (pos.x + b.facing as f32 * half.x - edge).abs() < 0.25
                || (pos.x + b.facing as f32 * half.x - edge) * b.facing as f32 > 0.0;
            if at_edge || probe(grid, pos, half, side, false) {
                b.hit_wall = true;
                return true;
            }
            b.timer >= active_ticks
        }
        AttackKind::Bells { .. } => b.timer >= active_ticks,
        AttackKind::Sweep {
            reach,
            second_gap_ms,
        } => {
            let gap = ms_to_ticks(second_gap_ms);
            if first {
                spawn_arc(commands, boss, half, pos, b.facing, reach, atk.damage);
                b.second_tell = true;
            }
            // Turn to face the player shortly before the second swing.
            if b.timer + 24 == gap + 1 {
                if let Some(t) = target {
                    b.facing = sign(t.x - pos.x, b.facing);
                }
            }
            if b.timer == gap + 1 {
                spawn_arc(commands, boss, half, pos, b.facing, reach, atk.damage);
                b.second_tell = false;
            }
            b.timer >= active_ticks
        }
        AttackKind::Pendulums {
            count,
            amp,
            period_ms,
            life_ms,
        } => {
            if first {
                for p in pendulums {
                    commands.entity(p).despawn();
                }
                let (lo, hi) = b.arena;
                let width = hi.x - lo.x;
                let length = 4.5;
                for k in 0..count {
                    let x = lo.x + (k + 1) as f32 * width / (count + 1) as f32;
                    let pivot = Vec2::new(x, hi.y - 0.5);
                    let period = ms_to_ticks(period_ms).max(2);
                    let bob = pivot + Vec2::new(0.0, -length);
                    let half_b = Vec2::new(0.7, 0.7);
                    let e = commands.spawn_empty().id();
                    commands.entity(e).insert((
                        BossSpawn,
                        RoomEntity,
                        Pendulum {
                            pivot,
                            length,
                            amp,
                            period_ticks: period,
                            // Alternate directions so they never line up.
                            age: if k % 2 == 0 { 0 } else { period / 2 },
                        },
                        SimPos(bob),
                        PrevPos(bob),
                        Hitbox {
                            half: half_b,
                            team: Team::Enemy,
                            damage: atk.damage,
                            kind: HitKind::Contact,
                            attack_dir: AttackDir::Forward,
                            once: false,
                            owner: boss,
                        },
                        Hurtbox {
                            half: half_b,
                            team: Team::Hazard,
                        },
                        Pogoable,
                        Lifetime(ms_to_ticks(life_ms)),
                    ));
                }
            }
            b.timer >= active_ticks
        }
        AttackKind::Toll {
            waves,
            interval_ms,
            speed,
        } => {
            let interval = ms_to_ticks(interval_ms).max(1);
            if b.waves_done < waves && (b.timer - 1) % interval == 0 {
                b.waves_done += 1;
                for dir in [-1i8, 1] {
                    spawn_shockwave(
                        commands,
                        boss,
                        Vec2::new(feet.x + dir as f32 * half.x, feet.y),
                        dir,
                        speed,
                        ms_to_ticks(3000.0),
                        atk.damage,
                    );
                }
            }
            b.timer >= active_ticks
        }
    }
}

/// Warning glyphs count down, then a bell falls where they were.
pub fn glyph_tick(mut commands: Commands, mut q: Query<(Entity, &mut Glyph)>) {
    for (e, mut g) in &mut q {
        g.ticks = g.ticks.saturating_sub(1);
        if g.ticks == 0 {
            let p = Vec2::new(g.x, g.ceiling_y);
            commands.spawn((
                BossSpawn,
                RoomEntity,
                Projectile,
                Hitbox {
                    half: g.bell_half,
                    team: Team::Enemy,
                    damage: 1,
                    kind: HitKind::Projectile,
                    attack_dir: AttackDir::Forward,
                    once: false,
                    owner: e,
                },
                AlreadyHit::default(),
                SimPos(p),
                PrevPos(p),
                Velocity(Vec2::new(0.0, -BELL_SPEED)),
                Lifetime(ms_to_ticks(3000.0)),
            ));
            commands.entity(e).despawn();
        }
    }
}

/// Pendulum bells swing on their chains.
pub fn pendulum_motion(mut q: Query<(&mut Pendulum, &mut SimPos)>) {
    for (mut p, mut pos) in &mut q {
        p.age += 1;
        let phase = std::f32::consts::TAU * p.age as f32 / p.period_ticks as f32;
        let max_angle = (p.amp / p.length).clamp(-1.0, 1.0).asin();
        let angle = max_angle * phase.sin();
        pos.0 = p.pivot + Vec2::new(p.length * angle.sin(), -p.length * angle.cos());
    }
}

/// Seals the exits while a boss fight is on.
pub fn update_arena_lock(mut lock: ResMut<ArenaLock>, q: Query<&BossBrain>) {
    lock.0 = q
        .iter()
        .any(|b| !matches!(b.state, BossState::Sleeping | BossState::Dying));
}

/// Remember defeated bosses so they stay dead.
pub fn record_boss_defeat(mut msgs: MessageReader<BossDefeated>, mut flags: ResMut<WorldFlags>) {
    for m in msgs.read() {
        if let Some(t) = m.tag {
            flags.defeated.insert(t);
        }
    }
}
