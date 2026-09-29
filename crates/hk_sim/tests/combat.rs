//! Exact-tick tests for combat: nail, pogo, soul, hitstop, damage, hazards,
//! focus, Ember Bolt, death. Numbers come from `CombatTuning::default()`.

mod common;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use common::*;
use hk_sim::combat::*;
use hk_sim::components::{Aabb, PrevPos, SimPos, Velocity};
use hk_sim::input::Action;
use hk_sim::player::{Motor, PlayerState};
use hk_sim::testing::Harness;
use hk_sim::tuning::{CombatTuning, Tuning};
use hk_sim::world::TileGrid;

const DUMMY_Y: f32 = 2.0 + 0.5 + 0.001;

fn ct() -> CombatTuning {
    CombatTuning::default()
}

fn dummy(h: &mut Harness, pos: Vec2, hp: i32) -> Entity {
    h.world_mut()
        .spawn((
            SimPos(pos),
            PrevPos(pos),
            Velocity::default(),
            Aabb {
                half: Vec2::new(0.4, 0.5),
            },
            Hurtbox {
                half: Vec2::new(0.4, 0.5),
                team: Team::Enemy,
            },
            Health::full(hp),
            Pogoable,
        ))
        .id()
}

fn immovable_dummy(h: &mut Harness, pos: Vec2, hp: i32) -> Entity {
    let e = dummy(h, pos, hp);
    h.world_mut().entity_mut(e).insert(Poise(0.0));
    e
}

/// A persistent damaging box (enemy body / spikes) that never hurts itself.
fn damage_zone(h: &mut Harness, pos: Vec2, half: Vec2, team: Team, kind: HitKind) -> Entity {
    let e = h.world_mut().spawn_empty().id();
    h.world_mut().entity_mut(e).insert((
        SimPos(pos),
        PrevPos(pos),
        Hitbox {
            half,
            team,
            damage: 1,
            kind,
            attack_dir: AttackDir::Forward,
            once: false,
            owner: e,
        },
    ));
    e
}

fn spikes(h: &mut Harness, pos: Vec2) -> Entity {
    let e = damage_zone(h, pos, Vec2::new(0.5, 0.25), Team::Hazard, HitKind::Hazard);
    h.world_mut().entity_mut(e).insert((
        Hurtbox {
            half: Vec2::new(0.5, 0.25),
            team: Team::Hazard,
        },
        Pogoable,
    ));
    e
}

fn hp(h: &Harness, e: Entity) -> i32 {
    h.world().get::<Health>(e).map_or(0, |x| x.hp)
}
fn soul(h: &Harness, e: Entity) -> i32 {
    h.world().get::<Soul>(e).unwrap().value
}
fn cs(h: &Harness, e: Entity) -> &CombatState {
    h.world().get::<CombatState>(e).unwrap()
}
fn set_hp(h: &mut Harness, e: Entity, v: i32) {
    h.world_mut().get_mut::<Health>(e).unwrap().hp = v;
}
fn set_soul(h: &mut Harness, e: Entity, v: i32) {
    h.world_mut().get_mut::<Soul>(e).unwrap().value = v;
}
fn mash(h: &mut Harness, a: Action) {
    h.release(a);
    h.press(a);
}
fn no_recoil(h: &mut Harness) {
    h.world_mut()
        .resource_mut::<Tuning>()
        .combat
        .nail_recoil_speed = 0.0;
}

// ------------------------------------------------------------------ nail --

#[test]
fn nail_lands_on_the_5th_tick_and_only_once_per_swing() {
    let (mut h, _p) = flat_scene(NONE);
    let d = dummy(&mut h, Vec2::new(11.5, DUMMY_Y), 100);
    h.press(Action::Attack);
    h.tick_n(4);
    assert_eq!(hp(&h, d), 100, "4 startup ticks: no hitbox yet");
    h.tick();
    assert_eq!(hp(&h, d), 95, "first active tick lands the hit");
    h.release(Action::Attack);
    h.tick_n(60);
    assert_eq!(
        hp(&h, d),
        95,
        "one swing = one hit, however long the box lives"
    );
    assert_eq!(h.drain_messages::<Hit>().len(), 1);
}

#[test]
fn forward_reach_ends_where_tuning_says() {
    let hit_at = |dx: f32| {
        let (mut h, _p) = flat_scene(NONE);
        let d = dummy(&mut h, Vec2::new(10.0 + dx, DUMMY_Y), 100);
        h.press(Action::Attack);
        h.tick_n(8);
        hp(&h, d) < 100
    };
    assert!(hit_at(2.9), "inside reach");
    assert!(!hit_at(3.1), "just outside reach");
    assert!(!hit_at(-1.5), "facing right: nothing behind is hit");
}

