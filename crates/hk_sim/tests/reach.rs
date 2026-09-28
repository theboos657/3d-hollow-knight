//! The reachability tool: does it agree with the physics? Small synthetic
//! rooms with known answers (gaps, walls, ledges, pits, spikes), then a tiny
//! three-room world played out stage by stage.

mod common;

use bevy_math::Vec2;
use common::*;
use hk_sim::player::Abilities;
use hk_sim::reach::*;
use hk_sim::tuning::Tuning;
use hk_sim::world::room::*;

/// A room built cell by cell; `y` counts up from the bottom row.
struct B {
    w: usize,
    h: usize,
    cells: Vec<Vec<char>>,
}

impl B {
    /// Walls on both sides, a two-tile floor (top at y = 2), open above.
    fn new(w: usize, h: usize) -> Self {
        let mut b = B {
            w,
            h,
            cells: vec![vec!['.'; w]; h],
        };
        b.fill(0, 0, w, 2, '#');
        b.fill(0, 0, 1, h, '#');
        b.fill(w - 1, 0, 1, h, '#');
        b.fill(0, h - 1, w, 1, '#');
        b
    }

    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: char) -> &mut Self {
        for j in y..(y + h).min(self.h) {
            for i in x..(x + w).min(self.w) {
                self.cells[j][i] = c;
            }
        }
        self
    }

    fn def(&self, id: &str) -> RoomDef {
        let tiles: Vec<String> = self
            .cells
            .iter()
            .rev()
            .map(|r| r.iter().collect::<String>())
            .collect();
        RoomDef {
            id: id.into(),
            name: id.into(),
            theme: Theme::Ashen,
            tiles,
            spawns: vec![],
            entries: vec![EntryDef {
                name: "start".into(),
                at: (3.0, 2.0),
                facing: 1,
            }],
            exits: vec![],
            benches: vec![],
            pickups: vec![],
        }
    }
}

fn tuning() -> Tuning {
    Tuning::default()
}

fn exit_at(x: f32, y: f32) -> ExitDef {
    ExitDef {
        rect: (x, y, 2.0, 5.0),
        to: "nowhere".into(),
        entry: "x".into(),
    }
}

fn reach(def: &RoomDef, abil: Abilities) -> RoomReach {
    analyse_room(def, &tuning(), abil, Vec2::new(3.0, 2.0))
}

/// Two floors with a gap of `gap` tiles between; an exit on the far side.
fn gap_room(gap: usize) -> RoomDef {
    let w = 30 + gap;
    let mut b = B::new(w, 14);
    b.fill(12, 0, gap, 2, '.');
    let mut d = b.def("gap");
    d.exits.push(exit_at((w - 4) as f32, 2.0));
    d
}

fn crosses(gap: usize, abil: Abilities) -> bool {
    reach(&gap_room(gap), abil).exits[0]
}

fn widest_gap(abil: Abilities) -> usize {
    (2..24)
        .take_while(|&g| crosses(g, abil))
        .last()
        .unwrap_or(0)
}

#[test]
fn a_plain_jump_clears_ordinary_gaps_and_dash_clears_wider_ones() {
    assert!(crosses(3, NONE), "an easy gap");
    let plain = widest_gap(NONE);
    let dash = widest_gap(DASH);
    eprintln!("widest gap: plain jump {plain} tiles, with dash {dash} tiles");
    assert!(
        (5..=9).contains(&plain),
        "a running jump should clear 5-9 tiles, got {plain}"
    );
    assert!(
        dash >= plain + 3,
        "dash should add a real distance: {plain} -> {dash}"
    );
    // And the boundary is a boundary: nothing wider than it is crossed.
    assert!(!crosses(plain + 2, NONE));
    assert!(!crosses(dash + 2, DASH));
}

#[test]
fn wall_grip_climbs_a_wall_nothing_else_can() {
    let mut b = B::new(34, 30);
    b.fill(14, 0, 20, 16, '#'); // a 14-tile cliff (top at y = 16)
    let mut d = b.def("cliff");
    d.exits.push(exit_at(30.0, 16.0));
    assert!(!reach(&d, NONE).exits[0], "cannot jump 14 tiles");
    assert!(!reach(&d, DASH).exits[0], "dash does not climb");
    assert!(reach(&d, GRIP).exits[0], "wall jumps do");
}

