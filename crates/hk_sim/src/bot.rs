//! A scripted player for calibrating and regression-testing boss fights.
//!
//! The bot plays with human-like limits. Like a person, it has a reaction delay
//! to *events* (noticing a new telegraph, a new projectile, warning glyphs:
//! `reaction_ticks`, default 250 ms) but tracks the *positions* of things it
//! already knows about continuously. It uses the same
//! tools a person has (jump, dash-through with its i-frames, stepping off warning
//! glyphs, nail, bolt, focus), and makes mistakes now and then. It is *not* a
//! perfect player, so "the bot wins X % of the time in about T seconds" is a
//! meaningful balance signal.
//!
//! It is test/tool code: nothing in the game depends on it.

use std::collections::{HashSet, VecDeque};

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

use crate::boss::{Boss, BossBrain, BossDefeated, BossState, Glyph, Pendulum};
use crate::combat::{CombatState, Health, Hit, HitKind, Hitbox, Projectile, Soul, Team};
use crate::components::{SimPos, Velocity};
use crate::input::{apply_bits, bit, Action};
use crate::player::{Facing, Motor, Player};
use crate::rng::SimRng;
use crate::testing::Harness;
use crate::tuning::{AttackKind, Tuning};
use crate::SimTick;

#[derive(Clone, Copy, Debug)]
pub struct BotConfig {
    /// How late the bot sees the boss and its projectiles.
    pub reaction_ticks: u32,
    /// Chance of failing to react to any single attack / projectile.
    pub mistake_rate: f32,
    pub seed: u64,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            reaction_ticks: 30,
            mistake_rate: 0.10,
            seed: 1,
        }
    }
}

#[derive(Clone, Debug)]
struct Threat {
    id: Entity,
    pos: Vec2,
    vel: Vec2,
    half: Vec2,
}

#[derive(Clone, Debug)]
struct BossObs {
    pos: Vec2,
    half: Vec2,
    state: BossState,
    kind: Option<AttackKind>,
    timer: u32,
    recover_ticks: u32,
    telegraph_ticks: u32,
    aim: Vec2,
    facing: i8,
}

#[derive(Clone, Debug, Default)]
struct Obs {
    boss: Option<BossObs>,
    threats: Vec<Threat>,
    glyphs: Vec<f32>,
}

struct Me {
    pos: Vec2,
    grounded: bool,
    dash_ready: bool,
    hp: i32,
    soul: i32,
    facing: i8,
    swing_ready: bool,
}

pub struct Bot {
    cfg: BotConfig,
    rng: SimRng,
    history: VecDeque<Obs>,
    /// Threats (by entity) the bot has decided not to react to.
    ignored: HashSet<Entity>,
    /// Whether the current boss attack is one the bot will misplay.
    fumble: bool,
    last_state: Option<BossState>,
    attack_phase: bool,
    /// Keep Jump held until this tick, so a jump is a full jump and not a tap.
    jump_hold_until: u64,
}

impl Bot {
    pub fn new(cfg: BotConfig) -> Self {
        Self {
            cfg,
            rng: SimRng::new(cfg.seed),
            history: VecDeque::new(),
            ignored: HashSet::new(),
            fumble: false,
            last_state: None,
            attack_phase: false,
            jump_hold_until: 0,
        }
    }

