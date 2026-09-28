//! Room format, validation, building, and the exit transition.

mod common;

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use common::*;
use hk_sim::combat::{HitKind, Hitbox, SpawnTag};
use hk_sim::components::SimPos;
use hk_sim::enemy::Enemy;
use hk_sim::input::Action;
use hk_sim::player::spawn_player;
use hk_sim::testing::Harness;
use hk_sim::world::room::*;
use hk_sim::world::SKIN;

const AIR: &str = "....................";
const FLOOR: &str = "####################";

fn rows_flat() -> Vec<String> {
    let mut v = vec![AIR.to_string(); 8];
    v.push(FLOOR.to_string());
    v.push(FLOOR.to_string());
    v
}

fn def(id: &str) -> RoomDef {
    RoomDef {
        id: id.into(),
        name: id.into(),
        theme: Theme::Ashen,
        tiles: rows_flat(),
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

fn err(d: &RoomDef) -> String {
    d.validate().expect_err("should be invalid")
}

// ------------------------------------------------------------ the format --

#[test]
fn a_room_survives_a_ron_round_trip() {
    let mut d = def("A1");
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (12.0, 2.0),
        persistent: false,
    });
    d.exits.push(ExitDef {
        rect: (18.0, 2.0, 2.0, 6.0),
        to: "B1".into(),
        entry: "west".into(),
    });
    d.benches.push(BenchDef { at: (5.0, 2.0) });
    d.pickups.push(PickupDef {
        ability: Ability::Dash,
        at: (8.0, 2.0),
    });
    let text = ron::ser::to_string_pretty(&d, ron::ser::PrettyConfig::default()).unwrap();
    let back: RoomDef = ron::from_str(&text).unwrap();
    assert_eq!(d, back);
}

#[test]
fn optional_fields_default_when_omitted() {
    let text = r###"(id: "x", name: "X", tiles: ["..", "##"])"###;
    let d: RoomDef = ron::from_str(text).unwrap();
    assert!(d.spawns.is_empty() && d.exits.is_empty() && d.entries.is_empty());
    assert_eq!(d.theme, Theme::Sandbox);
}

#[test]
fn a_valid_room_validates() {
    def("A1").validate().unwrap();
}

#[test]
fn validation_catches_every_authoring_mistake() {
    let mut d = def("r");
    d.tiles[3].pop();
    assert!(err(&d).contains("row 3 has width 19"), "ragged rows");

    let mut d = def("r");
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (5.0, 0.5),
        persistent: false,
    });
    assert!(err(&d).contains("inside solid rock"), "spawn in the floor");

    let mut d = def("r");
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (50.0, 2.0),
        persistent: false,
    });
    assert!(err(&d).contains("outside the room"));

    let mut d = def("r");
    d.entries[0].at = (10.0, 0.0);
    assert!(err(&d).contains("inside solid rock"), "entry in the floor");

    let mut d = def("r");
    d.entries.push(d.entries[0].clone());
    assert!(err(&d).contains("duplicate entry"));

    let mut d = def("r");
    d.exits.push(ExitDef {
        rect: (9.0, 2.0, 3.0, 5.0),
        to: "q".into(),
        entry: "e".into(),
    });
    assert!(
        err(&d).contains("lies inside the exit"),
        "arrival would bounce straight back"
    );

    let mut d = def("r");
    d.exits.push(ExitDef {
        rect: (2.0, 2.0, 0.0, 5.0),
        to: "q".into(),
        entry: "e".into(),
    });
    assert!(err(&d).contains("empty rect"));
}

#[test]
fn a_library_checks_that_every_exit_leads_somewhere_real() {
    let mut a = def("A");
    a.exits.push(ExitDef {
        rect: (18.0, 2.0, 2.0, 6.0),
        to: "B".into(),
        entry: "here".into(),
    });
    let b = def("B");
    RoomLibrary::from_defs(vec![a.clone(), b.clone()])
        .validate()
        .unwrap();

    let e = RoomLibrary::from_defs(vec![a.clone()])
        .validate()
        .unwrap_err();
    assert!(e.contains("unknown room `B`"), "{e}");

    let mut b2 = b;
    b2.entries[0].name = "elsewhere".into();
    let e = RoomLibrary::from_defs(vec![a, b2]).validate().unwrap_err();
    assert!(e.contains("missing entry `here`"), "{e}");
}

#[test]
fn spawn_tags_are_stable_and_distinct() {
    let (a, b) = (def("A1"), def("A2"));
    assert_eq!(a.spawn_tag(3), def("A1").spawn_tag(3));
    assert_ne!(a.spawn_tag(3), a.spawn_tag(4));
    assert_ne!(
        a.spawn_tag(3),
        b.spawn_tag(3),
        "different rooms never share a tag"
    );
}

/// Every shipped room must be valid and every exit must lead somewhere real.
#[test]
fn the_shipped_rooms_all_validate_and_link_up() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/rooms");
    let lib = RoomLibrary::load_dir(std::path::Path::new(dir)).expect("rooms load");
    assert!(lib.get("sandbox").is_some(), "rooms found: {:?}", lib.ids());
    lib.validate().unwrap();
}