#[test]
fn ledges_are_reachable_by_height() {
    for (height, reachable) in [(2usize, true), (3, true), (5, false), (7, false)] {
        let mut b = B::new(30, 16);
        b.fill(14, 0, 16, 2 + height, '#');
        let mut d = b.def("ledge");
        d.exits.push(exit_at(26.0, (2 + height) as f32));
        assert_eq!(
            reach(&d, NONE).exits[0],
            reachable,
            "a {height}-tile ledge, no abilities"
        );
    }
}

#[test]
fn spikes_are_never_somewhere_to_stand() {
    let mut b = B::new(34, 14);
    b.fill(12, 1, 6, 1, '^'); // a spike bed sunk into the floor
    b.fill(12, 0, 6, 1, '#');
    b.fill(12, 1, 6, 1, '^');
    let mut d = b.def("spikes");
    d.exits.push(exit_at(30.0, 2.0));
    let r = reach(&d, NONE);
    assert!(r.exits[0], "you can jump the spikes");
    assert!(
        !r.nodes
            .iter()
            .any(|n| n.feet.x > 12.4 && n.feet.x < 17.6 && n.feet.y < 2.0),
        "no standing spot inside the spikes"
    );
}

#[test]
fn a_pit_with_no_way_out_is_a_softlock_and_stairs_are_not() {
    // High ground on both sides with a 6-deep pit between (x = 14..20).
    let make = |stairs: bool| {
        let mut b = B::new(40, 16);
        b.fill(1, 2, 13, 6, '#');
        b.fill(20, 2, 19, 6, '#');
        if stairs {
            // A step in the pit, low enough to hop, then the high ground.
            b.fill(18, 2, 2, 3, '#');
        }
        let mut d = b.def("pit");
        d.entries[0].at = (5.0, 8.0);
        d.exits.push(exit_at(35.0, 8.0));
        d
    };
    let start = Vec2::new(5.0, 8.0);
    let trapped = analyse_room(&make(false), &tuning(), NONE, start);
    assert!(trapped.exits[0], "you can cross above the pit");
    assert!(trapped.trap_count > 0, "but falling in is a softlock");
    assert!(
        trapped
            .traps
            .iter()
            .all(|t| t.x > 13.0 && t.x < 20.0 && t.y < 3.0),
        "the trap is the pit floor: {:?}",
        trapped.traps
    );
    let stepped = analyse_room(&make(true), &tuning(), NONE, start);
    assert_eq!(stepped.trap_count, 0, "with a step you can climb out");
}

#[test]
fn one_way_platforms_can_be_jumped_up_through_and_dropped_through() {
    let mut b = B::new(30, 16);
    b.fill(12, 5, 8, 1, '='); // 3 tiles above the floor
    b.fill(12, 9, 8, 1, '='); // and another 4 above that
    let mut d = b.def("oneway");
    d.exits.push(exit_at(15.0, 6.0)); // rests on the first platform's top
    let r = reach(&d, NONE);
    assert!(r.exits[0], "up through the underside");
    assert_eq!(r.trap_count, 0, "and back down again");
}

#[test]
fn pickups_and_benches_report_whether_they_can_be_touched() {
    let mut b = B::new(30, 16);
    b.fill(14, 0, 16, 9, '#'); // a 7-tile cliff
    let mut d = b.def("shrine");
    d.pickups.push(PickupDef {
        ability: Ability::Dash,
        at: (6.0, 2.0),
    });
    d.pickups.push(PickupDef {
        ability: Ability::WallGrip,
        at: (20.0, 9.0),
    });
    d.benches.push(BenchDef { at: (8.0, 2.0) });
    d.benches.push(BenchDef { at: (24.0, 9.0) });
    let r = reach(&d, NONE);
    assert_eq!(r.pickups, vec![true, false]);
    assert_eq!(r.benches, vec![true, false]);
}