#[test]
fn swing_cadence_is_exactly_42_ticks() {
    let (mut h, p) = flat_scene(NONE);
    let mut starts = Vec::new();
    for _ in 0..140 {
        mash(&mut h, Action::Attack);
        h.tick();
        if cs(&h, p).attack.is_some_and(|a| a.age == 0) {
            starts.push(h.tick_count());
        }
    }
    assert!(starts.len() >= 4, "swings started: {starts:?}");
    for w in starts.windows(2) {
        assert_eq!(
            w[1] - w[0],
            ct().nail_cooldown_ticks() as u64,
            "gaps: {starts:?}"
        );
    }
}

#[test]
fn up_slash_hits_above() {
    let (mut h, p) = flat_scene(NONE);
    let d = dummy(&mut h, Vec2::new(10.0, 4.75), 100);
    h.press(Action::Up);
    h.press(Action::Attack);
    h.tick();
    assert_eq!(cs(&h, p).attack.unwrap().dir, AttackDir::Up);
    h.tick_n(6);
    assert_eq!(hp(&h, d), 95);
}

#[test]
fn down_slash_only_exists_in_the_air() {
    // On the ground, Down + Attack is an ordinary forward swing.
    let (mut h, p) = flat_scene(NONE);
    h.press(Action::Down);
    h.press(Action::Attack);
    h.tick();
    assert_eq!(cs(&h, p).attack.unwrap().dir, AttackDir::Forward);

    let rows = flat();
    let (mut h, p) = scene(&rows, Vec2::new(10.0, 8.0), NONE);
    h.press(Action::Down);
    h.press(Action::Attack);
    h.tick();
    assert_eq!(cs(&h, p).attack.unwrap().dir, AttackDir::Down);
}

// ------------------------------------------------------------ dash i-frames --

#[test]
fn dashing_through_a_hazard_is_safe_but_only_for_about_120ms() {
    let (mut h, p) = flat_scene(DASH);
    // Two thin damage zones on the dash path. The dash moves 0.2 u per tick,
    // and the i-frames cover the first 15 ticks (x = 10 .. ~13).
    let _early = damage_zone(
        &mut h,
        Vec2::new(11.5, 2.75),
        Vec2::new(0.3, 1.5),
        Team::Enemy,
        HitKind::Contact,
    );
    let _late = damage_zone(
        &mut h,
        Vec2::new(13.9, 2.75),
        Vec2::new(0.3, 1.5),
        Team::Enemy,
        HitKind::Contact,
    );
    h.press(Action::Dash);
    h.tick();
    h.release(Action::Dash);
    assert!(
        h.world().get::<Invulnerable>(p).is_some(),
        "dash grants i-frames"
    );
    let mut hp_after_early = None;
    for _ in 0..30 {
        h.tick();
        let x = pos(&h, p).x;
        if x > 12.5 && hp_after_early.is_none() {
            hp_after_early = Some(hp(&h, p));
        }
    }
    assert_eq!(
        hp_after_early,
        Some(5),
        "passed through the first zone unharmed"
    );
    assert_eq!(
        hp(&h, p),
        4,
        "the second zone, reached after the i-frames ended, hurts"
    );
}

#[test]
fn dash_iframes_never_cut_short_longer_ones() {
    let (mut h, p) = flat_scene(DASH);
    h.world_mut().entity_mut(p).insert(Invulnerable(500));
    h.press(Action::Dash);
    h.tick();
    let left = h.world().get::<Invulnerable>(p).unwrap().0;
    assert!(left > 400, "kept the longer window: {left}");
}

// ------------------------------------------------------------- pogo/soul --

fn pogo_scene(abil: hk_sim::player::Abilities) -> (Harness, Entity) {
    let rows = flat();
    let (mut h, p) = scene(&rows, Vec2::new(10.0, 5.0), abil);
    h.world_mut().get_mut::<Motor>(p).unwrap().air_dash_ready = false;
    h.press(Action::Down);
    h.press(Action::Attack);
    (h, p)
}

