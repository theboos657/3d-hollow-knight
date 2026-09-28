//! Boss framework and the Bellwarden / Matron: data lints, attack selection,
//! exact attack timings and effects, phases, death, arena lock, determinism.

mod common;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use common::*;
use hk_sim::boss::ai::pick_attack;
use hk_sim::boss::*;
use hk_sim::combat::*;
use hk_sim::components::{SimPos, Velocity};
use hk_sim::input::Action;
use hk_sim::player::spawn_player;
use hk_sim::rng::SimRng;
use hk_sim::testing::Harness;
use hk_sim::tuning::{AttackKind, BossDef, Tuning};
use hk_sim::world::room::*;
use hk_sim::world::{TileGrid, SKIN};

fn defs() -> hk_sim::tuning::BossTuning {
    Tuning::default().bosses
}
fn ct() -> hk_sim::tuning::CombatTuning {
    Tuning::default().combat
}
fn ticks(ms: f32) -> usize {
    hk_sim::ms_to_ticks(ms) as usize
}

// ---------------------------------------------------------------- data lint --

/// Fairness rules from the design, enforced on the data itself.
#[test]
fn every_attack_is_telegraphed_and_punishable_in_every_phase() {
    for boss in &defs().bosses {
        let min_mult = boss.recover_mult.iter().cloned().fold(f32::MAX, f32::min);
        for a in &boss.attacks {
            assert!(
                a.telegraph_ms >= 450.0,
                "{} / {}: tell {} ms is under 450 ms",
                boss.id,
                a.name,
                a.telegraph_ms
            );
            assert!(
                a.recover_ms * min_mult >= 400.0,
                "{} / {}: recovery {} ms x{} is under 400 ms in the snappiest phase",
                boss.id,
                a.name,
                a.recover_ms,
                min_mult
            );
            assert!(a.weight > 0.0 && a.min_range <= a.max_range && a.min_phase <= a.max_phase);
            if let AttackKind::Sweep { second_gap_ms, .. } = a.kind {
                assert!(
                    second_gap_ms >= 300.0,
                    "{}: combo follow-up needs a >= 300 ms tell",
                    a.name
                );
            }
            if let AttackKind::Toll {
                waves, interval_ms, ..
            } = a.kind
            {
                assert!(
                    a.active_ms >= (waves - 1) as f32 * interval_ms + 100.0,
                    "{}: active time must cover all waves",
                    a.name
                );
            }
            if let AttackKind::Bells { warn_ms, .. } = a.kind {
                assert!(
                    a.telegraph_ms >= warn_ms,
                    "{}: glyphs must warn for the whole tell",
                    a.name
                );
            }
        }
    }
}

#[test]
fn phases_are_well_formed_and_every_phase_has_real_choices() {
    for boss in &defs().bosses {
        assert_eq!(
            boss.recover_mult.len(),
            boss.phases() as usize,
            "{}",
            boss.id
        );
        assert!(
            boss.phase_thresholds.windows(2).all(|w| w[0] > w[1]),
            "descending thresholds"
        );
        assert!(boss.phase_thresholds.iter().all(|t| *t > 0.0 && *t < 1.0));
        for phase in 1..=boss.phases() {
            let n = boss
                .attacks
                .iter()
                .filter(|a| phase >= a.min_phase && phase <= a.max_phase)
                .count();
            assert!(n >= 2, "{} phase {phase} has only {n} attack(s)", boss.id);
        }
    }
    let bw = defs().get("bellwarden").unwrap().clone();
    assert_eq!(bw.phases(), 3);
    assert_eq!(bw.hp, 800);
}

// ---------------------------------------------------------------- selection --

fn brain(def: &BossDef, phase: u8) -> BossBrain {
    let mut b = BossBrain::new((Vec2::new(1.0, 1.0), Vec2::new(39.0, 19.0)));
    b.phase = phase;
    let _ = def;
    b
}

fn name(def: &BossDef, i: usize) -> &str {
    &def.attacks[i].name
}

#[test]
fn phase_one_only_uses_phase_one_attacks() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(3);
    let b = brain(&d, 1);
    for _ in 0..500 {
        let i = pick_attack(&d, &b, 6.0, false, true, &mut rng).unwrap();
        assert!(
            ["Toll Slam", "Warden's Charge", "Falling Bells"].contains(&name(&d, i)),
            "phase 1 picked {}",
            name(&d, i)
        );
    }
}

