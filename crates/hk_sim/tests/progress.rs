//! Benches, pickups, boss rewards, respawning at the last bench, and saves.

mod common;

use bevy_ecs::message::Messages;
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use common::*;
use hk_sim::boss::{ArenaLock, BossDefeated};
use hk_sim::combat::{CombatState, Health, RespawnPoint, Soul};
use hk_sim::components::SimPos;
use hk_sim::enemy::Enemy;
use hk_sim::input::Action;
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::testing::Harness;
use hk_sim::world::progress::*;
use hk_sim::world::room::*;
use hk_sim::world::SKIN;

const AIR: &str = "....................";
const FLOOR: &str = "####################";

fn room(id: &str) -> RoomDef {
    let mut tiles = vec![AIR.to_string(); 8];
    tiles.push(FLOOR.to_string());
    tiles.push(FLOOR.to_string());
    RoomDef {
        id: id.into(),
        name: id.into(),
        theme: Theme::Ashen,
        tiles,
        spawns: vec![],
        entries: vec![EntryDef {
            name: "here".into(),
            at: (10.0, 2.0),
            facing: 1,
        }],
        exits: vec![],
        benches: vec![],
        pickups: vec![],
    }
}

fn husk_at(x: f32) -> SpawnDef {
    SpawnDef {
        kind: SpawnKind::Husk,
        at: (x, 2.0),
        persistent: false,
    }
}

fn world_of(rooms: Vec<RoomDef>, abil: Abilities) -> (Harness, Entity) {
    let mut h = Harness::new();
    h.world_mut().insert_resource(RoomLibrary::from_defs(rooms));
    let p = spawn_player(h.world_mut(), Vec2::new(1.0, 5.0), abil);
    (h, p)
}

fn enemies(h: &mut Harness) -> usize {
    h.world_mut().query::<&Enemy>().iter(h.world()).count()
}

fn set_hp(h: &mut Harness, p: Entity, hp: i32) {
    h.world_mut().get_mut::<Health>(p).unwrap().hp = hp;
}

fn hp(h: &Harness, p: Entity) -> i32 {
    h.world().get::<Health>(p).unwrap().hp
}

fn run_out_transition(h: &mut Harness) {
    let mut guard = 0;
    while h.world().resource::<Transition>().active() {
        h.tick();
        guard += 1;
        assert!(guard < 200, "transition never finished");
    }
}

fn kill(h: &mut Harness, p: Entity) {
    let mut cs = h.world_mut().get_mut::<CombatState>(p).unwrap();
    cs.dead = true;
    cs.dead_ticks = 4;
}

// ------------------------------------------------------------------ benches --

fn bench_room() -> RoomDef {
    let mut a = room("A");
    a.benches.push(BenchDef { at: (10.0, 2.0) });
    a.spawns.push(husk_at(17.0));
    a
}