#[test]
fn pogo_on_an_enemy_bounces_refills_air_dash_and_grants_soul() {
    let (mut h, p) = pogo_scene(DASH);
    let d = dummy(&mut h, Vec2::new(10.0, DUMMY_Y), 100);
    let mut guard = 0;
    while hp(&h, d) == 100 {
        h.tick();
        guard += 1;
        assert!(guard < 20, "down slash never connected");
    }
    assert_eq!(vel(&h, p).y, ct().pogo_speed, "pogo launch speed");
    assert!(motor(&h, p).air_dash_ready, "air dash refilled");
    assert_eq!(soul(&h, p), 11, "enemy hit grants soul");
}

#[test]
fn pogo_on_spikes_bounces_without_soul_or_damage() {
    let (mut h, p) = pogo_scene(NONE);
    let _s = spikes(&mut h, Vec2::new(10.0, 2.25));
    let mut guard = 0;
    while vel(&h, p).y < 10.0 {
        h.tick();
        guard += 1;
        assert!(guard < 20, "down slash never connected with the spikes");
    }
    assert_eq!(vel(&h, p).y, ct().pogo_speed);
    assert_eq!(soul(&h, p), 0, "hazards give no soul");
    assert_eq!(
        hp(&h, p),
        5,
        "pogo happens before the body touches the spikes"
    );
}

#[test]
fn soul_gains_11_per_hit_and_caps_at_99() {
    let (mut h, p) = flat_scene(NONE);
    no_recoil(&mut h);
    let d = immovable_dummy(&mut h, Vec2::new(11.0, DUMMY_Y), 10_000);
    let mut seen = Vec::new();
    for _ in 0..700 {
        mash(&mut h, Action::Attack);
        h.tick();
        let s = soul(&h, p);
        if seen.last() != Some(&s) {
            seen.push(s);
        }
    }
    assert_eq!(seen, vec![0, 11, 22, 33, 44, 55, 66, 77, 88, 99]);
    assert!(hp(&h, d) <= 10_000 - 5 * 10, "at least 10 hits landed");
}

// --------------------------------------------------------------- hitstop --

#[test]
fn hitstop_freezes_the_sim_but_input_still_latches() {
    let (mut h, p) = pogo_scene(DASH);
    let d = dummy(&mut h, Vec2::new(10.0, DUMMY_Y), 100);
    while hp(&h, d) == 100 {
        h.tick();
    }
    h.release(Action::Attack);
    h.release(Action::Down);
    let y0 = pos(&h, p).y;
    let t0 = h.tick_count();
    h.press(Action::Dash); // pressed during the freeze
    for i in 1..=ct().hitstop_nail_ticks() as u64 {
        h.tick();
        assert_eq!(h.tick_count(), t0 + i, "the tick counter keeps running");
        assert_eq!(pos(&h, p).y, y0, "frozen: no gravity, no movement");
        assert_ne!(state(&h, p), PlayerState::Dash);
    }
    h.tick();
    assert_eq!(
        state(&h, p),
        PlayerState::Dash,
        "the buffered press fires when time resumes"
    );
}

// ---------------------------------------------------------------- damage --

fn overlapping_enemy(h: &mut Harness) -> Entity {
    damage_zone(
        h,
        Vec2::new(10.6, 2.75),
        Vec2::new(3.0, 1.5),
        Team::Enemy,
        HitKind::Contact,
    )
}

#[test]
fn contact_damage_knocks_back_stuns_and_grants_iframes() {
    let (mut h, p) = flat_scene(NONE);
    let _e = overlapping_enemy(&mut h);
    h.tick();
    assert_eq!(hp(&h, p), 4);
    assert!(h.world().get::<Invulnerable>(p).is_some());
    assert_eq!(
        vel(&h, p).x,
        -ct().hurt_knock_vx,
        "pushed away from the enemy (enemy is to the right)"
    );
    assert!(vel(&h, p).y > 0.0);

    // Frozen for the hurt hitstop, then stunned for exactly `stun_ticks`.
    h.tick_n(ct().hitstop_hurt_ticks());
    let mut hurt_ticks = 0;
    for _ in 0..60 {
        h.tick();
        if state(&h, p) == PlayerState::Hurt {
            hurt_ticks += 1;
        }
    }
    assert_eq!(hurt_ticks, ct().stun_ticks(), "stunned for exactly 200 ms");
}