#[test]
fn later_phases_unlock_the_new_attacks() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(4);
    let mut seen2 = std::collections::HashSet::new();
    let mut seen3 = std::collections::HashSet::new();
    for _ in 0..3000 {
        seen2.insert(
            name(
                &d,
                pick_attack(&d, &brain(&d, 2), 3.0, false, true, &mut rng).unwrap(),
            )
            .to_string(),
        );
        seen3.insert(
            name(
                &d,
                pick_attack(&d, &brain(&d, 3), 3.0, false, true, &mut rng).unwrap(),
            )
            .to_string(),
        );
    }
    for n in ["Chain Sweep", "Pendulum Bells", "Falling Bells II"] {
        assert!(seen2.contains(n), "phase 2 never used {n}: {seen2:?}");
    }
    assert!(!seen2.contains("Final Toll"), "Final Toll is phase 3 only");
    assert!(
        seen3.contains("Final Toll"),
        "phase 3 uses Final Toll: {seen3:?}"
    );
    assert!(
        !seen3.contains("Falling Bells"),
        "phase 1 bells are retired"
    );
}

#[test]
fn attacks_respect_their_range() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(5);
    for _ in 0..1000 {
        let close = pick_attack(&d, &brain(&d, 2), 1.0, false, true, &mut rng).unwrap();
        assert_ne!(name(&d, close), "Toll Slam", "slam needs >= 3 u");
        let far = pick_attack(&d, &brain(&d, 2), 12.0, false, true, &mut rng).unwrap();
        assert_ne!(name(&d, far), "Chain Sweep", "sweep reaches <= 4.5 u");
    }
}

#[test]
fn no_attack_is_ever_chosen_three_times_in_a_row() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(6);
    let mut b = brain(&d, 2);
    let mut seq = Vec::new();
    for _ in 0..4000 {
        let i = pick_attack(&d, &b, 3.0, false, true, &mut rng).unwrap();
        b.history = [b.history[1], Some(i)];
        seq.push(i);
    }
    assert!(
        seq.windows(3).all(|w| !(w[0] == w[1] && w[1] == w[2])),
        "found a triple repeat"
    );
    assert!(
        seq.windows(2).any(|w| w[0] == w[1]),
        "pairs are still allowed"
    );
}

#[test]
fn pendulums_are_not_stacked_while_they_are_still_swinging() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(7);
    for _ in 0..1000 {
        let i = pick_attack(&d, &brain(&d, 2), 3.0, true, true, &mut rng).unwrap();
        assert_ne!(name(&d, i), "Pendulum Bells");
    }
}

#[test]
fn a_faraway_player_makes_the_boss_close_in_rather_than_do_nothing() {
    let d = defs().get("bellwarden").unwrap().clone();
    let mut rng = SimRng::new(8);
    // Phase 2 at range 30: only Charge and Bells reach that far, so a pick exists;
    // an artificial range no attack covers returns None so the boss walks instead.
    assert!(pick_attack(&d, &brain(&d, 2), 30.0, false, true, &mut rng).is_some());
    let mut short = d.clone();
    for a in &mut short.attacks {
        a.max_range = 5.0;
    }
    assert!(pick_attack(&short, &brain(&short, 2), 30.0, false, true, &mut rng).is_none());
    assert!(
        pick_attack(&short, &brain(&short, 2), 30.0, false, false, &mut rng).is_some(),
        "gives up and attacks anyway"
    );
}

// ------------------------------------------------------------------- arena --

fn arena_rows() -> Vec<String> {
    let mut v = vec!["#".repeat(40)];
    for _ in 0..17 {
        v.push(format!("#{}#", ".".repeat(38)));
    }
    v.push("#".repeat(40));
    v.push("#".repeat(40));
    v
}

fn arena_bounds() -> (Vec2, Vec2) {
    (Vec2::new(1.0, 2.0), Vec2::new(39.0, 19.0))
}

/// Player at `px`, boss at `bx`, both standing on the floor.
fn boss_scene(id: &str, px: f32, bx: f32) -> (Harness, Entity, Entity) {
    let rows = arena_rows();
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
    let p = spawn_player(h.world_mut(), Vec2::new(px, REST_Y), NONE);
    let b = spawn_boss(h.world_mut(), id, Vec2::new(bx, 2.0), arena_bounds()).unwrap();
    h.tick_n(3);
    (h, p, b)
}

fn only_attack(h: &mut Harness, id: &str, attack: &str) {
    let mut t = h.world_mut().resource_mut::<Tuning>();
    let def = t.bosses.bosses.iter_mut().find(|b| b.id == id).unwrap();
    def.attacks.retain(|a| a.name == attack);
    assert_eq!(def.attacks.len(), 1, "no attack named {attack}");
    let a = &mut def.attacks[0];
    a.min_range = 0.0;
    a.max_range = 99.0;
    a.min_phase = 1;
    a.max_phase = 99;
    def.phase_thresholds.clear();
    def.recover_mult = vec![1.0];
}