#[test]
fn analysis_is_deterministic() {
    let d = gap_room(6);
    let a = reach(&d, DASH);
    let b = reach(&d, DASH);
    assert_eq!(a.nodes.len(), b.nodes.len());
    assert_eq!(a.exits, b.exits);
}

// ------------------------------------------------------------------- world --

/// R1 (dash pickup, exit east) -> R2 (a gap only dash can cross) -> R3.
/// R4 has no way in at all.
fn tiny_world() -> RoomLibrary {
    let mut r1 = B::new(30, 14).def("R1");
    r1.pickups.push(PickupDef {
        ability: Ability::Dash,
        at: (6.0, 2.0),
    });
    r1.exits.push(ExitDef {
        rect: (27.0, 2.0, 2.0, 5.0),
        to: "R2".into(),
        entry: "west".into(),
    });
    r1.entries[0].name = "start".into();

    let mut b2 = B::new(48, 14);
    b2.fill(12, 0, 9, 2, '.'); // a 9-wide chasm: beyond a plain jump, within a dash
    let mut r2 = b2.def("R2");
    r2.entries[0].name = "west".into();
    r2.exits.push(ExitDef {
        rect: (45.0, 2.0, 2.0, 5.0),
        to: "R3".into(),
        entry: "west".into(),
    });

    let mut r3 = B::new(30, 14).def("R3");
    r3.entries[0].name = "west".into();

    let mut r4 = B::new(30, 14).def("R4");
    r4.entries[0].name = "west".into();
    RoomLibrary::from_defs(vec![r1, r2, r3, r4])
}

#[test]
fn the_world_unfolds_in_stages_as_abilities_are_found() {
    let lib = tiny_world();
    let rep = analyse_world(&lib, &tuning(), ("R1", "start"), &[]).unwrap();
    assert_eq!(rep.stage_of("R1"), Some(0));
    assert_eq!(
        rep.stage_of("R2"),
        Some(0),
        "the door to R2 is open from the start"
    );
    assert_eq!(
        rep.stage_of("R3"),
        Some(1),
        "R3 is behind a gap only dash crosses"
    );
    assert_eq!(rep.stages.len(), 2, "the second stage finds nothing more");
    assert_eq!(rep.stages[0].gained, vec![Ability::Dash]);
    assert!(rep.stages[1].abilities.dash);
    assert_eq!(rep.unreachable, vec!["R4".to_string()], "R4 has no way in");
    assert!(!rep.is_clean());
}

#[test]
fn ignoring_a_room_removes_it_from_the_unreachable_list() {
    let lib = tiny_world();
    let rep = analyse_world(&lib, &tuning(), ("R1", "start"), &["R4"]).unwrap();
    assert!(rep.unreachable.is_empty());
}

#[test]
fn a_bad_start_is_an_error_not_a_panic() {
    let lib = tiny_world();
    assert!(analyse_world(&lib, &tuning(), ("nope", "start"), &[]).is_err());
    assert!(analyse_world(&lib, &tuning(), ("R1", "nope"), &[]).is_err());
}

/// A spike bed of `width` tiles on the floor, with an exit beyond it.
fn spike_room(width: usize) -> RoomDef {
    let w = 30 + width;
    let mut b = B::new(w, 14);
    b.fill(12, 2, width, 1, '^');
    let mut d = b.def("spikebed");
    d.exits.push(exit_at((w - 4) as f32, 2.0));
    d
}

#[test]
fn spike_beds_are_wider_to_cross_than_holes_of_the_same_size() {
    let widest = |abil: Abilities| {
        (2..24)
            .take_while(|&g| reach(&spike_room(g), abil).exits[0])
            .last()
            .unwrap_or(0)
    };
    let plain = widest(NONE);
    let dash = widest(DASH);
    eprintln!("widest spike bed: plain jump {plain} tiles, with dash {dash} tiles");
    // The world's dash gates are 6-tile spike beds (C1, D3): too wide for a
    // plain jump, but within a dash's reach. If the physics is retuned, this
    // is the test that says the level design needs revisiting.
    assert!(
        plain < 6,
        "a plain jump must not clear 6 spike tiles: {plain}"
    );
    assert!(dash >= 6, "a dash must clear 6 spike tiles: {dash}");
}
