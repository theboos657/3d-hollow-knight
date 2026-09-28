//! The shipped world: every room must be reachable in the intended order, and
//! the ability gates must really gate. (The analysis runs the real player
//! controller, so this is a statement about the physics, not a sketch.)

mod common;

use bevy_math::Vec2;
use common::*;
use hk_sim::player::Abilities;
use hk_sim::reach::*;
use hk_sim::tuning::Tuning;
use hk_sim::world::room::*;

const BOTH: Abilities = Abilities {
    dash: true,
    wall_grip: true,
};

fn library() -> RoomLibrary {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/rooms");
    RoomLibrary::load_dir(std::path::Path::new(dir)).expect("rooms load")
}

fn tuning() -> Tuning {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/tuning");
    Tuning::load_dir(std::path::Path::new(dir)).0
}

fn from_entry(lib: &RoomLibrary, room: &str, entry: &str, abil: Abilities) -> RoomReach {
    let def = lib.get(room).unwrap_or_else(|| panic!("no room {room}"));
    let e = def
        .entry(entry)
        .unwrap_or_else(|| panic!("no entry {entry}"));
    analyse_room(def, &tuning(), abil, Vec2::new(e.at.0, e.at.1))
}

fn exit_to(lib: &RoomLibrary, room: &str, to: &str) -> usize {
    lib.get(room)
        .unwrap()
        .exits
        .iter()
        .position(|x| x.to == to)
        .unwrap_or_else(|| panic!("{room} has no exit to {to}"))
}

// ---------------------------------------------------------------- the gates --

#[test]
fn the_flooded_walk_needs_dash() {
    let lib = library();
    let east = exit_to(&lib, "C1", "C2");
    assert!(!from_entry(&lib, "C1", "top", NONE).exits[east]);
    assert!(!from_entry(&lib, "C1", "top", GRIP).exits[east]);
    assert!(from_entry(&lib, "C1", "top", DASH).exits[east]);
}

#[test]
fn the_bell_shaft_needs_dash_to_start_climbing() {
    let lib = library();
    let top = exit_to(&lib, "C3", "C4");
    assert!(!from_entry(&lib, "C3", "west", NONE).exits[top]);
    // (Wall Grip alone could climb the outer wall, but the only way into C3 is
    // through C1's dash gap, so the two are never met apart.)
    assert!(from_entry(&lib, "C3", "west", DASH).exits[top]);
}

#[test]
fn the_grip_shrine_holds_its_prize_and_needs_it_to_leave() {
    let lib = library();
    let out = exit_to(&lib, "C4", "C3");
    let with_dash = from_entry(&lib, "C4", "top", DASH);
    assert!(
        with_dash.pickups[0],
        "the pickup is reachable with Dash alone"
    );
    assert!(
        with_dash.trap_count > 0,
        "until you have it, the chamber floor is a one-way trip"
    );
    let with_grip = from_entry(&lib, "C4", "top", BOTH);
    assert!(with_grip.exits[out], "with Wall Grip you climb out");
    assert_eq!(with_grip.trap_count, 0);
}

#[test]
fn the_ascent_needs_wall_grip() {
    let lib = library();
    let top = exit_to(&lib, "D1", "D2");
    assert!(!from_entry(&lib, "D1", "bottom", NONE).exits[top]);
    assert!(!from_entry(&lib, "D1", "bottom", DASH).exits[top]);
    assert!(from_entry(&lib, "D1", "bottom", GRIP).exits[top]);
}

#[test]
fn the_gauntlet_needs_both_abilities() {
    let lib = library();
    let east = exit_to(&lib, "D3", "D4");
    assert!(
        !from_entry(&lib, "D3", "west", DASH).exits[east],
        "the wall stops Dash alone"
    );
    assert!(
        !from_entry(&lib, "D3", "west", GRIP).exits[east],
        "the spikes stop Grip alone"
    );
    assert!(from_entry(&lib, "D3", "west", BOTH).exits[east]);
}

#[test]
fn the_early_game_needs_no_abilities() {
    let lib = library();
    for (room, entry, to) in [
        ("A1", "start", "A2"),
        ("A2", "top", "A3"),
        ("A3", "west", "A4"),
        ("A4", "west", "B1"),
        ("B1", "west", "B2"),
        ("B2", "west", "B3"),
        ("B3", "west", "B4"),
    ] {
        let r = from_entry(&lib, room, entry, NONE);
        assert!(
            r.exits[exit_to(&lib, room, to)],
            "{room} -> {to} with no abilities"
        );
        assert_eq!(r.trap_count, 0, "{room} has a softlock: {:?}", r.traps);
    }
}

// ------------------------------------------------------------- the whole map --

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "slow unoptimised: cargo test --release -p hk_sim --test world"
)]
fn the_world_can_be_completed_in_the_intended_order() {
    let lib = library();
    let rep = analyse_world(
        &lib,
        &tuning(),
        ("A1", "start"),
        &["sandbox", "dev_matron", "dev_bellwarden"],
    )
    .unwrap();
    assert!(
        rep.is_clean(),
        "unreachable {:?}, dead exits {:?}, dead pickups {:?}, dead benches {:?}, softlocks {:?}",
        rep.unreachable,
        rep.dead_exits,
        rep.dead_pickups,
        rep.dead_benches,
        rep.traps
    );
    assert_eq!(
        rep.stages.len(),
        3,
        "no abilities, then Dash, then Dash + Wall Grip"
    );
    assert_eq!(
        rep.stages[0].gained,
        vec![Ability::Dash],
        "the Matron teaches Dash"
    );
    assert_eq!(
        rep.stages[1].gained,
        vec![Ability::WallGrip],
        "the shrine teaches Wall Grip"
    );
    assert!(rep.stages[2].gained.is_empty());
    for r in ["A1", "A2", "A3", "A4", "B1", "B2", "B3", "B4"] {
        assert_eq!(rep.stage_of(r), Some(0), "{r} is open from the start");
    }
    for r in ["C2", "C3", "C4"] {
        assert_eq!(rep.stage_of(r), Some(1), "{r} opens with Dash");
    }
    for r in ["D2", "D3", "D4"] {
        assert_eq!(rep.stage_of(r), Some(2), "{r} opens with Wall Grip");
    }
    assert_eq!(rep.stage_of("D4"), Some(2), "the throne is the last stage");
}

#[test]
fn every_room_has_a_way_back_and_benches_are_where_the_fights_are() {
    let lib = library();
    let ids: Vec<&str> = lib
        .ids()
        .into_iter()
        .filter(|i| !matches!(*i, "sandbox" | "dev_matron" | "dev_bellwarden"))
        .collect();
    assert_eq!(ids.len(), 16, "the world has 16 rooms: {ids:?}");
    // Both boss arenas are dead ends whose only exit leads back.
    for boss_room in ["B4", "D4"] {
        let d = lib.get(boss_room).unwrap();
        assert_eq!(d.exits.len(), 1, "{boss_room} has exactly one door");
        assert!(
            d.spawns.iter().any(|s| s.persistent),
            "{boss_room}'s boss stays dead once defeated"
        );
        // Floor and ceiling one tile thick, so the boss arena fits the room.
        assert_eq!(
            d.tiles
                .first()
                .unwrap()
                .chars()
                .filter(|c| *c == '#')
                .count(),
            d.width() as usize
        );
    }
    // A bench close to each boss so retries are quick.
    for room in ["B3", "D3"] {
        assert!(
            !lib.get(room).unwrap().benches.is_empty(),
            "{room} has a bench"
        );
    }
}
