//! Enemy state machines: exact timings, ledges, stagger, shields, projectiles.

mod common;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use common::*;
use hk_sim::combat::*;
use hk_sim::components::SimPos;
use hk_sim::enemy::{spawn_enemy, Brain, EnemyKind, EnemyState};
use hk_sim::input::Action;
use hk_sim::testing::Harness;
use hk_sim::tuning::{EnemyTuning, Tuning};
use hk_sim::world::{TileGrid, SKIN};

fn et() -> EnemyTuning {
    EnemyTuning::default()
}

fn ct() -> hk_sim::tuning::CombatTuning {
    hk_sim::tuning::CombatTuning::default()
}

fn spawn(h: &mut Harness, kind: EnemyKind, x: f32) -> Entity {
    let half_y = match kind {
        EnemyKind::Husk => et().husk.half.1,
        EnemyKind::Wisp => et().wisp.half.1,
        EnemyKind::Shieldbearer => et().shield.half.1,
        EnemyKind::Spitter => et().spitter.half.1,
    };
    spawn_enemy(h.world_mut(), kind, Vec2::new(x, 2.0 + half_y + SKIN))
}

fn brain(h: &Harness, e: Entity) -> Brain {
    h.world().get::<Brain>(e).unwrap().clone()
}
fn estate(h: &Harness, e: Entity) -> EnemyState {
    h.world().get::<Brain>(e).unwrap().state
}
fn epos(h: &Harness, e: Entity) -> Vec2 {
    h.world().get::<SimPos>(e).unwrap().0
}
fn hp(h: &Harness, e: Entity) -> i32 {
    h.world().get::<Health>(e).map_or(0, |x| x.hp)
}
fn set_player_x(h: &mut Harness, p: Entity, x: f32) {
    h.world_mut().get_mut::<SimPos>(p).unwrap().0.x = x;
}
/// The player cannot be damaged (so AI timing is not disturbed by hitstop).
fn untouchable(h: &mut Harness, p: Entity) {
    h.world_mut().entity_mut(p).insert(Invulnerable(u32::MAX));
}
fn trace(h: &mut Harness, e: Entity, ticks: u32) -> Vec<EnemyState> {
    (0..ticks)
        .map(|_| {
            h.tick();
            estate(h, e)
        })
        .collect()
}
fn runs(t: &[EnemyState]) -> Vec<(EnemyState, usize)> {
    let mut out: Vec<(EnemyState, usize)> = Vec::new();
    for s in t {
        match out.last_mut() {
            Some((last, n)) if last == s => *n += 1,
            _ => out.push((*s, 1)),
        }
    }
    out
}
fn run_len(r: &[(EnemyState, usize)], s: EnemyState) -> usize {
    r.iter().find(|(x, _)| *x == s).map_or(0, |(_, n)| *n)
}
fn no_recoil(h: &mut Harness) {
    h.world_mut()
        .resource_mut::<Tuning>()
        .combat
        .nail_recoil_speed = 0.0;
}

// ------------------------------------------------------------- fairness --

/// Data lint: every enemy telegraph is readable and every attack leaves a
/// punish window. Guards future tuning edits.
#[test]
fn every_enemy_telegraphs_and_leaves_a_punish_window() {
    let t = et();
    for (name, windup, recover) in [
        ("husk", t.husk.windup_ms, t.husk.recover_ms),
        ("wisp", t.wisp.windup_ms, t.wisp.recover_ms),
        ("shield", t.shield.windup_ms, t.shield.recover_ms),
        ("spitter", t.spitter.windup_ms, t.spitter.recover_ms),
    ] {
        assert!(
            windup >= 300.0,
            "{name}: windup {windup} ms is too short to read"
        );
        assert!(
            recover >= 400.0,
            "{name}: recover {recover} ms leaves no punish window"
        );
    }
}

// ----------------------------------------------------------------- husk --

#[test]
fn husk_patrols_within_range_and_turns_around() {
    let (mut h, _p) = flat_scene(NONE); // player far away at x = 10
    let e = spawn(&mut h, EnemyKind::Husk, 40.0);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for _ in 0..900 {
        h.tick();
        let x = epos(&h, e).x;
        lo = lo.min(x);
        hi = hi.max(x);
        assert_eq!(estate(&h, e), EnemyState::Idle);
    }
    let r = et().husk.patrol_range;
    assert!(
        lo >= 40.0 - r - 0.1 && hi <= 40.0 + r + 0.1,
        "range {lo}..{hi}"
    );
    assert!(
        hi - lo > 2.0 * r - 0.5,
        "it should patrol the whole range: {lo}..{hi}"
    );
}

