//! Helpers shared by the integration tests.
#![allow(dead_code)]

use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use hk_sim::components::{SimPos, Velocity};
use hk_sim::player::{spawn_player, Abilities, Motor, PlayerState};
use hk_sim::testing::Harness;
use hk_sim::tuning::PlayerTuning;
use hk_sim::world::{TileGrid, SKIN};

pub const REST_Y: f32 = 2.0 + 0.75 + SKIN; // floor top + half height + skin

pub fn t() -> PlayerTuning {
    PlayerTuning::default()
}

pub fn scene(rows: &[&str], start: Vec2, abil: Abilities) -> (Harness, Entity) {
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(rows));
    let e = spawn_player(h.world_mut(), start, abil);
    (h, e)
}

pub fn pos(h: &Harness, e: Entity) -> Vec2 {
    h.world().get::<SimPos>(e).unwrap().0
}
pub fn vel(h: &Harness, e: Entity) -> Vec2 {
    h.world().get::<Velocity>(e).unwrap().0
}
pub fn motor(h: &Harness, e: Entity) -> Motor {
    h.world().get::<Motor>(e).unwrap().clone()
}
pub fn state(h: &Harness, e: Entity) -> PlayerState {
    *h.world().get::<PlayerState>(e).unwrap()
}

/// Flat arena: floor top at y = 2, 60 wide, open above.
pub fn flat() -> Vec<&'static str> {
    const AIR: &str = "............................................................";
    const FLOOR: &str = "############################################################";
    let mut v = vec![AIR; 12];
    v.push(FLOOR);
    v.push(FLOOR);
    v
}

pub fn flat_scene(abil: Abilities) -> (Harness, Entity) {
    let rows = flat();
    let (mut h, e) = scene(&rows, Vec2::new(10.0, REST_Y), abil);
    h.tick_n(5); // settle
    assert!(
        motor(&h, e).grounded,
        "player should start settled on the floor"
    );
    (h, e)
}

pub const NONE: Abilities = Abilities {
    dash: false,
    wall_grip: false,
};
pub const DASH: Abilities = Abilities {
    dash: true,
    wall_grip: false,
};
pub const GRIP: Abilities = Abilities {
    dash: false,
    wall_grip: true,
};