// ---------------------------------------------------------- building it --

fn harness_with(rooms: Vec<RoomDef>) -> (Harness, Entity) {
    let mut h = Harness::new();
    h.world_mut().insert_resource(RoomLibrary::from_defs(rooms));
    let p = spawn_player(h.world_mut(), Vec2::new(1.0, 5.0), NONE);
    (h, p)
}

#[test]
fn entering_a_room_builds_the_grid_spawns_and_places_the_player() {
    let mut d = def("A1");
    d.tiles[7] = "........^^^.^.......".into(); // two spike runs: 3 wide and 1 wide
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (14.0, 2.0),
        persistent: false,
    });
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Dummy,
        at: (16.0, 2.0),
        persistent: false,
    });
    d.entries.push(EntryDef {
        name: "west".into(),
        at: (3.0, 2.0),
        facing: -1,
    });
    let (mut h, p) = harness_with(vec![d]);
    enter_room(h.world_mut(), "A1", "west").unwrap();

    assert_eq!(h.world().resource::<CurrentRoom>().id, "A1");
    assert_eq!(h.world().resource::<hk_sim::world::TileGrid>().width(), 20);
    let pos = h.world().get::<SimPos>(p).unwrap().0;
    assert_eq!(
        pos,
        Vec2::new(3.0, 2.0 + 0.75 + SKIN),
        "feet on the entry point"
    );
    assert_eq!(h.world().get::<hk_sim::player::Facing>(p).unwrap().0, -1);

    assert_eq!(
        h.world_mut().query::<&Enemy>().iter(h.world()).count(),
        1,
        "one real enemy"
    );
    assert_eq!(
        h.world_mut().query::<&SpawnTag>().iter(h.world()).count(),
        2,
        "plus the dummy"
    );
    let spikes: Vec<f32> = h
        .world_mut()
        .query::<&Hitbox>()
        .iter(h.world())
        .filter(|hb| hb.kind == HitKind::Hazard)
        .map(|hb| hb.half.x * 2.0)
        .collect();
    assert_eq!(spikes.len(), 2, "runs of ^ merge into one hazard each");
    assert!(
        spikes.contains(&3.0) && spikes.contains(&1.0),
        "widths {spikes:?}"
    );
    assert_eq!(h.drain_messages::<RoomEntered>().len(), 1);
}

#[test]
fn leaving_a_room_removes_its_contents_but_not_the_player() {
    let mut a = def("A");
    a.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (14.0, 2.0),
        persistent: false,
    });
    let b = def("B");
    let (mut h, p) = harness_with(vec![a, b]);
    enter_room(h.world_mut(), "A", "here").unwrap();
    assert_eq!(h.world_mut().query::<&Enemy>().iter(h.world()).count(), 1);
    enter_room(h.world_mut(), "B", "here").unwrap();
    assert_eq!(h.world_mut().query::<&Enemy>().iter(h.world()).count(), 0);
    assert!(h.world().get_entity(p).is_ok(), "the player persists");
}

#[test]
fn persistent_spawns_stay_dead_but_ordinary_ones_come_back() {
    let mut d = def("A");
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (14.0, 2.0),
        persistent: true,
    });
    d.spawns.push(SpawnDef {
        kind: SpawnKind::Husk,
        at: (16.0, 2.0),
        persistent: false,
    });
    let tag0 = d.spawn_tag(0);
    let tag1 = d.spawn_tag(1);
    let (mut h, _p) = harness_with(vec![d]);
    h.world_mut()
        .resource_mut::<WorldFlags>()
        .defeated
        .extend([tag0, tag1]);
    enter_room(h.world_mut(), "A", "here").unwrap();
    let tags: Vec<u32> = h
        .world_mut()
        .query::<&SpawnTag>()
        .iter(h.world())
        .map(|t| t.0)
        .collect();
    assert_eq!(
        tags,
        vec![tag1],
        "the persistent one is gone; the ordinary one respawned"
    );
}

#[test]
fn a_bad_room_id_is_an_error_not_a_panic() {
    let (mut h, _p) = harness_with(vec![def("A")]);
    assert!(enter_room(h.world_mut(), "nope", "here")
        .unwrap_err()
        .contains("unknown room"));
    assert!(enter_room(h.world_mut(), "A", "nope")
        .unwrap_err()
        .contains("no entry"));
}

// ------------------------------------------------------------ transition --

fn two_rooms() -> Vec<RoomDef> {
    let mut a = def("A");
    a.entries = vec![EntryDef {
        name: "start".into(),
        at: (10.0, 2.0),
        facing: 1,
    }];
    a.exits.push(ExitDef {
        rect: (18.0, 2.0, 2.0, 6.0),
        to: "B".into(),
        entry: "west".into(),
    });
    let mut b = def("B");
    b.entries = vec![EntryDef {
        name: "west".into(),
        at: (3.0, 2.0),
        facing: 1,
    }];
    b.exits.push(ExitDef {
        rect: (0.0, 2.0, 1.5, 6.0),
        to: "A".into(),
        entry: "start".into(),
    });
    vec![a, b]
}