    fn observe(world: &mut World, boss: Entity) -> Obs {
        let tuning = world.resource::<Tuning>().clone();
        let mut obs = Obs::default();
        if let (Some(b), Some(bb), Some(p), Some(a)) = (
            world.get::<Boss>(boss).cloned(),
            world.get::<BossBrain>(boss).cloned(),
            world.get::<SimPos>(boss).copied(),
            world.get::<crate::components::Aabb>(boss).copied(),
        ) {
            let attack_def = bb
                .attack
                .and_then(|i| tuning.bosses.get(&b.id).and_then(|d| d.attacks.get(i)));
            let kind = attack_def.map(|a| a.kind.clone());
            let telegraph_ticks = attack_def.map_or(0, |a| crate::ms_to_ticks(a.telegraph_ms));
            obs.boss = Some(BossObs {
                pos: p.0,
                half: a.half,
                state: bb.state,
                kind,
                timer: bb.timer,
                recover_ticks: bb.recover_ticks,
                telegraph_ticks,
                aim: bb.aim,
                facing: bb.facing,
            });
        }
        let mut q = world.query::<(
            Entity,
            &Hitbox,
            &SimPos,
            Option<&Velocity>,
            Has<Projectile>,
            Has<Pendulum>,
        )>();
        for (id, hb, pos, vel, proj, pend) in q.iter(world) {
            if hb.team != Team::Enemy || !(proj || pend) || hb.kind == HitKind::Nail {
                continue;
            }
            obs.threats.push(Threat {
                id,
                pos: pos.0,
                vel: vel.map_or(Vec2::ZERO, |v| v.0),
                half: hb.half,
            });
        }
        let mut g = world.query::<&Glyph>();
        obs.glyphs = g.iter(world).map(|g| g.x).collect();
        obs
    }

    fn me(world: &mut World, player: Entity) -> Option<Me> {
        let pos = world.get::<SimPos>(player)?.0;
        let m = world.get::<Motor>(player)?;
        let hp = world.get::<Health>(player)?;
        let soul = world.get::<Soul>(player)?.value;
        let facing = world.get::<Facing>(player)?.0;
        let cs = world.get::<CombatState>(player)?;
        Some(Me {
            pos,
            grounded: m.grounded,
            dash_ready: m.dash_cooldown == 0 && (m.grounded || m.air_dash_ready),
            hp: hp.hp,
            soul,
            facing,
            swing_ready: cs.attack.is_none() && cs.attack_cooldown == 0,
        })
    }

    /// Decides this tick's held buttons (as a bitset).
    pub fn decide(&mut self, world: &mut World, player: Entity, boss: Entity) -> u16 {
        let tick = world.resource::<SimTick>().0;
        let mut bits = self.decide_core(world, player, boss);
        // A person holds the jump button through the rise; a one-tick press is a hop.
        // Never walk into the boss's body (a person would not).
        if let (Some(bo), Some(me)) = (
            self.history.back().and_then(|o| o.boss.clone()),
            Self::me(world, player),
        ) {
            let dx = bo.pos.x - me.pos.x;
            let toward_bit = if dx >= 0.0 {
                bit(Action::Right)
            } else {
                bit(Action::Left)
            };
            if dx.abs() < bo.half.x + 0.9 && me.pos.y < bo.pos.y + bo.half.y + 0.4 {
                bits &= !toward_bit;
            }
        }
        if bits & bit(Action::Jump) != 0 && tick >= self.jump_hold_until {
            self.jump_hold_until = tick + 55;
        }
        if tick < self.jump_hold_until {
            bits |= bit(Action::Jump);
        }
        bits
    }