fn bb(h: &Harness, b: Entity) -> BossBrain {
    h.world().get::<BossBrain>(b).unwrap().clone()
}
fn bpos(h: &Harness, b: Entity) -> Vec2 {
    h.world().get::<SimPos>(b).unwrap().0
}
fn boss_hp(h: &Harness, b: Entity) -> i32 {
    h.world().get::<Health>(b).map_or(0, |x| x.hp)
}
fn player_hp(h: &Harness, p: Entity) -> i32 {
    h.world().get::<Health>(p).unwrap().hp
}
fn untouchable(h: &mut Harness, p: Entity) {
    h.world_mut().entity_mut(p).insert(Invulnerable(u32::MAX));
}
fn count<T: Component>(h: &mut Harness) -> usize {
    h.world_mut().query::<&T>().iter(h.world()).count()
}
fn run_lengths(states: &[BossState]) -> Vec<(BossState, usize)> {
    let mut out: Vec<(BossState, usize)> = Vec::new();
    for s in states {
        match out.last_mut() {
            Some((l, n)) if l == s => *n += 1,
            _ => out.push((*s, 1)),
        }
    }
    out
}
fn run_len(r: &[(BossState, usize)], s: BossState) -> usize {
    r.iter().find(|(x, _)| *x == s).map_or(0, |(_, n)| *n)
}
/// Runs `n` ticks recording the boss state after each.
fn states(h: &mut Harness, b: Entity, n: usize) -> Vec<BossState> {
    (0..n)
        .map(|_| {
            h.tick();
            h.world()
                .get::<BossBrain>(b)
                .map_or(BossState::Dying, |x| x.state)
        })
        .collect()
}
/// Ticks until the boss has just entered `state` (the current tick is its first).
fn until_state(h: &mut Harness, b: Entity, state: BossState) {
    let mut guard = 0;
    while bb(h, b).state != state {
        h.tick();
        guard += 1;
        assert!(guard < 3000, "boss never reached {state:?}");
    }
}
/// Waits until the waking roar is over. The tick this returns on is the last
/// tick of the intro, so the next tick is the first of whatever comes next.
fn wake_and_finish_intro(h: &mut Harness, b: Entity, _id: &str) {
    let mut guard = 0;
    while matches!(bb(h, b).state, BossState::Sleeping | BossState::Intro) {
        h.tick();
        guard += 1;
        assert!(guard < 2000, "the boss never finished waking");
    }
}

// ----------------------------------------------------------------- waking --

#[test]
fn the_boss_sleeps_until_the_player_steps_into_the_arena() {
    let rows = arena_rows();
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
    let p = spawn_player(h.world_mut(), Vec2::new(5.0, REST_Y), NONE);
    // A tighter arena than the room: x in 10..39, so x = 5 is outside it.
    let b = spawn_boss(
        h.world_mut(),
        "bellwarden",
        Vec2::new(30.0, 2.0),
        (Vec2::new(10.0, 2.0), Vec2::new(39.0, 19.0)),
    )
    .unwrap();
    h.tick_n(200);
    assert_eq!(bb(&h, b).state, BossState::Sleeping);
    assert!(h.drain_messages::<BossAwoke>().is_empty());

    h.world_mut().get_mut::<SimPos>(p).unwrap().0.x = 20.0;
    h.tick_n(2);
    assert_eq!(bb(&h, b).state, BossState::Intro);
    assert_eq!(
        h.drain_messages::<BossAwoke>().len(),
        1,
        "woke exactly once"
    );
    assert!(h.world().resource::<ArenaLock>().0, "the arena seals");
}

#[test]
fn the_waking_roar_is_invulnerable_and_harmless() {
    let (mut h, p, b) = boss_scene("bellwarden", 27.0, 30.0);
    h.tick_n(30); // mid-intro
    assert_eq!(bb(&h, b).state, BossState::Intro);
    // Touching the body does nothing yet, and the nail does no damage.
    h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(30.0, REST_Y);
    h.press(Action::Attack);
    h.tick_n(30);
    assert_eq!(player_hp(&h, p), 5, "no contact damage while waking");
    assert_eq!(boss_hp(&h, b), 800, "no damage taken while waking");
    // After the intro the fight is on.
    h.release(Action::Attack);
    h.tick_n(ticks(2000.0) as u32);
    assert!(!matches!(
        bb(&h, b).state,
        BossState::Intro | BossState::Sleeping
    ));
    assert!(h.world().get::<Disarmed>(b).is_none(), "armed again");
}

// ---------------------------------------------------------- attack effects --

