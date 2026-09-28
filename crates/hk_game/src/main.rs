//! Hollow Knight 3D — game binary. Rendering, camera, audio and UI live here;
//! all gameplay decisions live in `hk_sim`.
//!
//! Options: `--room ID` (default `sandbox`), `--entry NAME` (default `start`),
//! `--smoke-test` (scripted headless run that saves `out/smoke.png`).

mod assets;
mod camera_rig;
mod debug;
mod devices;
mod interp;
mod scene;
mod vfx;
mod visuals;
mod world_view;

use bevy::prelude::*;
use bevy::window::PresentMode;
use hk_sim::tuning::Tuning;
use hk_sim::world::room::RoomLibrary;
use hk_sim::{SimPlugin, TICK_HZ};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let smoke = args.iter().any(|a| a == "--smoke-test");
    let value_of = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let start = world_view::StartRoom {
        room: value_of("--room").unwrap_or_else(|| "sandbox".into()),
        entry: value_of("--entry").unwrap_or_else(|| "start".into()),
    };

    // Tuning and rooms are plain files: edit and restart, no rebuild needed.
    let assets_dir = assets::find_assets_dir();
    let (tuning, warnings) = Tuning::load_dir(&assets_dir.join("tuning"));
    for w in warnings {
        eprintln!("tuning warning: {w}");
    }
    let library = match RoomLibrary::load_dir(&assets_dir.join("rooms")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not load rooms: {e}");
            RoomLibrary::default()
        }
    };
    if let Err(e) = library.validate() {
        eprintln!("room warning: {e}");
    }

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
    // Inserted before SimPlugin so its `init_resource` keeps ours.
    .insert_resource(tuning)
    .insert_resource(library)
    .insert_resource(start)
    .add_plugins((
        SimPlugin,
        interp::InterpPlugin,
        scene::ScenePlugin,
        world_view::WorldViewPlugin,
        visuals::VisualsPlugin,
        camera_rig::CameraRigPlugin,
        vfx::VfxPlugin,
        debug::DebugPlugin { smoke },
    ));

    // A smoke run is scripted (see debug.rs); real devices would fight it.
    if !smoke {
        app.add_plugins(devices::DevicePlugin);
    }

    app.run();
}