#[test]
fn iframes_last_1300ms_then_the_next_hit_lands() {
    let (mut h, p) = flat_scene(NONE);
    let _e = overlapping_enemy(&mut h);
    // Keep the player alive and in place so the enemy keeps overlapping.
    let mut hit_ticks = Vec::new();
    let mut last_hp = hp(&h, p);
    for _ in 0..400 {
        h.tick();
        let now = hp(&h, p);
        if now < last_hp {
            hit_ticks.push(h.tick_count());
            last_hp = now;
            // Put the player back inside the enemy's box each time.
            h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(10.6, REST_Y);
        }
    }
    assert!(hit_ticks.len() >= 2, "hits at {hit_ticks:?}");
    // Gap = the hit tick itself + 156 protected ticks + the freeze frames.
    let expected = 1 + ct().iframes_ticks() as u64 + ct().hitstop_hurt_ticks() as u64;
    assert_eq!(
        hit_ticks[1] - hit_ticks[0],
        expected,
        "hits at {hit_ticks:?}"
    );
}

#[test]
fn two_overlapping_enemies_never_double_hit_in_one_tick() {
    let (mut h, p) = flat_scene(NONE);
    overlapping_enemy(&mut h);
    overlapping_enemy(&mut h);
    h.tick();
    assert_eq!(hp(&h, p), 4);
}

#[test]
fn spikes_hurt_then_return_you_to_the_last_safe_ground() {
    let (mut h, p) = flat_scene(NONE);
    let spawn = pos(&h, p);
    let _s = spikes(&mut h, Vec2::new(16.0, 2.25));
    // Run at the spikes; safe ground must follow us along the floor, but stay
    // clear of the hazard (margin 1.5 + half widths).
    h.press(Action::Right);
    let mut guard = 0;
    while hp(&h, p) == 5 {
        h.tick();
        guard += 1;
        assert!(guard < 200, "never touched the spikes");
    }
    assert_eq!(hp(&h, p), 4);
    let back = pos(&h, p);
    assert!(
        back.x > spawn.x + 1.0,
        "safe ground followed the run, not the landing spot: {back}"
    );
    // Eligible only while |dx| >= 0.5 + 0.4 + 1.5 = 2.4 from the spikes at x=16.
    assert!(
        back.x <= 16.0 - 2.4 + 0.2,
        "respawned clear of the spikes: {back}"
    );
    assert_eq!(back.y, REST_Y);
    assert_eq!(vel(&h, p), Vec2::ZERO);

    // And it does not just re-hit the spikes: standing there is safe.
    h.release(Action::Right);
    h.tick_n(ct().iframes_ticks() + 40);
    assert_eq!(hp(&h, p), 4, "no repeat damage at the respawn spot");
}

// ----------------------------------------------------------------- focus --

fn focus_scene(hp: i32, soul: i32) -> (Harness, Entity) {
    let (mut h, p) = flat_scene(NONE);
    set_hp(&mut h, p, hp);
    set_soul(&mut h, p, soul);
    (h, p)
}

#[test]
fn focus_heals_one_mask_after_1s_for_33_soul() {
    let (mut h, p) = focus_scene(3, 40);
    h.press(Action::Focus);
    h.tick_n(ct().focus_ticks() - 1);
    assert_eq!(hp(&h, p), 3);
    assert!(cs(&h, p).focusing);
    assert_eq!(state(&h, p), PlayerState::Focus);
    h.tick();
    assert_eq!(hp(&h, p), 4);
    assert_eq!(soul(&h, p), 7);
}

#[test]
fn focus_needs_soul_and_missing_health() {
    let (mut h, p) = focus_scene(3, 32);
    h.press(Action::Focus);
    h.tick_n(200);
    assert_eq!((hp(&h, p), soul(&h, p)), (3, 32), "32 soul is not enough");

    let (mut h, p) = focus_scene(5, 99);
    h.press(Action::Focus);
    h.tick_n(200);
    assert_eq!(
        (hp(&h, p), soul(&h, p)),
        (5, 99),
        "full health: nothing to heal"
    );
}

#[test]
fn releasing_focus_resets_progress() {
    let (mut h, p) = focus_scene(3, 99);
    h.press(Action::Focus);
    h.tick_n(100);
    h.release(Action::Focus);
    h.tick();
    h.press(Action::Focus);
    h.tick_n(ct().focus_ticks() - 1);
    assert_eq!(hp(&h, p), 3, "progress was lost when released");
    h.tick();
    assert_eq!(hp(&h, p), 4);
}