#[test]
fn husk_notices_for_300ms_then_chases() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn(&mut h, EnemyKind::Husk, 18.0); // 8 units away, inside aggro 9
    let n = et().husk.notice_ticks(); // 36
    h.tick_n(n);
    assert_eq!(
        estate(&h, e),
        EnemyState::Notice,
        "still noticing after {n} ticks"
    );
    assert_eq!(epos(&h, e).x, 18.0, "stands still while noticing");
    h.tick();
    assert_eq!(estate(&h, e), EnemyState::Chase);
    h.tick_n(6);
    assert!(epos(&h, e).x < 18.0, "chasing toward the player");
}

#[test]
fn husk_attack_cycle_has_exact_windup_lunge_and_recovery() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn(&mut h, EnemyKind::Husk, 13.0);
    let mut xs = Vec::new();
    let mut states = Vec::new();
    for _ in 0..260 {
        h.tick();
        states.push(estate(&h, e));
        xs.push(epos(&h, e).x);
    }
    let r = runs(&states);
    let t = et().husk;
    assert_eq!(run_len(&r, EnemyState::Notice), t.notice_ticks() as usize);
    assert_eq!(
        run_len(&r, EnemyState::Windup),
        t.windup_ticks() as usize,
        "the tell: {r:?}"
    );
    assert_eq!(run_len(&r, EnemyState::Attack), t.lunge_ticks() as usize);
    assert_eq!(
        run_len(&r, EnemyState::Recover),
        t.recover_ticks() as usize,
        "punish window"
    );

    // Distance covered by the (first) lunge = speed x duration = 3.0 u.
    let first_attack = states
        .iter()
        .position(|s| *s == EnemyState::Attack)
        .unwrap();
    let last_attack = first_attack + run_len(&r, EnemyState::Attack) - 1;
    // The state flips to Attack at the end of tick `a`; the lunge velocity is
    // applied on ticks a+1 ..= a+30, so compare positions after a and a+30.
    let dist = (xs[last_attack + 1] - xs[first_attack]).abs();
    let expected = t.lunge_speed * t.lunge_ticks() as f32 / 120.0;
    assert!(
        (dist - expected).abs() < 0.05,
        "lunged {dist}, expected {expected}"
    );
}

#[test]
fn husk_stops_at_a_ledge_instead_of_walking_off() {
    // Floor exists for x < 30 and x >= 36 (a 6-wide gap between).
    let mut rows: Vec<String> = flat().iter().map(|s| s.to_string()).collect();
    let n = rows.len();
    for r in [n - 1, n - 2] {
        rows[r].replace_range(30..36, "......");
    }
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
    let p = hk_sim::player::spawn_player(h.world_mut(), Vec2::new(36.5, REST_Y), NONE);
    untouchable(&mut h, p);
    h.tick_n(5);
    let e = spawn(&mut h, EnemyKind::Husk, 28.0);
    let rest = 2.0 + et().husk.half.1 + SKIN;
    let mut max_x = 0.0f32;
    for _ in 0..500 {
        h.tick();
        max_x = max_x.max(epos(&h, e).x);
        assert!(
            (epos(&h, e).y - rest).abs() < 0.01,
            "never fell: y = {}",
            epos(&h, e).y
        );
    }
    assert!(max_x > 29.0, "it did walk to the edge ({max_x})");
    assert!(max_x < 30.0, "but not off it ({max_x})");
}

#[test]
fn a_hit_staggers_the_husk_for_exactly_the_knockback_time() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    no_recoil(&mut h);
    let e = spawn(&mut h, EnemyKind::Husk, 11.5);
    h.press(Action::Attack);
    let states = trace(&mut h, e, 80);
    let stagger = states.iter().filter(|s| **s == EnemyState::Stagger).count();
    assert_eq!(
        stagger,
        ct().enemy_knock_ticks() as usize,
        "states: {:?}",
        runs(&states)
    );
    assert_eq!(hp(&h, e), et().husk.hp - ct().nail_damage);
    assert_ne!(
        estate(&h, e),
        EnemyState::Stagger,
        "recovers from the stagger"
    );
}

