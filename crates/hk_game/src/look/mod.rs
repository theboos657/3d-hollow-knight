//! How the world looks: the level's stone, the chamber behind it, the light
//! that falls on it and the fires in it. Everything here reads the simulation's
//! room and never feeds back into it.

pub mod fixtures;
pub mod kits;
pub mod level;
pub mod props;
pub mod room;
pub mod style;
pub mod texture;

use bevy::prelude::*;

use crate::interp::RenderPrepSet;

/// Everything spawned for the current room (torn down when the room changes).
#[derive(Component, Clone, Copy)]
pub struct RoomVisual;

/// The main directional light (the one that casts shadows).
#[derive(Component)]
pub struct KeyLight;

/// A weak opposite light that lifts silhouettes off the wall.
#[derive(Component)]
pub struct RimLight;

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                room::rebuild_room,
                props::flicker,
                fixtures::attach_fixtures,
                fixtures::attach_spikes,
                fixtures::animate_fixtures,
            )
                .after(RenderPrepSet),
        );
    }
}