#[test]
fn toll_slam_telegraphs_leaps_lands_by_the_player_and_sends_shockwaves() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    only_attack(&mut h, "bellwarden", "Toll Slam");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");

    until_state(&mut h, b, BossState::Telegraph);
    let mut trace = vec![BossState::Telegraph];
    let mut peak = 0.0f32;
    let mut waves_seen = 0usize;
    let mut landed_x = None;
    for _ in 0..(ticks(650.0) + ticks(900.0) + ticks(700.0) + 60) {
        h.tick();
        let br = bb(&h, b);
        trace.push(br.state);
        peak = peak.max(bpos(&h, b).y);
        let w = count::<Projectile>(&mut h);
        if w > 0 && landed_x.is_none() {
            landed_x = Some(bpos(&h, b).x);
            waves_seen = w;
        }
    }
    let r = run_lengths(&trace);
    let d = defs().get("bellwarden").unwrap().clone();
    assert_eq!(
        run_len(&r, BossState::Telegraph),
        ticks(d.attacks[0].telegraph_ms),
        "the tell: {r:?}"
    );
    assert_eq!(
        run_len(&r, BossState::Recover),
        ticks(d.attacks[0].recover_ms),
        "punish window"
    );
    assert!(peak > 2.0 + 2.0 + 2.0, "it leaped: peak y {peak}");
    assert_eq!(waves_seen, 2, "one shockwave each way");
    let lx = landed_x.unwrap();
    assert!(
        (lx - 20.0).abs() < 3.0,
        "landed near where the player stood: {lx}"
    );
}

#[test]
fn a_shockwave_hurts_if_you_stand_in_it_and_misses_if_you_jump() {
    for jump in [false, true] {
        let (mut h, p, b) = boss_scene("bellwarden", 14.0, 30.0);
        only_attack(&mut h, "bellwarden", "Toll Slam");
        wake_and_finish_intro(&mut h, b, "bellwarden");
        // Wait for the shockwaves, then meet the left-going one.
        let mut guard = 0;
        while count::<Projectile>(&mut h) == 0 {
            h.tick();
            guard += 1;
            assert!(guard < 600, "no shockwave");
        }
        let mut jumped = false;
        for _ in 0..240 {
            h.tick();
            let wave_x = h
                .world_mut()
                .query::<(&Projectile, &Velocity, &SimPos)>()
                .iter(h.world())
                .filter(|(_, v, _)| v.x < 0.0)
                .map(|(_, _, s)| s.0.x)
                .fold(f32::MIN, f32::max);
            let px = h.world().get::<SimPos>(p).unwrap().0.x;
            if jump && !jumped && wave_x > f32::MIN && wave_x - px < 3.0 {
                h.press(Action::Jump);
                jumped = true;
            }
        }
        let hp = player_hp(&h, p);
        if jump {
            assert!(jumped, "the wave arrived");
            assert_eq!(hp, 5, "jumped the wave cleanly");
        } else {
            assert_eq!(hp, 4, "standing still: hit once");
        }
    }
}

#[test]
fn charge_rushes_at_the_player_and_a_wall_hit_staggers_it() {
    // From x = 20 the wall (left) is reached inside the active window.
    let (mut h, p, b) = boss_scene("bellwarden", 8.0, 20.0);
    only_attack(&mut h, "bellwarden", "Warden's Charge");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let d = defs().get("bellwarden").unwrap().clone();
    let a = &d
        .attacks
        .iter()
        .find(|a| a.name == "Warden's Charge")
        .unwrap()
        .clone();

    until_state(&mut h, b, BossState::Telegraph);
    let mut trace = vec![BossState::Telegraph];
    let mut hit_wall = false;
    let mut min_x = f32::MAX;
    for _ in 0..(ticks(a.telegraph_ms) + ticks(a.active_ms) + ticks(a.recover_ms) * 2 + 40) {
        h.tick();
        let br = bb(&h, b);
        trace.push(br.state);
        hit_wall |= br.hit_wall;
        min_x = min_x.min(bpos(&h, b).x);
    }
    let r = run_lengths(&trace);
    assert!(hit_wall, "reached the wall");
    assert_eq!(run_len(&r, BossState::Telegraph), ticks(a.telegraph_ms));
    // Wall stagger: recovery is 1.6x longer.
    assert_eq!(
        run_len(&r, BossState::Recover),
        ticks(a.recover_ms * 1.6),
        "stagger window: {r:?}"
    );
    // Boss half-width 1.5 against the arena's left edge at x = 1: centre ~2.5.
    assert!(min_x < 3.0, "reached the left wall: {min_x}");
}