    fn decide_core(&mut self, world: &mut World, player: Entity, boss: Entity) -> u16 {
        let now = Self::observe(world, boss);
        self.history.push_back(now.clone());
        while self.history.len() > self.cfg.reaction_ticks as usize + 1 {
            self.history.pop_front();
        }
        // Awareness (what the bot has *noticed*) lags by the reaction time...
        let aware = self.history.front().cloned().unwrap_or_default();
        let Some(me) = Self::me(world, player) else {
            return 0;
        };
        let (Some(mut bs), Some(now_boss)) = (aware.boss.clone(), now.boss.clone()) else {
            return 0;
        };
        // ...but where things are right now is tracked continuously.
        bs.pos = now_boss.pos;
        bs.half = now_boss.half;
        bs.timer = now_boss.timer;
        bs.recover_ticks = now_boss.recover_ticks;
        bs.telegraph_ticks = now_boss.telegraph_ticks;
        let mut known: HashSet<Entity> = aware.threats.iter().map(|t| t.id).collect();
        // A Slam or Toll that the bot has already noticed *announces* its
        // shockwaves: nobody needs a second reaction time to see them coming.
        if matches!(
            aware.boss.as_ref().and_then(|b| b.kind.as_ref()),
            Some(AttackKind::Slam { .. }) | Some(AttackKind::Toll { .. })
        ) {
            known.extend(now.threats.iter().map(|t| t.id));
        }
        let seen = Obs {
            boss: aware.boss.clone(),
            threats: now
                .threats
                .iter()
                .filter(|t| known.contains(&t.id))
                .cloned()
                .collect(),
            glyphs: aware.glyphs.clone(),
        };
        let arena = world
            .get::<BossBrain>(boss)
            .map(|b| b.arena)
            .unwrap_or((Vec2::new(1.0, 1.0), Vec2::new(39.0, 19.0)));
        let tick = world.resource::<SimTick>().0;
        self.fumble_check(&bs);

        let dx = bs.pos.x - me.pos.x;
        let dist = dx.abs();
        let toward = if dx >= 0.0 { 1 } else { -1 };
        let go = |dir: i8| {
            if dir > 0 {
                bit(Action::Right)
            } else {
                bit(Action::Left)
            }
        };

        // ---------------------------------------------------------- defence --
        // 1. Anything flying at me (shockwaves, bells, pendulums).
        let mut evade: Option<u16> = None;
        for t in &seen.threats {
            if self.ignored.contains(&t.id) {
                continue;
            }
            if self.rng.chance(self.cfg.mistake_rate) {
                self.ignored.insert(t.id);
                continue;
            }
            let rel = t.pos.x - me.pos.x;
            // Ground-level things moving horizontally at me.
            if t.vel.x.abs() > 1.0
                && t.vel.x.signum() == -rel.signum()
                && (t.pos.y - me.pos.y).abs() < 2.0
            {
                let gap = rel.abs() - (t.half.x + 0.3);
                let closing = t.vel.x.abs();
                let time = gap / closing;
                let dash_dir = if rel > 0.0 { 1 } else { -1 };
                // Dashing through must not end inside the boss's body.
                let dest = me.pos.x + dash_dir as f32 * 4.0;
                let boss_in_way = (bs.pos.x - dest).abs() < bs.half.x + 0.6
                    || (bs.pos.x - me.pos.x) * dash_dir as f32 > 0.0
                        && (bs.pos.x - me.pos.x).abs() < 4.0 + bs.half.x;
                let prefer_dash = me.dash_ready && self.boss_wants_dash(&bs) && !boss_in_way;
                if prefer_dash && (0.2..=2.9).contains(&gap) {
                    evade = Some(bit(Action::Dash) | go(dash_dir));
                } else if me.grounded && (0.05..=0.5).contains(&time) {
                    evade = Some(bit(Action::Jump));
                } else if me.dash_ready && !boss_in_way && (0.2..=2.9).contains(&gap) {
                    evade = Some(bit(Action::Dash) | go(dash_dir));
                }
            }
            // Falling bells: step aside.
            if t.vel.y < -5.0 && (t.pos.x - me.pos.x).abs() < t.half.x + 0.8 {
                let dir = if t.pos.x >= me.pos.x { -1 } else { 1 };
                evade = Some(go(dir));
            }
        }
        if let Some(b) = evade {
            return b;
        }

        // 2. Warning glyphs: stand in a gap between them (or just outside the row).
        if !seen.glyphs.is_empty() && !self.fumble {
            let safe = |x: f32| seen.glyphs.iter().all(|g| (g - x).abs() >= 1.05);
            if !safe(me.pos.x) {
                let mut xs = seen.glyphs.clone();
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let mut candidates: Vec<f32> = xs.windows(2).map(|w| (w[0] + w[1]) * 0.5).collect();
                candidates.push(xs[0] - 1.6);
                candidates.push(xs[xs.len() - 1] + 1.6);
                let best = candidates
                    .into_iter()
                    .filter(|c| *c > arena.0.x + 1.0 && *c < arena.1.x - 1.0 && safe(*c))
                    .min_by(|a, b| {
                        (a - me.pos.x)
                            .abs()
                            .partial_cmp(&(b - me.pos.x).abs())
                            .unwrap()
                    });
                if let Some(c) = best {
                    // Walk there, and stop once inside the gap (no overshooting).
                    if (c - me.pos.x).abs() > 0.15 {
                        return go(if c > me.pos.x { 1 } else { -1 });
                    }
                    return 0;
                }
            } else {
                // Already in a gap between the glyphs: hold still, and use the
                // quiet moment to heal if hurt.
                if me.hp <= 3 && me.soul >= 33 && me.grounded {
                    return bit(Action::Focus);
                }
                return 0;
            }
        }

        // 3. The boss's own attacks.
        if matches!(bs.state, BossState::Telegraph | BossState::Active) && !self.fumble {
            match bs.kind {
                Some(AttackKind::Charge { speed, .. }) => {
                    // The body will sweep toward us along the floor. Jump over it
                    // if it is low enough; otherwise dash through it. During the
                    // telegraph, the time until it starts counts too.
                    let toward_me = (bs.facing as f32) * (me.pos.x - bs.pos.x) > 0.0;
                    if !toward_me {
                        return 0;
                    }
                    let gap = (bs.pos.x - me.pos.x).abs() - (bs.half.x + 0.3);
                    let wait = if bs.state == BossState::Telegraph {
                        bs.telegraph_ticks.saturating_sub(bs.timer) as f32 / crate::TICK_HZ as f32
                    } else {
                        0.0
                    };
                    let t_contact = wait + gap.max(0.0) / speed;
                    let top = 2.0 * bs.half.y + 0.15;
                    // Feet are above `top` between t1 and t2 of a full jump
                    // (v0 = 20, g = 55.6). The body takes `overlap` seconds to pass.
                    let disc = 400.0 - 111.1 * top;
                    if disc > 0.0 && me.grounded {
                        let (t1, t2) = ((20.0 - disc.sqrt()) / 55.56, (20.0 + disc.sqrt()) / 55.56);
                        let overlap = (2.0 * bs.half.x + 0.6) / speed;
                        if t_contact >= t1 && t_contact <= t2 - overlap {
                            return bit(Action::Jump);
                        }
                    }
                    // Fallback: dash through it (i-frames cover the pass).
                    if bs.state == BossState::Active {
                        let contact_in = gap.max(0.0) / (speed + 24.0);
                        if me.dash_ready && (0.0..=0.05).contains(&contact_in) {
                            return bit(Action::Dash) | go(-bs.facing);
                        }
                    }
                    return 0;
                }
                Some(AttackKind::Slam {
                    leap_vy,
                    max_leap_speed,
                    ..
                }) => {
                    // The boss lands where it is aiming, but a leap only reaches so
                    // far: work out the real landing spot.
                    let air_time = 2.0 * leap_vy / 60.0;
                    let leap = ((bs.aim.x - bs.pos.x) / air_time)
                        .clamp(-max_leap_speed, max_leap_speed)
                        * air_time;
                    let land_x = bs.pos.x + leap;
                    let off = me.pos.x - land_x;

                    // Close to the landing when it happens: time the jump to the
                    // landing (a person anticipates it, they do not wait to see the
                    // wave), or dash out through the body if right underneath.
                    if bs.state == BossState::Active && me.grounded {
                        let air_ticks = (air_time * crate::TICK_HZ as f32) as u32;
                        let remaining = air_ticks.saturating_sub(bs.timer);
                        if off.abs() < bs.half.x + 0.9
                            && (3..=9).contains(&remaining)
                            && me.dash_ready
                        {
                            // Underneath: dash out toward the open side.
                            let open = if arena.1.x - me.pos.x > me.pos.x - arena.0.x {
                                1
                            } else {
                                -1
                            };
                            return bit(Action::Dash) | go(open);
                        }
                        // Only for waves that arrive right away (about 4 u); farther
                        // ones are jumped when they actually approach.
                        if off.abs() >= bs.half.x + 0.9
                            && off.abs() < 4.2
                            && (0..=10).contains(&remaining)
                        {
                            return bit(Action::Jump);
                        }
                    }

                    // Otherwise stay well clear (about 7 u) so there is time to jump
                    // the shockwave, moving away from the boss and never into it.
                    let want_clear = 7.0;
                    if off.abs() < want_clear {
                        let dir = if off.abs() < 0.5 {
                            -toward
                        } else if off > 0.0 {
                            1
                        } else {
                            -1
                        };
                        let room = if dir > 0 {
                            arena.1.x - 1.5 - me.pos.x
                        } else {
                            me.pos.x - arena.0.x - 1.5
                        };
                        if room < 0.5 {
                            // Cornered: stay put and rely on the timed jump above.
                            return 0;
                        }
                        let into_body = dir == toward && dist < bs.half.x + 1.4;
                        return if into_body { 0 } else { go(dir) };
                    }
                    return 0;
                }
                Some(AttackKind::Sweep { .. }) => {
                    if dist < bs.half.x + 4.6 + 0.8 {
                        if me.dash_ready {
                            return bit(Action::Dash) | go(-toward);
                        }
                        return go(-toward);
                    }
                    return 0;
                }
                _ => {}
            }
        }

        // A wave is crossing the floor toward me: hold position and wait for the
        // jump window instead of walking into it, then close in afterwards.
        let wave_coming = seen.threats.iter().any(|t| {
            !self.ignored.contains(&t.id)
                && t.vel.x.abs() > 1.0
                && t.vel.x.signum() == -(t.pos.x - me.pos.x).signum()
                && (t.pos.y - me.pos.y).abs() < 2.0
                && (t.pos.x - me.pos.x).abs() < 9.0
        });
        if wave_coming && me.grounded {
            return 0;
        }

        // ---------------------------------------------------------- offence --
        let reach = bs.half.x + 0.4 + 2.2 - 0.3; // stand a little inside nail reach
        let punish = matches!(bs.state, BossState::Recover);
        let remaining = bs.recover_ticks.saturating_sub(bs.timer);

        // Heal in safe windows (roars, long recoveries).
        // (The next attack always has a tell of at least 450 ms before it can hit,
        // so a recovery with about 0.7 s left, seen from a distance, is enough.)
        let safe_heal = matches!(bs.state, BossState::Transition | BossState::Intro)
            || (punish && remaining >= 80 && dist >= 6.0);
        if me.hp <= 3 && me.soul >= 33 && me.grounded && safe_heal {
            return bit(Action::Focus);
        }

        if punish {
            if me.soul >= 33 && dist > 3.0 && dist < 10.0 && me.swing_ready {
                let face = if me.facing == toward { 0 } else { go(toward) };
                if face == 0 {
                    return bit(Action::Cast);
                }
                return face;
            }
            if dist > reach {
                return go(toward);
            }
            // In reach: face and swing.
            let face = if me.facing == toward { 0 } else { go(toward) };
            let swing = if tick.is_multiple_of(2) {
                bit(Action::Attack)
            } else {
                0
            };
            return face | swing;
        }

        // Neutral: hold a comfortable distance so there is time to react, and
        // poke when the boss is only walking at us.
        if matches!(bs.state, BossState::Approach | BossState::Choose) && dist <= reach + 0.5 {
            let face = if me.facing == toward { 0 } else { go(toward) };
            return face
                | if tick.is_multiple_of(2) {
                    bit(Action::Attack)
                } else {
                    0
                };
        }
        let want = 6.5;
        if dist < want - 1.0 {
            return go(-toward);
        }
        if dist > want + 2.5 {
            return go(toward);
        }
        0
    }