#[test]
fn a_husk_dies_in_three_nail_hits() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    no_recoil(&mut h);
    let e = spawn(&mut h, EnemyKind::Husk, 11.5);
    h.world_mut().entity_mut(e).insert(Poise(0.0)); // hold still for the test
    let mut hits = 0;
    for _ in 0..400 {
        h.release(Action::Attack);
        h.press(Action::Attack);
        h.tick();
        hits += h
            .drain_messages::<Hit>()
            .iter()
            .filter(|x| x.victim == e)
            .count();
        if h.world().get_entity(e).is_err() {
            break;
        }
    }
    assert!(h.world().get_entity(e).is_err(), "husk should be dead");
    assert_eq!(hits, 3, "15 hp / 5 damage = exactly 3 hits");
    assert_eq!(h.drain_messages::<EnemyDied>().len(), 1);
}

#[test]
fn touching_a_husk_hurts() {
    let (mut h, p) = flat_scene(NONE);
    let _e = spawn(&mut h, EnemyKind::Husk, 10.4);
    h.tick();
    assert_eq!(h.world().get::<Health>(p).unwrap().hp, 4);
}

#[test]
fn enemies_lose_interest_when_the_player_dies() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn(&mut h, EnemyKind::Husk, 16.0);
    h.tick_n(60);
    assert_ne!(estate(&h, e), EnemyState::Idle, "hunting the player");
    h.world_mut().get_mut::<CombatState>(p).unwrap().dead = true;
    h.tick_n(5);
    assert_eq!(estate(&h, e), EnemyState::Idle, "no living target");
}

// ----------------------------------------------------------------- wisp --

#[test]
fn an_idle_wisp_hovers_and_bobs_without_falling() {
    let (mut h, _p) = flat_scene(NONE);
    let e = spawn_enemy(h.world_mut(), EnemyKind::Wisp, Vec2::new(40.0, 6.0));
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for _ in 0..480 {
        h.tick();
        let y = epos(&h, e).y;
        lo = lo.min(y);
        hi = hi.max(y);
    }
    let range = hi - lo;
    assert!(
        range > 0.5 && range < 0.9,
        "bobbed {range} (amp 0.4 => ~0.8)"
    );
    assert!(lo > 5.4 && hi < 6.6, "stays near home height: {lo}..{hi}");
}

#[test]
fn a_wisp_telegraphs_then_dives_at_the_player_then_recovers() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn_enemy(h.world_mut(), EnemyKind::Wisp, Vec2::new(14.0, 8.0));
    let mut states = Vec::new();
    let mut aim = Vec2::ZERO;
    for _ in 0..500 {
        h.tick();
        states.push(estate(&h, e));
        if estate(&h, e) == EnemyState::Windup {
            aim = brain(&h, e).aim;
        }
    }
    let r = runs(&states);
    let t = et().wisp;
    assert_eq!(run_len(&r, EnemyState::Notice), t.notice_ticks() as usize);
    assert_eq!(
        run_len(&r, EnemyState::Windup),
        t.windup_ticks() as usize,
        "the tell: {r:?}"
    );
    assert_eq!(run_len(&r, EnemyState::Attack), t.dive_ticks() as usize);
    assert_eq!(
        run_len(&r, EnemyState::Recover),
        t.recover_ticks() as usize,
        "punish window"
    );
    assert!(aim.y < -0.6, "the dive is aimed down at the player: {aim}");
}

// --------------------------------------------------------------- shield --

/// Shieldbearer just right of the player; both settle, shield notices.
fn shield_scene() -> (Harness, Entity, Entity) {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    no_recoil(&mut h);
    let s = spawn(&mut h, EnemyKind::Shieldbearer, 11.7);
    (h, p, s)
}

#[test]
fn a_shield_blocks_forward_nail_hits_from_the_front() {
    let (mut h, p, s) = shield_scene();
    // Restore recoil so the block feedback can be observed.
    h.world_mut()
        .resource_mut::<Tuning>()
        .combat
        .nail_recoil_speed = 4.0;
    h.press(Action::Attack);
    let mut blocked = 0;
    for _ in 0..12 {
        h.tick();
        blocked += h.drain_messages::<Blocked>().len();
    }
    assert_eq!(blocked, 1, "one swing, one block");
    assert_eq!(hp(&h, s), et().shield.hp, "no damage through the shield");
    assert_eq!(
        h.world().get::<Soul>(p).unwrap().value,
        0,
        "no soul from a block"
    );
    assert!(h.drain_messages::<Hit>().is_empty());
    assert!(vel(&h, p).x < 0.0, "the block pushes the player back");
}

