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

/// Arriving in a room should never put you inside an enemy's aggro range: a
/// fair arrival is a chance to look before you are in a fight.
#[test]
fn nothing_is_waiting_on_top_of_a_doorway() {
    let lib = library();
    let t = tuning();
    let aggro_of = |k: SpawnKind| match k {
        SpawnKind::Husk => t.enemies.husk.aggro_radius,
        SpawnKind::Wisp => t.enemies.wisp.aggro_radius,
        SpawnKind::Shieldbearer => t.enemies.shield.aggro_radius,
        SpawnKind::Spitter => t.enemies.spitter.aggro_radius,
        _ => 0.0,
    };
    let mut bad = Vec::new();
    for id in lib.ids() {
        if matches!(id, "sandbox" | "dev_matron" | "dev_bellwarden") {
            continue;
        }
        let d = lib.get(id).unwrap();
        for e in &d.entries {
            for (i, s) in d.spawns.iter().enumerate() {
                if matches!(
                    s.kind,
                    SpawnKind::Dummy | SpawnKind::Matron | SpawnKind::Bellwarden
                ) {
                    continue;
                }
                let (dx, dy) = ((e.at.0 - s.at.0).abs(), (e.at.1 - s.at.1).abs());
                if dx < aggro_of(s.kind) + 1.5 && dy < 4.5 {
                    bad.push(format!(
                        "{id}: {:?} #{i} at ({}, {}) is {dx:.1} tiles from entry `{}`",
                        s.kind, s.at.0, s.at.1, e.name
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "creatures too close to a doorway:\n{}",
        bad.join("\n")
    );
}

/// Ground creatures must be placed on something to stand on (else they drop
/// into a pit or spikes the moment the room loads), and never on spikes.
#[test]
fn ground_creatures_stand_on_solid_ground() {
    use hk_sim::world::grid::Tile;
    let lib = library();
    let mut bad = Vec::new();
    for id in lib.ids() {
        let d = lib.get(id).unwrap();
        let grid = d.grid();
        for (i, s) in d.spawns.iter().enumerate() {
            if !matches!(
                s.kind,
                SpawnKind::Husk | SpawnKind::Shieldbearer | SpawnKind::Spitter
            ) {
                continue;
            }
            let below = grid.get(s.at.0.floor() as i32, (s.at.1 - 0.5).floor() as i32);
            let here = grid.get(s.at.0.floor() as i32, s.at.1.floor() as i32);
            if !matches!(below, Tile::Solid | Tile::OneWay) || here == Tile::Spike {
                bad.push(format!(
                    "{id}: {:?} #{i} at ({}, {}) has {below:?} below it",
                    s.kind, s.at.0, s.at.1
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "floating or misplaced creatures:\n{}",
        bad.join("\n")
    );
}

// -------------------------------------------------------------------- doors --

/// Every door in the world, actually walked through in the real simulation:
/// the fade, the swap, arriving where the destination says, and staying there.
#[test]
fn every_door_leads_where_it_says_and_you_arrive_safely() {
    use bevy_ecs::prelude::*;
    use hk_sim::components::SimPos;
    use hk_sim::player::spawn_player;
    use hk_sim::testing::Harness;
    use hk_sim::world::room::{enter_room, CurrentRoom, Transition};

    let lib = library();
    // Creatures would only get in the way of measuring the doors; bosses stay
    // (a boss room must be safe to walk into).
    let quiet = |mut d: RoomDef| {
        d.spawns
            .retain(|s| matches!(s.kind, SpawnKind::Matron | SpawnKind::Bellwarden));
        d
    };
    let defs: Vec<RoomDef> = lib
        .ids()
        .into_iter()
        .filter(|i| !matches!(*i, "sandbox" | "dev_matron" | "dev_bellwarden"))
        .map(|i| quiet(lib.get(i).unwrap().clone()))
        .collect();
    let mut checked = 0;
    for def in &defs {
        for (xi, exit) in def.exits.iter().enumerate() {
            let mut h = Harness::new();
            h.world_mut().insert_resource(tuning());
            h.world_mut()
                .insert_resource(RoomLibrary::from_defs(defs.clone()));
            let p = spawn_player(h.world_mut(), Vec2::ZERO, BOTH);
            let start = &def.entries[0];
            // Leaving a boss room means the boss is already beaten (its doors
            // are sealed during the fight).
            for (i, s) in def.spawns.iter().enumerate() {
                if s.persistent {
                    h.world_mut()
                        .resource_mut::<WorldFlags>()
                        .defeated
                        .insert(def.spawn_tag(i));
                }
            }
            enter_room(h.world_mut(), &def.id, &start.name).unwrap();
            h.tick_n(3);

            // Walk into the doorway: stand in the middle of the exit's rectangle.
            let (rx, ry, rw, rh) = exit.rect;
            h.world_mut().get_mut::<SimPos>(p).unwrap().0 = Vec2::new(rx + rw * 0.5, ry + rh * 0.5);
            let mut guard = 0;
            while h.world().resource::<CurrentRoom>().id == def.id {
                h.tick();
                guard += 1;
                assert!(
                    guard < 200,
                    "{} exit #{xi} to {} never fired",
                    def.id,
                    exit.to
                );
            }
            let dest = defs.iter().find(|d| d.id == exit.to).unwrap();
            let entry = dest.entry(&exit.entry).unwrap();
            let at = h.world().get::<SimPos>(p).unwrap().0;
            assert!(
                (at.x - entry.at.0).abs() < 0.01,
                "{} -> {}: arrived at x {} not {}",
                def.id,
                exit.to,
                at.x,
                entry.at.0
            );

            // Then live there for a second: no bouncing back, no damage, and
            // not left inside anything.
            h.tick_n(200);
            assert!(!h.world().resource::<Transition>().active());
            assert_eq!(
                h.world().resource::<CurrentRoom>().id,
                exit.to,
                "{} -> {}: bounced straight back out",
                def.id,
                exit.to
            );
            let hp = h.world().get::<hk_sim::combat::Health>(p).unwrap();
            assert_eq!(hp.hp, hp.max, "{} -> {}: hurt on arrival", def.id, exit.to);
            let end = h.world().get::<SimPos>(p).unwrap().0;
            assert!(
                end.y > 0.0 && end.y < dest.height() as f32,
                "{} -> {}: ended up outside the room at {end:?}",
                def.id,
                exit.to
            );
            checked += 1;
        }
    }
    assert_eq!(checked, defs.iter().map(|d| d.exits.len()).sum::<usize>());
    assert!(checked >= 30, "only {checked} doors checked");
}
