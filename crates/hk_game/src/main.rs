//! Hollow Knight 3D — game binary. Rendering, camera, audio and UI live here;
//! all gameplay decisions live in `hk_sim`.
//!
//! Options: `--room ID` / `--entry NAME` (developer start, nothing is saved; add
//! `--all` to unlock every move), `--new` (ignore the save),
//! `--smoke-test` (scripted headless run that saves `out/smoke.png`),
//! `--bot` (the boss-fight bot plays), `--shots moment,...` and
//! `--boss-hp-pct N` (see demo.rs).

mod assets;
mod audio;
mod boss_view;
mod camera_rig;
mod debug;
mod demo;
mod devices;
mod hud;
mod interp;
mod menu;
mod models;
mod rig;
mod save_io;
mod scene;
mod settings;
mod toast;
mod vfx;
mod viewer;
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
    let bot = args.iter().any(|a| a == "--bot");
    let shots: Vec<String> = value_of("--shots")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let boss_hp_pct = value_of("--boss-hp-pct").and_then(|v| v.parse::<f32>().ok());
    let viewer = args.iter().any(|a| a == "--viewer");
    let dev_room = value_of("--room");
    let assets_dir = assets::find_assets_dir();
    let save_dir = save_io::save_dir(&assets_dir);
    // A developer start (--room) never touches the save; otherwise continue
    // the saved game unless --new asks for a fresh one.
    // Scripted runs (smoke test, bot demo, screenshots) never read or write a save.
    let scripted = smoke || bot || !shots.is_empty() || viewer;
    let start = match dev_room {
        _ if viewer => world_view::StartMode::Viewer,
        Some(room) => world_view::StartMode::Dev {
            room,
            entry: value_of("--entry").unwrap_or_else(|| "start".into()),
            all: args.iter().any(|a| a == "--all"),
        },
        None => match (
            scripted || args.iter().any(|a| a == "--new"),
            save_io::load(&save_dir),
        ) {
            (false, Some(save)) => world_view::StartMode::Continue(save),
            _ => world_view::StartMode::New,
        },
    };
    let dev_start = matches!(start, world_view::StartMode::Dev { .. });
    let autosave = !scripted && !dev_start;
    // Developer and scripted runs go straight into the game; everyone else
    // gets the title screen.
    let skip_title = (scripted || dev_start) && !args.iter().any(|a| a == "--title");
    let settings = settings::load(&save_dir);

    // Tuning and rooms are plain files: edit and restart, no rebuild needed.
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
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Hollow Toll".into(),
                    present_mode: if settings.vsync {
                        PresentMode::AutoVsync
                    } else {
                        PresentMode::AutoNoVsync
                    },
                    resolution: (1280, 720).into(),
                    ..default()
                }),
                ..default()
            })
            // Sounds live next to the tuning and rooms.
            .set(AssetPlugin {
                file_path: assets_dir.to_string_lossy().into_owned(),
                ..default()
            }),
    )
    .insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
    .insert_resource(ClearColor(Color::srgb(0.015, 0.02, 0.035)))
    // Inserted before SimPlugin so its `init_resource` keeps ours.
    .insert_resource(tuning)
    .insert_resource(library)
    .insert_resource(start)
    .insert_resource(settings)
    .add_plugins((
        SimPlugin,
        interp::InterpPlugin,
        scene::ScenePlugin,
        world_view::WorldViewPlugin,
        visuals::VisualsPlugin,
        camera_rig::CameraRigPlugin,
        (
            vfx::VfxPlugin,
            models::knight::KnightPlugin,
            models::enemies::EnemyModelsPlugin,
        ),
        boss_view::BossViewPlugin,
        toast::ToastPlugin,
        hud::HudPlugin,
        audio::AudioPlugin,
        menu::MenuPlugin {
            skip_title,
            ignore_save: args.iter().any(|a| a == "--new"),
        },
        save_io::SavePlugin {
            enabled: autosave,
            dir: save_dir,
        },
        debug::DebugPlugin { smoke },
        demo::DemoPlugin {
            prefix: value_of("--shot-prefix").unwrap_or_default(),
            bot,
            shots,
            boss_hp_pct,
        },
    ));

    // A smoke run is scripted (see debug.rs); real devices would fight it.
    if viewer {
        app.add_plugins(viewer::ViewerPlugin {
            set: viewer::ViewerSet::parse(value_of("--viewer-set").as_deref()),
            columns: value_of("--viewer-cols").map(|v| {
                v.split(',')
                    .filter_map(|c| c.trim().parse::<usize>().ok())
                    .collect()
            }),
        });
    }
    if args.iter().any(|a| a == "--show-hitboxes") {
        app.insert_resource(visuals::ShowHitboxes(true));
    }
    if !smoke && !bot {
        app.add_plugins(devices::DevicePlugin);
    }

    app.run();
}