#[test]
fn resting_heals_sets_the_checkpoint_and_resets_the_room_behind_a_fade() {
    let (mut h, p) = world_of(vec![bench_room()], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    set_hp(&mut h, p, 2);
    h.world_mut().get_mut::<Soul>(p).unwrap().value = 0;
    // A husk that has been killed comes back when you rest.
    let husk = h
        .world_mut()
        .query_filtered::<Entity, With<Enemy>>()
        .iter(h.world())
        .next()
        .unwrap();
    h.world_mut().despawn(husk);
    assert_eq!(enemies(&mut h), 0);
    h.drain_messages::<BenchRested>();

    h.press(Action::Up);
    h.tick();
    assert_eq!(h.drain_messages::<BenchRested>().len(), 1);
    assert_eq!(hp(&h, p), 5, "healed to full");
    assert_eq!(h.world().get::<Soul>(p).unwrap().value, 99, "soul refilled");
    let cp = h.world().resource::<Checkpoint>().clone();
    assert_eq!(cp.room, "A");
    assert_eq!(cp.pos, Vec2::new(10.0, 2.0), "feet on the bench");
    assert!(h.world().resource::<Transition>().active(), "fades out");

    run_out_transition(&mut h);
    assert_eq!(enemies(&mut h), 1, "the room reloaded, the husk is back");
    assert_eq!(h.world().resource::<CurrentRoom>().id, "A");
}

#[test]
fn resting_needs_a_bench_a_press_and_solid_ground() {
    let (mut h, p) = world_of(vec![bench_room()], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    set_hp(&mut h, p, 2);

    // No press: nothing happens, however long you stand there.
    h.tick_n(30);
    assert_eq!(hp(&h, p), 2);
    assert!(!h.world().resource::<Checkpoint>().is_set());

    // Away from the bench.
    h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(4.0, REST_Y);
    h.tick_n(3);
    h.press(Action::Up);
    h.tick_n(3);
    assert_eq!(hp(&h, p), 2, "too far from the bench");
    h.release(Action::Up);

    // In the air above it.
    h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(10.0, REST_Y + 1.5);
    h.press(Action::Up);
    h.tick();
    assert_eq!(hp(&h, p), 2, "not while airborne");
}

#[test]
fn no_resting_while_a_boss_fight_seals_the_room() {
    let mut a = bench_room();
    a.spawns.clear();
    a.spawns.push(SpawnDef {
        kind: SpawnKind::Matron,
        at: (16.0, 2.0),
        persistent: true,
    });
    let (mut h, p) = world_of(vec![a], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    assert!(
        h.world().resource::<ArenaLock>().0,
        "the boss woke and the room is sealed"
    );
    set_hp(&mut h, p, 2);
    h.press(Action::Up);
    h.tick_n(3);
    assert_eq!(hp(&h, p), 2, "no benches in the middle of a boss fight");
    assert!(!h.world().resource::<Checkpoint>().is_set());
}

// -------------------------------------------------------------- respawning --

#[test]
fn dying_returns_you_to_the_last_bench_in_another_room() {
    let mut b = room("B");
    b.spawns.push(husk_at(15.0));
    let (mut h, p) = world_of(vec![bench_room(), b], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    h.press(Action::Up);
    h.tick();
    run_out_transition(&mut h);
    h.release(Action::Up);

    enter_room(h.world_mut(), "B", "here").unwrap();
    h.tick_n(5);
    assert_eq!(h.world().resource::<CurrentRoom>().id, "B");
    set_hp(&mut h, p, 1);
    h.world_mut().get_mut::<Soul>(p).unwrap().value = 50;
    kill(&mut h, p);

    let mut guard = 0;
    while h.world().resource::<CurrentRoom>().id != "A" {
        h.tick();
        guard += 1;
        assert!(guard < 200, "never went back to the bench");
    }
    run_out_transition(&mut h);
    assert_eq!(hp(&h, p), 5, "full health");
    assert!(!h.world().get::<CombatState>(p).unwrap().dead);
    let at = h.world().get::<SimPos>(p).unwrap().0;
    assert_eq!(at, Vec2::new(10.0, 2.0 + 0.75 + SKIN), "on the bench");
}

#[test]
fn a_boss_room_resets_when_you_die_in_it_and_a_defeated_boss_stays_dead() {
    let mut arena = room("Arena");
    arena.spawns.push(SpawnDef {
        kind: SpawnKind::Matron,
        at: (15.0, 2.0),
        persistent: true,
    });
    let mut home = room("Home");
    home.benches.push(BenchDef { at: (10.0, 2.0) });
    let (mut h, p) = world_of(vec![home, arena.clone()], NONE);
    enter_room(h.world_mut(), "Home", "here").unwrap();
    h.tick_n(5);
    h.press(Action::Up);
    h.tick();
    run_out_transition(&mut h);
    h.release(Action::Up);

    enter_room(h.world_mut(), "Arena", "here").unwrap();
    let boss = |h: &mut Harness| {
        h.world_mut()
            .query::<&hk_sim::boss::Boss>()
            .iter(h.world())
            .count()
    };
    assert_eq!(boss(&mut h), 1);
    // Wound it, die, and come back: it is a fresh boss again.
    let b = h
        .world_mut()
        .query_filtered::<Entity, With<hk_sim::boss::Boss>>()
        .iter(h.world())
        .next()
        .unwrap();
    h.world_mut().get_mut::<Health>(b).unwrap().hp = 10;
    kill(&mut h, p);
    let mut guard = 0;
    while h.world().resource::<CurrentRoom>().id != "Home" {
        h.tick();
        guard += 1;
        assert!(guard < 300);
    }
    run_out_transition(&mut h);
    enter_room(h.world_mut(), "Arena", "here").unwrap();
    let b = h
        .world_mut()
        .query_filtered::<(Entity, &Health), With<hk_sim::boss::Boss>>()
        .iter(h.world())
        .next()
        .map(|(_, hp)| hp.hp)
        .unwrap();
    assert_eq!(b, 300, "a fresh Matron, at full health");

    // Defeat it for good: no boss on the next visit.
    let tag = arena.spawn_tag(0);
    h.world_mut()
        .resource_mut::<WorldFlags>()
        .defeated
        .insert(tag);
    enter_room(h.world_mut(), "Arena", "here").unwrap();
    assert_eq!(boss(&mut h), 0);
}

#[test]
fn without_a_checkpoint_death_still_respawns_in_place() {
    // (The sandbox and the unit tests have no bench.)
    let (mut h, p) = world_of(vec![room("A")], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    h.world_mut().resource_mut::<RespawnPoint>().0 = Vec2::new(5.0, REST_Y);
    kill(&mut h, p);
    h.tick_n(6);
    assert_eq!(
        h.world().get::<SimPos>(p).unwrap().0,
        Vec2::new(5.0, REST_Y)
    );
    assert!(!h.world().resource::<Transition>().active());
}

// ----------------------------------------------------------------- pickups --

fn shrine() -> RoomDef {
    let mut a = room("Shrine");
    a.pickups.push(PickupDef {
        ability: Ability::WallGrip,
        at: (10.0, 2.0),
    });
    a
}

#[test]
fn a_pickup_teaches_an_ability_once_and_stays_gone() {
    let (mut h, p) = world_of(vec![shrine()], NONE);
    enter_room(h.world_mut(), "Shrine", "here").unwrap();
    assert!(!h.world().get::<Abilities>(p).unwrap().wall_grip);
    h.tick_n(3);
    let gained = h.drain_messages::<AbilityGained>();
    assert_eq!(gained.len(), 1);
    assert_eq!(gained[0].ability, Ability::WallGrip);
    let a = *h.world().get::<Abilities>(p).unwrap();
    assert!(a.wall_grip && !a.dash, "only what the pickup gives");
    assert_eq!(
        h.world_mut().query::<&Pickup>().iter(h.world()).count(),
        0,
        "the pickup is used up"
    );

    // Leave and come back: it does not reappear.
    enter_room(h.world_mut(), "Shrine", "here").unwrap();
    assert_eq!(
        h.world_mut().query::<&Pickup>().iter(h.world()).count(),
        0,
        "collected pickups stay collected"
    );
}

#[test]
fn defeating_the_matron_teaches_dash_but_other_bosses_do_not() {
    let (mut h, p) = world_of(vec![room("A")], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.world_mut()
        .resource_mut::<Messages<BossDefeated>>()
        .write(BossDefeated {
            id: "bellwarden".into(),
            tag: None,
        });
    h.tick();
    assert!(!h.world().get::<Abilities>(p).unwrap().dash);
    assert!(h.drain_messages::<AbilityGained>().is_empty());

    h.world_mut()
        .resource_mut::<Messages<BossDefeated>>()
        .write(BossDefeated {
            id: "matron".into(),
            tag: Some(1),
        });
    h.tick();
    assert!(h.world().get::<Abilities>(p).unwrap().dash);
    let g = h.drain_messages::<AbilityGained>();
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].ability, Ability::Dash);
}

#[test]
fn spawn_and_pickup_tags_never_collide() {
    let d = room("Same");
    let mut seen = std::collections::HashSet::new();
    for i in 0..40 {
        assert!(seen.insert(d.spawn_tag(i)));
        assert!(seen.insert(d.pickup_tag(i)));
    }
}

// -------------------------------------------------------------------- save --

#[test]
fn a_save_round_trips_through_ron_and_restores_the_world() {
    let (mut h, p) = world_of(vec![bench_room(), shrine()], DASH);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(5);
    h.press(Action::Up);
    h.tick();
    run_out_transition(&mut h);
    h.release(Action::Up);
    enter_room(h.world_mut(), "Shrine", "here").unwrap();
    h.tick_n(3); // takes the wall-grip pickup
    h.world_mut()
        .resource_mut::<WorldFlags>()
        .defeated
        .insert(0xABCD_E000);

    let saved = SaveData::capture(h.world_mut());
    assert!(saved.dash && saved.wall_grip);
    assert_eq!(
        saved.room, "A",
        "you wake up at the bench, not where you saved"
    );
    let text = saved.to_ron();
    let loaded = SaveData::from_ron(&text).expect("parses");
    assert_eq!(saved, loaded);
    let _ = p;

    // A brand new world picks up exactly where the save left off.
    let mut h2 = Harness::new();
    h2.world_mut()
        .insert_resource(RoomLibrary::from_defs(vec![bench_room(), shrine()]));
    loaded.apply(h2.world_mut()).unwrap();
    h2.tick_n(3);
    assert_eq!(h2.world().resource::<CurrentRoom>().id, "A");
    let p2 = h2
        .world_mut()
        .query_filtered::<Entity, With<hk_sim::player::Player>>()
        .iter(h2.world())
        .next()
        .unwrap();
    let a = *h2.world().get::<Abilities>(p2).unwrap();
    assert!(a.dash && a.wall_grip);
    assert!(h2
        .world()
        .resource::<WorldFlags>()
        .defeated
        .contains(&0xABCD_E000));
    assert_eq!(
        h2.world().get::<SimPos>(p2).unwrap().0,
        Vec2::new(10.0, 2.0 + 0.75 + SKIN)
    );
    assert_eq!(h2.world().resource::<Checkpoint>().room, "A");
}

#[test]
fn a_save_from_another_version_is_refused_not_misread() {
    let s = SaveData {
        version: SAVE_VERSION + 1,
        room: "A".into(),
        ..SaveData::default()
    };
    let mut h = Harness::new();
    h.world_mut()
        .insert_resource(RoomLibrary::from_defs(vec![room("A")]));
    assert!(s.apply(h.world_mut()).is_err());
    assert!(SaveData::from_ron("(nonsense").is_err());
}

#[test]
fn deaths_and_play_time_are_counted_and_saved() {
    let (mut h, p) = world_of(vec![bench_room()], NONE);
    enter_room(h.world_mut(), "A", "here").unwrap();
    h.tick_n(120);
    assert_eq!(h.world().resource::<RunStats>().deaths, 0);
    let secs = h.world().resource::<RunStats>().seconds();
    assert!(
        (0.9..=1.1).contains(&secs),
        "120 ticks is one second, got {secs}"
    );
    // Die twice (the real damage path sends PlayerDied).
    for _ in 0..2 {
        h.world_mut()
            .resource_mut::<Messages<hk_sim::combat::PlayerDied>>()
            .write(hk_sim::combat::PlayerDied);
        h.tick();
    }
    let _ = p;
    assert_eq!(h.world().resource::<RunStats>().deaths, 2);

    let saved = SaveData::capture(h.world_mut());
    assert_eq!(saved.deaths, 2);
    assert!(saved.play_ticks >= 120);
    let mut h2 = Harness::new();
    h2.world_mut()
        .insert_resource(RoomLibrary::from_defs(vec![bench_room()]));
    SaveData {
        room: "A".into(),
        ..saved.clone()
    }
    .apply(h2.world_mut())
    .unwrap();
    assert_eq!(h2.world().resource::<RunStats>().deaths, 2);
    assert_eq!(h2.world().resource::<RunStats>().ticks, saved.play_ticks);
}