fn walk_to_the_exit() -> (Harness, Entity, u64) {
    let (mut h, p) = harness_with(two_rooms());
    enter_room(h.world_mut(), "A", "start").unwrap();
    h.tick_n(5);
    h.press(Action::Right);
    let mut guard = 0;
    while !h.world().resource::<Transition>().active() {
        h.tick();
        guard += 1;
        assert!(guard < 300, "never reached the exit");
    }
    let t0 = h.tick_count();
    (h, p, t0)
}

#[test]
fn an_exit_fades_out_swaps_the_room_and_fades_in_over_36_ticks() {
    let (mut h, p, t0) = walk_to_the_exit();
    assert_eq!(h.world().resource::<Transition>().phase, Phase::Out);
    let frozen_x = h.world().get::<SimPos>(p).unwrap().0.x;

    let mut active = 1; // the tick that started it
    let mut fades = vec![h.world().resource::<Transition>().fade()];
    let mut swapped_at = None;
    let mut arrival = Vec2::ZERO;
    for _ in 0..80 {
        h.tick();
        let tr = h.world().resource::<Transition>().clone();
        if !tr.active() {
            break;
        }
        active += 1;
        fades.push(tr.fade());
        if swapped_at.is_none() && h.world().resource::<CurrentRoom>().id == "B" {
            swapped_at = Some(h.tick_count() - t0);
            arrival = h.world().get::<SimPos>(p).unwrap().0;
        }
        if swapped_at.is_none() {
            assert_eq!(
                h.world().get::<SimPos>(p).unwrap().0.x,
                frozen_x,
                "frozen during fade-out"
            );
        }
    }
    assert_eq!(active, 2 * FADE_TICKS, "fade out + fade in");
    assert_eq!(
        swapped_at,
        Some(FADE_TICKS as u64),
        "swap happens at full black"
    );
    let peak = fades.iter().cloned().fold(0.0f32, f32::max);
    assert_eq!(peak, 1.0, "fully black at the swap");
    let at_peak = fades.iter().position(|f| *f == 1.0).unwrap();
    assert!(
        fades[..=at_peak].windows(2).all(|w| w[1] >= w[0]),
        "fade out only gets darker"
    );
    assert!(
        fades[at_peak..].windows(2).all(|w| w[1] <= w[0]),
        "fade in only gets lighter"
    );

    assert_eq!(h.world().resource::<CurrentRoom>().id, "B");
    assert_eq!(
        arrival,
        Vec2::new(3.0, 2.0 + 0.75 + SKIN),
        "arrived exactly at B's west entry"
    );
    let now = h.world().get::<SimPos>(p).unwrap().0;
    assert!(
        (now.x - 3.0).abs() < 0.05,
        "and stays there until the fade-in ends: {now}"
    );
    assert!(h
        .drain_messages::<RoomEntered>()
        .iter()
        .any(|m| m.id == "B"));
}

#[test]
fn arriving_does_not_bounce_you_straight_back() {
    let (mut h, _p, _t0) = walk_to_the_exit();
    h.release(Action::Right);
    h.tick_n(200);
    assert_eq!(h.world().resource::<CurrentRoom>().id, "B");
    assert!(!h.world().resource::<Transition>().active());
}

#[test]
fn a_press_made_during_the_fade_is_not_lost() {
    let (mut h, p, _t0) = walk_to_the_exit();
    h.release(Action::Right);
    h.tick_n(FADE_TICKS + 4); // mid fade, room already swapped or about to be
    h.tick_n(FADE_TICKS - 6);
    h.press(Action::Jump);
    let mut guard = 0;
    while h.world().resource::<Transition>().active() {
        h.tick();
        guard += 1;
        assert!(guard < 60);
    }
    h.tick_n(2);
    assert!(
        h.world().get::<hk_sim::components::Velocity>(p).unwrap().y > 5.0,
        "the jump fired once time resumed"
    );
}

#[test]
fn projectiles_and_enemies_do_not_follow_you_through_an_exit() {
    let (mut h, p) = harness_with(two_rooms());
    enter_room(h.world_mut(), "A", "start").unwrap();
    h.tick_n(5);
    h.world_mut()
        .get_mut::<hk_sim::combat::Soul>(p)
        .unwrap()
        .value = 99;
    h.press(Action::Cast);
    h.tick();
    assert_eq!(
        h.world_mut()
            .query::<&hk_sim::combat::Projectile>()
            .iter(h.world())
            .count(),
        1
    );
    enter_room(h.world_mut(), "B", "west").unwrap();
    assert_eq!(
        h.world_mut()
            .query::<&hk_sim::combat::Projectile>()
            .iter(h.world())
            .count(),
        0
    );
}