#[test]
fn a_charge_that_runs_out_of_time_recovers_normally() {
    let (mut h, p, b) = boss_scene("bellwarden", 8.0, 37.0);
    only_attack(&mut h, "bellwarden", "Warden's Charge");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let d = defs().get("bellwarden").unwrap().clone();
    let a = d
        .attacks
        .iter()
        .find(|a| a.name == "Warden's Charge")
        .unwrap()
        .clone();
    until_state(&mut h, b, BossState::Telegraph);
    let mut t = vec![BossState::Telegraph];
    t.extend(states(
        &mut h,
        b,
        ticks(a.telegraph_ms) + ticks(a.active_ms) + ticks(a.recover_ms) + 40,
    ));
    let r = run_lengths(&t);
    assert_eq!(
        run_len(&r, BossState::Recover),
        ticks(a.recover_ms),
        "no wall, no stagger: {r:?}"
    );
    assert!(!bb(&h, b).hit_wall);
}

#[test]
fn charging_into_the_player_hurts() {
    let (mut h, p, b) = boss_scene("bellwarden", 14.0, 30.0);
    only_attack(&mut h, "bellwarden", "Warden's Charge");
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let a = defs()
        .get("bellwarden")
        .unwrap()
        .attacks
        .iter()
        .find(|a| a.name == "Warden's Charge")
        .unwrap()
        .clone();
    until_state(&mut h, b, BossState::Telegraph);
    // Judge at the end of the first charge, before a second one can start.
    h.tick_n((ticks(a.telegraph_ms) + ticks(a.active_ms) + 5) as u32);
    assert_eq!(player_hp(&h, p), 4, "run over once by the first charge");
}

#[test]
fn bells_warn_with_glyphs_then_drop_exactly_there() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 32.0);
    only_attack(&mut h, "bellwarden", "Falling Bells");
    wake_and_finish_intro(&mut h, b, "bellwarden");
    // Advance to the first tick of the tell.
    let mut guard = 0;
    while count::<Glyph>(&mut h) == 0 {
        h.tick();
        guard += 1;
        assert!(guard < 600, "no glyphs appeared");
    }
    let mut xs: Vec<f32> = h
        .world_mut()
        .query::<&Glyph>()
        .iter(h.world())
        .map(|g| g.x)
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        xs,
        vec![17.0, 20.0, 23.0],
        "one on the player, one each side (spread 3)"
    );

    // The warning is visible for exactly 800 ms (96 ticks, counting the first
    // tick above), then bells fall there.
    let warn = ticks(800.0);
    h.tick_n(warn as u32 - 1);
    assert_eq!(
        count::<Glyph>(&mut h),
        3,
        "still warning on the last tick of the warning"
    );
    assert_eq!(count::<Projectile>(&mut h), 0, "no bell yet");
    h.tick();
    assert_eq!(count::<Glyph>(&mut h), 0, "glyphs gone");
    assert_eq!(count::<Projectile>(&mut h), 3, "three bells falling");
    h.tick_n(90);
    assert_eq!(
        count::<Projectile>(&mut h),
        0,
        "bells hit the floor and vanish"
    );
    assert_eq!(player_hp(&h, p), 4, "standing on a glyph gets you hit");
}

#[test]
fn stepping_off_the_glyph_avoids_the_bell() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 32.0);
    only_attack(&mut h, "bellwarden", "Falling Bells");
    wake_and_finish_intro(&mut h, b, "bellwarden");
    while count::<Glyph>(&mut h) == 0 {
        h.tick();
    }
    // Slip to x = 21.5: 1.5 from the glyph at 20 and from the one at 23.
    h.world_mut().get_mut::<SimPos>(p).unwrap().0.x = 21.5;
    h.tick_n(ticks(800.0) as u32 + 100);
    assert_eq!(player_hp(&h, p), 5, "safe between the glyphs");
}

#[test]
fn chain_sweep_swings_twice_with_a_second_tell() {
    let (mut h, p, b) = boss_scene("bellwarden", 27.0, 30.0);
    only_attack(&mut h, "bellwarden", "Chain Sweep");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let mut arc_ticks = Vec::new();
    let mut second_tell_seen = false;
    for _ in 0..(ticks(450.0) + ticks(900.0) + 30) {
        h.tick();
        let fresh = h
            .world_mut()
            .query::<(&HitboxFollow, &Lifetime)>()
            .iter(h.world())
            .filter(|(_, l)| l.0 == 12 - 1)
            .count();
        for _ in 0..fresh {
            arc_ticks.push(h.tick_count());
        }
        second_tell_seen |= bb(&h, b).second_tell;
    }
    assert_eq!(arc_ticks.len(), 2, "two swings: {arc_ticks:?}");
    assert_eq!(
        arc_ticks[1] - arc_ticks[0],
        ticks(400.0) as u64,
        "the follow-up comes 400 ms later"
    );
    assert!(second_tell_seen, "the boss shows the second tell");
}