    /// Dash-through is the better answer while jumps are dangerous or too slow.
    fn boss_wants_dash(&self, b: &BossObs) -> bool {
        matches!(
            b.kind,
            Some(AttackKind::Toll { .. }) | Some(AttackKind::Pendulums { .. })
        )
    }

    /// New boss attack: decide whether the bot will misplay it.
    fn fumble_check(&mut self, b: &BossObs) {
        let attacking = matches!(b.state, BossState::Telegraph);
        if attacking && !self.attack_phase {
            self.fumble = self.rng.chance(self.cfg.mistake_rate);
        }
        self.attack_phase = attacking || matches!(b.state, BossState::Active);
        self.last_state = Some(b.state);
    }

    /// Drives one tick: decide, apply the buttons, advance the sim.
    pub fn tick(&mut self, h: &mut Harness, player: Entity, boss: Entity) {
        let bits = self.decide(h.world_mut(), player, boss);
        let t = SimTick(h.tick_count());
        let mut input =
            std::mem::take(&mut *h.world_mut().resource_mut::<crate::input::InputState>());
        apply_bits(&mut input, bits, &t);
        *h.world_mut().resource_mut::<crate::input::InputState>() = input;
        h.tick();
    }
}

#[derive(Clone, Debug)]
pub struct FightResult {
    pub won: bool,
    pub died: bool,
    pub ticks: u64,
    pub hits_taken: u32,
    /// How many masks the bot healed with Focus.
    pub heals: u32,
    pub boss_hp_left: i32,
    pub max_phase: u8,
    pub attacks_used: Vec<usize>,
    /// What hurt the player: (tick of the fight, hit kind, boss attack index, boss state).
    pub hit_log: Vec<(u64, HitKind, Option<usize>, BossState)>,
    /// Positions at each hit: (boss, player).
    pub hit_pos: Vec<(Vec2, Vec2)>,
}

