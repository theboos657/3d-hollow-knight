//! Hollow Knight 3D — game binary. Rendering, camera, audio and UI live here;
//! all gameplay decisions live in `hk_sim`.

mod debug;
mod devices;
mod interp;
mod scene;

use bevy::prelude::*;
use bevy::window::PresentMode;
use hk_sim::{SimPlugin, TICK_HZ};

fn main() {
    let smoke = std::env::args().any(|a| a == "--smoke-test");

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Hollow Knight 3D".into(),
            present_mode: PresentMode::AutoVsync,
            resolution: (1280, 720).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
    .insert_resource(ClearColor(Color::srgb(0.015, 0.02, 0.035)))
    .add_plugins((
        SimPlugin,
        interp::InterpPlugin,
        scene::ScenePlugin,
        debug::DebugPlugin { smoke },
    ));

    // A smoke run is scripted (see debug.rs); real devices would fight it.
    if !smoke {
        app.add_plugins(devices::DevicePlugin);
    }

    app.run();
}