#[test]
fn a_sweep_hits_at_close_range_and_whiffs_at_a_distance() {
    for (px, expected_hp) in [(26.5, 4), (20.0, 5)] {
        let (mut h, p, b) = boss_scene("bellwarden", px, 30.0);
        only_attack(&mut h, "bellwarden", "Chain Sweep");
        wake_and_finish_intro(&mut h, b, "bellwarden");
        h.tick_n(ticks(450.0) as u32 + ticks(900.0) as u32 + 30);
        assert_eq!(player_hp(&h, p), expected_hp, "player at x = {px}");
    }
}

#[test]
fn pendulum_bells_swing_are_pogo_able_and_expire() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    only_attack(&mut h, "bellwarden", "Pendulum Bells");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let mut guard = 0;
    while count::<Pendulum>(&mut h) == 0 {
        h.tick();
        guard += 1;
        assert!(guard < 600);
    }
    assert_eq!(count::<Pendulum>(&mut h), 3);
    let batch: Vec<Entity> = h
        .world_mut()
        .query_filtered::<Entity, With<Pendulum>>()
        .iter(h.world())
        .collect();
    let first = h
        .world_mut()
        .query::<(&Pendulum, &SimPos)>()
        .iter(h.world())
        .next()
        .map(|(p, s)| (p.pivot, s.0.x))
        .unwrap();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for _ in 0..300 {
        h.tick();
        for (pd, s) in h
            .world_mut()
            .query::<(&Pendulum, &SimPos)>()
            .iter(h.world())
        {
            if (pd.pivot.x - first.0.x).abs() < 0.01 {
                lo = lo.min(s.0.x);
                hi = hi.max(s.0.x);
            }
        }
    }
    let swing = hi - lo;
    assert!(swing > 5.5 && swing < 7.6, "swings about +-3.5 u: {swing}");
    assert!(
        h.world_mut()
            .query::<(&Pendulum, &Pogoable)>()
            .iter(h.world())
            .count()
            == 3,
        "all pogo-able"
    );
    // The first batch vanishes after its lifetime (9 s). (The lone attack in this
    // test may throw a fresh batch afterwards, so track the originals.)
    h.tick_n(ticks(9500.0) as u32);
    assert!(
        batch.iter().all(|e| h.world().get_entity(*e).is_err()),
        "the first batch expired"
    );
}

#[test]
fn final_toll_sends_three_pairs_of_waves_on_a_rhythm() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    only_attack(&mut h, "bellwarden", "Final Toll");
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    let mut spawn_ticks: Vec<u64> = Vec::new();
    let mut last = 0;
    for _ in 0..(ticks(800.0) + ticks(1800.0) + 20) {
        h.tick();
        let n = count::<Projectile>(&mut h);
        if n > last {
            for _ in 0..(n - last) {
                spawn_ticks.push(h.tick_count());
            }
        }
        last = n;
    }
    assert_eq!(
        spawn_ticks.len(),
        6,
        "3 waves x 2 directions: {spawn_ticks:?}"
    );
    assert_eq!(
        spawn_ticks[2] - spawn_ticks[0],
        ticks(600.0) as u64,
        "one wave every 600 ms"
    );
    assert_eq!(spawn_ticks[4] - spawn_ticks[2], ticks(600.0) as u64);
}

// ------------------------------------------------------------------ phases --

fn set_boss_hp(h: &mut Harness, b: Entity, hp: i32) {
    h.world_mut().get_mut::<Health>(b).unwrap().hp = hp;
}

#[test]
fn phases_change_after_the_current_attack_and_the_roar_is_invulnerable() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    // Drop below 65 % while it is mid-attack: it finishes the attack first.
    let mut guard = 0;
    while !matches!(bb(&h, b).state, BossState::Telegraph | BossState::Active) {
        h.tick();
        guard += 1;
        assert!(guard < 600);
    }
    set_boss_hp(&mut h, b, 500); // 62.5 %
    assert_ne!(bb(&h, b).state, BossState::Transition);
    guard = 0;
    while bb(&h, b).state != BossState::Transition {
        assert_eq!(bb(&h, b).phase, 1, "still phase 1 until the attack is over");
        h.tick();
        guard += 1;
        assert!(guard < 1500, "never changed phase");
    }
    assert_eq!(bb(&h, b).phase, 2);
    let msgs = h.drain_messages::<BossPhaseChanged>();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].phase, 2);

    // The roar: no damage, then the fight resumes.
    let hp0 = boss_hp(&h, b);
    h.world_mut().get_mut::<SimPos>(p).unwrap().0.x = bpos(&h, b).x - 2.5;
    h.press(Action::Attack);
    h.tick_n(20);
    assert_eq!(boss_hp(&h, b), hp0, "invulnerable during the roar");
    h.release(Action::Attack);
    h.tick_n(ticks(1500.0) as u32 + 4);
    assert_ne!(bb(&h, b).state, BossState::Transition);
}