#[test]
fn jumping_and_damage_interrupt_focus() {
    let (mut h, p) = focus_scene(3, 99);
    h.press(Action::Focus);
    h.tick_n(50);
    h.press(Action::Jump);
    h.tick();
    assert!(!cs(&h, p).focusing, "jump cancels focus");
    assert!(vel(&h, p).y > 5.0, "and the jump still happens");

    let (mut h, p) = focus_scene(3, 99);
    h.press(Action::Focus);
    h.tick_n(50);
    assert!(cs(&h, p).focusing);
    overlapping_enemy(&mut h);
    h.tick();
    assert!(
        !cs(&h, p).focusing && cs(&h, p).focus_ticks == 0,
        "damage interrupts focus"
    );
}

// ------------------------------------------------------------ Ember Bolt --

fn walled(col: usize) -> Vec<String> {
    flat()
        .iter()
        .enumerate()
        .map(|(row, line)| {
            let mut s = line.to_string();
            if row < 12 {
                s.replace_range(col..col + 1, "#");
            }
            s
        })
        .collect()
}

#[test]
fn bolt_costs_33_pierces_two_enemies_and_dies_on_the_wall() {
    let rows = walled(30);
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let (mut h, p) = scene(&rows, Vec2::new(10.0, REST_Y), NONE);
    h.tick_n(5);
    set_soul(&mut h, p, 66);
    let d1 = dummy(&mut h, Vec2::new(16.0, DUMMY_Y), 100);
    let d2 = dummy(&mut h, Vec2::new(19.0, DUMMY_Y), 100);
    h.press(Action::Cast);
    h.tick();
    assert_eq!(soul(&h, p), 33, "costs 33 soul");
    h.release(Action::Cast);
    h.tick_n(80);
    assert_eq!(hp(&h, d1), 100 - ct().spell_damage);
    assert_eq!(
        hp(&h, d2),
        100 - ct().spell_damage,
        "pierces through the first target"
    );
    assert_eq!(soul(&h, p), 33, "spell hits grant no soul");
    h.tick_n(100);
    let alive = h.world_mut().query::<&Projectile>().iter(h.world()).count();
    assert_eq!(alive, 0, "stopped by the wall");
}

#[test]
fn bolt_expires_after_its_lifetime_and_needs_33_soul() {
    let (mut h, p) = flat_scene(NONE);
    set_soul(&mut h, p, 32);
    h.press(Action::Cast);
    h.tick();
    assert_eq!(soul(&h, p), 32, "32 soul is not enough");
    assert_eq!(
        h.world_mut().query::<&Projectile>().iter(h.world()).count(),
        0
    );

    set_soul(&mut h, p, 33);
    h.release(Action::Cast);
    h.press(Action::Cast);
    h.tick();
    assert_eq!(
        h.world_mut().query::<&Projectile>().iter(h.world()).count(),
        1
    );
    h.tick_n(ct().spell_lifetime_ticks() + 2);
    assert_eq!(
        h.world_mut().query::<&Projectile>().iter(h.world()).count(),
        0
    );
}

// ------------------------------------------------------------ death etc. --

#[test]
fn death_then_respawn_at_the_respawn_point() {
    let (mut h, p) = flat_scene(NONE);
    set_hp(&mut h, p, 1);
    set_soul(&mut h, p, 50);
    h.world_mut().resource_mut::<RespawnPoint>().0 = Vec2::new(5.0, REST_Y);
    let _e = overlapping_enemy(&mut h);
    h.tick();
    let died_at = h.tick_count();
    assert_eq!(h.drain_messages::<PlayerDied>().len(), 1);
    assert!(cs(&h, p).dead);
    assert_eq!(hp(&h, p), 0);

    // Movement derives the state, and the death tick is followed by hitstop,
    // so `Dead` shows once time resumes.
    h.tick_n(ct().hitstop_hurt_ticks() + 1);
    assert_eq!(state(&h, p), PlayerState::Dead);

    let mut guard = 0;
    while h.drain_messages::<PlayerRespawned>().is_empty() {
        h.tick();
        guard += 1;
        assert!(guard < 400, "never respawned");
    }
    let elapsed = h.tick_count() - died_at;
    let delay = ct().respawn_delay_ticks() as u64;
    let freeze = ct().hitstop_hurt_ticks() as u64;
    assert!(
        (delay..=delay + freeze + 2).contains(&elapsed),
        "respawned {elapsed} ticks after dying"
    );
    assert_eq!(hp(&h, p), 5);
    assert_eq!(soul(&h, p), 0);
    assert_eq!(pos(&h, p), Vec2::new(5.0, REST_Y));
    assert!(!cs(&h, p).dead);
    assert!(h.world().get::<Invulnerable>(p).is_some());
}