#[test]
fn a_shield_does_not_stop_a_hit_from_behind_and_turns_slowly() {
    let (mut h, p, s) = shield_scene();
    // It notices with the player on its left and faces left.
    h.tick_n(et().shield.notice_ticks() - 4);
    assert_eq!(brain(&h, s).facing, -1);
    // The player slips behind it (to its right) during the notice beat.
    set_player_x(&mut h, p, 13.6);
    h.tick_n(8); // now in Chase, player behind: it stands and turns
    assert_eq!(estate(&h, s), EnemyState::Chase);
    assert_eq!(
        brain(&h, s).facing,
        -1,
        "still facing away: the shield is on the wrong side"
    );
    h.press(Action::Left); // face left to strike the unshielded back
    h.press(Action::Attack);
    h.tick_n(10);
    assert_eq!(
        hp(&h, s),
        et().shield.hp - ct().nail_damage,
        "hit from behind lands"
    );

    // It only turns once the player has stayed behind for the whole delay.
    // `turn_timer` only ever counts up (it pauses while frozen or staggered)
    // and the shield flips on exactly the tick it reaches the limit.
    h.release(Action::Attack);
    h.release(Action::Left);
    let limit = et().shield.turn_delay_ticks();
    let mut prev = brain(&h, s).turn_timer;
    let mut guard = 0;
    loop {
        h.tick();
        guard += 1;
        assert!(guard < 300, "never turned");
        let b = brain(&h, s);
        if b.facing == 1 {
            assert_eq!(
                prev,
                limit - 1,
                "flipped exactly when the timer hit {limit}"
            );
            break;
        }
        assert!(b.turn_timer >= prev, "turn timer went backwards");
        prev = b.turn_timer;
    }
}

#[test]
fn a_down_slash_pogo_works_on_a_shield() {
    let rows = flat();
    let (mut h, p) = scene(&rows, Vec2::new(11.7, 6.0), NONE);
    untouchable(&mut h, p);
    let s = spawn(&mut h, EnemyKind::Shieldbearer, 11.7);
    h.press(Action::Down);
    h.press(Action::Attack);
    let mut guard = 0;
    while hp(&h, s) == et().shield.hp {
        h.tick();
        guard += 1;
        assert!(guard < 20, "down slash never landed");
    }
    assert_eq!(hp(&h, s), et().shield.hp - ct().nail_damage);
    assert_eq!(vel(&h, p).y, ct().pogo_speed);
    assert!(h.drain_messages::<Blocked>().is_empty());
}

#[test]
fn a_bolt_stops_dead_against_a_shield() {
    let (mut h, p, s) = shield_scene();
    h.world_mut().get_mut::<Soul>(p).unwrap().value = 66;
    h.press(Action::Cast);
    h.tick_n(30);
    assert_eq!(hp(&h, s), et().shield.hp, "bolt did no damage");
    assert_eq!(
        h.world_mut().query::<&Projectile>().iter(h.world()).count(),
        0,
        "bolt is gone"
    );
    assert_eq!(
        h.world().get::<Soul>(p).unwrap().value,
        33,
        "the 33 soul was still spent"
    );
}

#[test]
fn shield_bash_has_exact_windup_and_recovery() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn(&mut h, EnemyKind::Shieldbearer, 12.0); // 2.0 away: inside bash range
    let states = trace(&mut h, e, 300);
    let r = runs(&states);
    let t = et().shield;
    assert_eq!(
        run_len(&r, EnemyState::Windup),
        t.windup_ticks() as usize,
        "{r:?}"
    );
    assert_eq!(run_len(&r, EnemyState::Attack), t.bash_ticks() as usize);
    assert_eq!(run_len(&r, EnemyState::Recover), t.recover_ticks() as usize);
}

// -------------------------------------------------------------- spitter --

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
fn a_spitter_backs_away_when_the_player_is_too_close() {
    let (mut h, p) = flat_scene(NONE);
    untouchable(&mut h, p);
    let e = spawn(&mut h, EnemyKind::Spitter, 14.0); // dx = 4 < min_range 5
    let x0 = epos(&h, e).x;
    h.tick_n(et().spitter.notice_ticks() + 30);
    let x1 = epos(&h, e).x;
    assert!(
        x1 > x0 + 0.5,
        "retreated away from the player: {x0} -> {x1}"
    );
    h.tick_n(120);
    assert!(
        epos(&h, e).x - 10.0 >= et().spitter.min_range - 0.2,
        "settles at min range"
    );
}