#[test]
fn a_huge_hit_still_goes_through_every_phase_in_order() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    untouchable(&mut h, p);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    set_boss_hp(&mut h, b, 100); // 12.5 %: below both thresholds
    let mut phases = Vec::new();
    for _ in 0..4000 {
        h.tick();
        phases.extend(
            h.drain_messages::<BossPhaseChanged>()
                .iter()
                .map(|m| m.phase),
        );
        if phases.len() == 2 {
            break;
        }
    }
    assert_eq!(phases, vec![2, 3], "cannot skip a phase");
}

// ------------------------------------------------------------------- death --

#[test]
fn death_clears_the_attacks_pauses_time_and_removes_the_boss_once() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 32.0);
    only_attack(&mut h, "bellwarden", "Falling Bells");
    untouchable(&mut h, p);
    let tag = 0xABC0_0001u32;
    h.world_mut().entity_mut(b).insert(SpawnTag(tag));
    wake_and_finish_intro(&mut h, b, "bellwarden");
    while count::<Glyph>(&mut h) == 0 {
        h.tick();
    }
    set_boss_hp(&mut h, b, 0);
    h.tick();
    assert_eq!(bb(&h, b).state, BossState::Dying);
    h.tick();
    assert_eq!(count::<Glyph>(&mut h), 0, "attack leftovers are cleared");
    assert!(
        h.world().resource::<HitStop>().0 > 40,
        "a long freeze on the killing blow"
    );
    let d = h.drain_messages::<BossDefeated>();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].tag, Some(tag));
    assert!(
        h.world().resource::<WorldFlags>().defeated.contains(&tag),
        "remembered as defeated"
    );

    h.tick_n(ticks(2500.0) as u32 + 90);
    assert!(
        h.world().get_entity(b).is_err(),
        "removed after the death sequence"
    );
    assert_eq!(h.drain_messages::<EnemyDied>().len(), 1);
    assert!(!h.world().resource::<ArenaLock>().0, "the arena opens");
}

#[test]
fn nothing_can_hurt_a_dying_boss_or_be_hurt_by_it() {
    let (mut h, p, b) = boss_scene("bellwarden", 27.0, 30.0);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    set_boss_hp(&mut h, b, 0);
    h.tick_n(3);
    h.world_mut().get_mut::<SimPos>(p).unwrap().0.x = 30.0;
    h.tick_n(40);
    assert_eq!(player_hp(&h, p), 5, "a dying boss does not touch you");
}

// -------------------------------------------------------------- arena lock --

#[test]
fn exits_are_sealed_during_the_fight_and_open_after_it() {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    untouchable(&mut h, p);
    h.world_mut().spawn((
        RoomEntity,
        SimPos(Vec2::new(20.0, 5.0)),
        hk_sim::components::PrevPos(Vec2::new(20.0, 5.0)),
        RoomExit {
            half: Vec2::new(2.0, 5.0),
            to: "x".into(),
            entry: "y".into(),
        },
    ));
    h.tick_n(50); // boss wakes: the arena seals
    assert!(h.world().resource::<ArenaLock>().0);
    assert!(
        !h.world().resource::<Transition>().active(),
        "cannot leave mid-fight"
    );
    set_boss_hp(&mut h, b, 0);
    h.tick_n(ticks(2500.0) as u32 + 120);
    assert!(!h.world().resource::<ArenaLock>().0);
    assert!(
        h.world().resource::<Transition>().active(),
        "now the exit works"
    );
}

#[test]
fn a_persistent_boss_stays_dead_when_you_come_back() {
    let mut rows = vec!["#".repeat(20)];
    for _ in 0..7 {
        rows.push(format!("#{}#", ".".repeat(18)));
    }
    rows.push("#".repeat(20));
    rows.push("#".repeat(20));
    let mut def = RoomDef {
        id: "arena".into(),
        name: "Arena".into(),
        theme: Theme::Throne,
        tiles: rows,
        spawns: vec![SpawnDef {
            kind: SpawnKind::Matron,
            at: (14.0, 2.0),
            persistent: true,
        }],
        entries: vec![EntryDef {
            name: "in".into(),
            at: (4.0, 2.0),
            facing: 1,
        }],
        exits: vec![],
        benches: vec![],
        pickups: vec![],
    };
    def.tiles.reverse();
    def.tiles.reverse();
    let tag = def.spawn_tag(0);
    let mut h = Harness::new();
    h.world_mut()
        .insert_resource(RoomLibrary::from_defs(vec![def]));
    spawn_player(h.world_mut(), Vec2::ZERO, NONE);
    enter_room(h.world_mut(), "arena", "in").unwrap();
    assert_eq!(count::<Boss>(&mut h), 1, "the boss is there the first time");
    h.world_mut()
        .resource_mut::<WorldFlags>()
        .defeated
        .insert(tag);
    enter_room(h.world_mut(), "arena", "in").unwrap();
    assert_eq!(count::<Boss>(&mut h), 0, "and gone once defeated");
}