// -------------------------------------------------- enemy-side reactions --

#[test]
fn enemy_dies_at_zero_hp_and_reports_it() {
    let (mut h, _p) = flat_scene(NONE);
    let d = dummy(&mut h, Vec2::new(11.5, DUMMY_Y), 5);
    h.press(Action::Attack);
    h.tick_n(6);
    assert!(h.world().get_entity(d).is_err(), "despawned");
    let died = h.drain_messages::<EnemyDied>();
    assert_eq!(died.len(), 1);
    assert_eq!(died[0].entity, d);
}

#[test]
fn enemy_knockback_decays_and_zero_poise_is_immune() {
    let (mut h, _p) = flat_scene(NONE);
    no_recoil(&mut h);
    let a = dummy(&mut h, Vec2::new(11.5, DUMMY_Y), 100);
    let b = immovable_dummy(&mut h, Vec2::new(11.5, DUMMY_Y), 100);
    h.press(Action::Attack);
    h.tick_n(80);
    let moved = pos(&h, a).x - 11.5;
    // 9 u/s decaying linearly over 18 ticks: 9/120 * (1+2+..+18)/18 = 0.7125
    assert!((moved - 0.7125).abs() < 0.02, "knocked back {moved}");
    assert_eq!(pos(&h, b).x, 11.5, "zero poise: not moved");
    assert!(h.world().get::<Knockback>(a).is_none(), "knockback ends");
}

#[test]
fn forward_hit_recoils_the_player_a_little() {
    let (mut h, p) = flat_scene(NONE);
    let d = dummy(&mut h, Vec2::new(11.5, DUMMY_Y), 100);
    h.press(Action::Attack);
    while hp(&h, d) == 100 {
        h.tick();
    }
    h.release(Action::Attack);
    let x0 = pos(&h, p).x;
    assert_eq!(vel(&h, p).x, -ct().nail_recoil_speed);
    h.tick_n(60);
    let back = x0 - pos(&h, p).x;
    assert!(back > 0.3 && back < 0.45, "recoiled {back}");
}

// ----------------------------------------------------------- determinism --

fn scripted_fight() -> Vec<(f32, f32, i32, i32)> {
    let (mut h, p) = flat_scene(DASH);
    let _d1 = dummy(&mut h, Vec2::new(14.0, DUMMY_Y), 60);
    let _d2 = dummy(&mut h, Vec2::new(6.0, DUMMY_Y), 60);
    let _s = spikes(&mut h, Vec2::new(18.0, 2.25));
    let mut rng = hk_sim::rng::SimRng::new(2024);
    let mut trace = Vec::new();
    for tick in 0..2000 {
        if tick % 5 == 0 {
            h.set(Action::Left, rng.chance(0.3));
            h.set(Action::Right, rng.chance(0.5));
            h.set(Action::Up, rng.chance(0.2));
            h.set(Action::Down, rng.chance(0.2));
            h.set(Action::Jump, rng.chance(0.4));
            h.set(Action::Attack, rng.chance(0.6));
            h.set(Action::Dash, rng.chance(0.1));
            h.set(Action::Cast, rng.chance(0.1));
            h.set(Action::Focus, rng.chance(0.2));
        }
        h.tick();
        let q = pos(&h, p);
        trace.push((q.x, q.y, hp(&h, p), soul(&h, p)));
    }
    trace
}

#[test]
fn a_scripted_fight_replays_bit_identically() {
    let a = scripted_fight();
    assert_eq!(a, scripted_fight());
    // The script must actually exercise combat, or determinism proves nothing.
    assert!(a.iter().any(|t| t.3 > 0), "soul was gained");
    assert!(a.iter().any(|t| t.2 < 5), "damage was taken");
}

#[allow(dead_code)]
fn _unused(_: &TileGrid) {}

#[test]
fn falling_out_of_the_world_puts_you_back_on_safe_ground() {
    let (mut h, p) = flat_scene(NONE);
    h.tick_n(30); // stand long enough for the spot to count as safe
    let safe = pos(&h, p);
    h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(10.0, -60.0);
    h.tick_n(2);
    let back = pos(&h, p);
    assert!(
        (back.x - safe.x).abs() < 0.5 && back.y > 0.0,
        "put back near {safe:?}, got {back:?}"
    );
    assert_eq!(hp(&h, p), 5, "no damage for falling out of the world");
}