impl FightResult {
    pub fn seconds(&self) -> f32 {
        self.ticks as f32 / crate::TICK_HZ as f32
    }
}

/// Runs a fight until the boss dies, the player dies, or `max_ticks` pass.
pub fn run_boss_fight(
    h: &mut Harness,
    player: Entity,
    boss: Entity,
    cfg: BotConfig,
    max_ticks: u64,
) -> FightResult {
    let mut bot = Bot::new(cfg);
    let start = h.tick_count();
    let mut last_hp = h.world().get::<Health>(player).map_or(0, |x| x.hp);
    let mut hits = 0;
    let mut heals = 0;
    let mut max_phase = 1;
    let mut attacks = Vec::new();
    let mut last_attack = None;
    let mut won = false;
    let mut died = false;
    let mut hit_log = Vec::new();
    let mut hit_pos = Vec::new();

    while h.tick_count() - start < max_ticks {
        bot.tick(h, player, boss);
        for hit in h.drain_messages::<Hit>() {
            if hit.victim_team == Team::Player {
                let (a, st) = h
                    .world()
                    .get::<BossBrain>(boss)
                    .map_or((None, BossState::Sleeping), |b| (b.attack, b.state));
                hit_log.push((h.tick_count() - start, hit.kind, a, st));
                let bp = h.world().get::<SimPos>(boss).map_or(Vec2::ZERO, |p| p.0);
                let pp = h.world().get::<SimPos>(player).map_or(Vec2::ZERO, |p| p.0);
                hit_pos.push((bp, pp));
            }
        }
        let hp = h.world().get::<Health>(player).map_or(0, |x| x.hp);
        if hp < last_hp {
            hits += 1;
        } else if hp > last_hp {
            heals += 1;
        }
        last_hp = hp;
        if let Some(b) = h.world().get::<BossBrain>(boss) {
            max_phase = max_phase.max(b.phase);
            if b.attack != last_attack {
                if let Some(a) = b.attack {
                    attacks.push(a);
                }
                last_attack = b.attack;
            }
        }
        if !h.drain_messages::<BossDefeated>().is_empty() {
            won = true;
            break;
        }
        if h.world().get::<CombatState>(player).is_some_and(|c| c.dead) {
            died = true;
            break;
        }
    }
    let boss_hp_left = h.world().get::<Health>(boss).map_or(0, |x| x.hp);
    FightResult {
        won,
        died,
        ticks: h.tick_count() - start,
        hits_taken: hits,
        heals,
        boss_hp_left,
        max_phase,
        attacks_used: attacks,
        hit_log,
        hit_pos,
    }
}

/// Convenience: is `e` the player (used by tools that query the world).
pub fn find_player(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .next()
}