#[test]
fn a_spitter_shot_after_a_500ms_tell_hurts_and_is_spent() {
    let (mut h, p) = flat_scene(NONE);
    let e = spawn(&mut h, EnemyKind::Spitter, 18.0); // dx = 8: in the firing band
    let t = et().spitter;
    let mut states = Vec::new();
    let mut fired_at = None;
    let mut hit_at = None;
    for i in 0..400 {
        h.tick();
        states.push(estate(&h, e));
        let shots = h.world_mut().query::<&Projectile>().iter(h.world()).count();
        if shots > 0 && fired_at.is_none() {
            fired_at = Some(i);
        }
        if h.world().get::<Health>(p).unwrap().hp < 5 {
            hit_at = Some(i);
            // Judge the first shot at the moment it lands.
            assert_eq!(
                h.world().get::<Health>(p).unwrap().hp,
                4,
                "exactly one point of damage"
            );
            assert_eq!(shots, 0, "the shot is spent on hit");
            break;
        }
    }
    let r = runs(&states);
    assert_eq!(
        run_len(&r, EnemyState::Windup),
        t.windup_ticks() as usize,
        "the tell: {r:?}"
    );
    let (f, hh) = (
        fired_at.expect("it fired"),
        hit_at.expect("the shot connected"),
    );
    // ~7.4 u at 9 u/s = ~99 ticks of flight.
    assert!((85..115).contains(&(hh - f)), "flight time {}", hh - f);
}

#[test]
fn a_spitter_shot_stops_at_a_wall() {
    let rows = walled(14);
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let (mut h, p) = scene(&rows, Vec2::new(10.0, REST_Y), NONE);
    h.tick_n(5);
    let _e = spawn(&mut h, EnemyKind::Spitter, 18.0);
    let mut shots_seen = 0;
    let mut wall_hits = 0;
    for _ in 0..400 {
        h.tick();
        let live = h.world_mut().query::<&Projectile>().iter(h.world()).count();
        if live > shots_seen {
            shots_seen = live;
        }
        if live == 0 && shots_seen > 0 {
            wall_hits += 1;
        }
    }
    assert!(
        shots_seen >= 1,
        "the spitter did fire (otherwise this proves nothing)"
    );
    assert!(wall_hits > 0, "the shot ended");
    assert_eq!(
        h.world().get::<Health>(p).unwrap().hp,
        5,
        "the wall protected the player"
    );
}

// ---------------------------------------------------------- determinism --

fn scripted_melee() -> Vec<(f32, f32, i32, i32, u8)> {
    let (mut h, p) = flat_scene(DASH);
    let husk = spawn(&mut h, EnemyKind::Husk, 20.0);
    let _w = spawn_enemy(h.world_mut(), EnemyKind::Wisp, Vec2::new(24.0, 7.0));
    let _s = spawn(&mut h, EnemyKind::Shieldbearer, 28.0);
    let _sp = spawn(&mut h, EnemyKind::Spitter, 34.0);
    let mut rng = hk_sim::rng::SimRng::new(77);
    let mut trace = Vec::new();
    for tick in 0..2500 {
        if tick % 6 == 0 {
            h.set(Action::Right, rng.chance(0.55));
            h.set(Action::Left, rng.chance(0.15));
            h.set(Action::Jump, rng.chance(0.3));
            h.set(Action::Attack, rng.chance(0.6));
            h.set(Action::Dash, rng.chance(0.08));
            h.set(Action::Cast, rng.chance(0.08));
        }
        h.tick();
        let q = pos(&h, p);
        let hp = h.world().get::<Health>(p).unwrap().hp;
        let soul = h.world().get::<Soul>(p).unwrap().value;
        let hs = h.world().get::<Brain>(husk).map_or(9, |b| b.state as u8);
        trace.push((q.x, q.y, hp, soul, hs));
    }
    trace
}

#[test]
fn a_melee_with_all_four_enemy_types_replays_bit_identically() {
    let a = scripted_melee();
    assert_eq!(a, scripted_melee());
    assert!(a.iter().any(|t| t.2 < 5), "the player took damage");
    assert!(a.iter().any(|t| t.3 > 0), "the player landed hits");
}