// ------------------------------------------------------------------ matron --

#[test]
fn the_matron_is_a_smaller_single_phase_fight_using_the_same_machinery() {
    let d = defs().get("matron").unwrap().clone();
    assert_eq!(d.phases(), 1);
    assert_eq!(d.hp, 300);
    assert_eq!(d.attacks.len(), 2);
    let (mut h, p, b) = boss_scene("matron", 20.0, 30.0);
    untouchable(&mut h, p);
    h.tick_n(ticks(d.intro_ms) as u32 + 2);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..3000 {
        h.tick();
        if let Some(a) = bb(&h, b).attack {
            seen.insert(d.attacks[a].name.clone());
        }
    }
    assert!(
        seen.contains("Toll Slam") && seen.contains("Warden's Charge"),
        "{seen:?}"
    );
    assert_eq!(bb(&h, b).phase, 1);
}

#[test]
fn a_boss_cannot_be_knocked_back_but_can_be_pogoed() {
    let (mut h, p, b) = boss_scene("bellwarden", 27.0, 30.0);
    wake_and_finish_intro(&mut h, b, "bellwarden");
    untouchable(&mut h, p);
    // Down-slash from above: a pogo bounce.
    h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(bpos(&h, b).x, bpos(&h, b).y + 4.0);
    h.world_mut()
        .get_mut::<hk_sim::player::Motor>(p)
        .unwrap()
        .grounded = false;
    h.press(Action::Down);
    h.press(Action::Attack);
    let mut bounced = false;
    for _ in 0..30 {
        h.tick();
        if h.world().get::<Velocity>(p).unwrap().y == ct().pogo_speed {
            bounced = true;
            break;
        }
    }
    assert!(bounced, "pogo off the boss's head");
    assert!(boss_hp(&h, b) < 800, "and it damaged the boss");
    assert!(
        h.world().get::<Knockback>(b).is_none(),
        "no knockback on bosses"
    );
}

// -------------------------------------------------------------- determinism --

fn scripted_boss_fight(seed: u64) -> (Vec<(f32, f32, i32, i32, u8)>, Vec<usize>) {
    let (mut h, p, b) = boss_scene("bellwarden", 20.0, 30.0);
    *h.world_mut().resource_mut::<SimRng>() = SimRng::new(seed);
    let mut rng = SimRng::new(11);
    let mut trace = Vec::new();
    let mut attacks = Vec::new();
    let mut last = None;
    for tick in 0..4000 {
        if tick % 5 == 0 {
            h.set(Action::Right, rng.chance(0.4));
            h.set(Action::Left, rng.chance(0.4));
            h.set(Action::Jump, rng.chance(0.3));
            h.set(Action::Attack, rng.chance(0.6));
            h.set(Action::Dash, rng.chance(0.05));
        }
        h.tick();
        let q = h.world().get::<SimPos>(p).unwrap().0;
        let bh = boss_hp(&h, b);
        let st = h.world().get::<BossBrain>(b).map_or(9, |x| x.state as u8);
        trace.push((q.x, q.y, player_hp(&h, p), bh, st));
        let cur = h.world().get::<BossBrain>(b).and_then(|x| x.attack);
        if cur != last {
            if let Some(a) = cur {
                attacks.push(a);
            }
            last = cur;
        }
    }
    (trace, attacks)
}

#[test]
fn a_whole_boss_fight_replays_bit_identically() {
    let (a, seq_a) = scripted_boss_fight(1);
    let (b, seq_b) = scripted_boss_fight(1);
    assert_eq!(a, b);
    assert_eq!(seq_a, seq_b);
    assert!(a.iter().any(|t| t.2 < 5), "the player took damage");
    assert!(a.iter().any(|t| t.3 < 800), "the boss took damage");
    assert!(seq_a.len() >= 3, "several attacks happened: {seq_a:?}");
}

#[test]
fn different_seeds_give_different_attack_orders() {
    let (_, s1) = scripted_boss_fight(1);
    let (_, s2) = scripted_boss_fight(2);
    assert_ne!(s1, s2, "the seed drives the boss's choices");
}

#[allow(dead_code)]
fn _skin() -> f32 {
    SKIN
}
