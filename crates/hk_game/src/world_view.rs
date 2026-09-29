//! Showing the current room: themed level geometry and lighting, props
//! (exits, benches, pickups), the fade overlay for room changes, starting the
//! game, and the sandbox's convenience respawn.

use bevy::prelude::*;
use hk_sim::combat::EnemyDied;
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::world::progress::{Checkpoint, SaveData};
use hk_sim::world::room::*;

use crate::interp::RenderPrepSet;

/// How the game begins.
#[derive(Resource, Clone)]
pub enum StartMode {
    /// Pick up a saved game at its last bench.
    Continue(SaveData),
    /// A fresh game in the first room, with no abilities.
    New,
    /// `--viewer`: no game at all, just the model stage (see viewer.rs).
    Viewer,
    /// Developer start (`--room ID`): straight into a room. The sandbox (and
    /// `--all`) unlock every move; nothing is saved.
    Dev {
        room: String,
        entry: String,
        all: bool,
    },
}

/// The room a new game starts in.
pub const FIRST_ROOM: (&str, &str) = ("A1", "start");

pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_fade)
            .add_systems(PostStartup, auto_start)
            .add_systems(Update, (update_fade, sandbox_respawn).after(RenderPrepSet));
    }
}

#[derive(Component)]
struct FadeOverlay;

// ---------------------------------------------------------------- starting --

/// Starts a game from the title screen (or straight away in dev runs).
pub fn begin_game(world: &mut World, mode: StartMode) {
    match mode {
        StartMode::Continue(save) => {
            if let Err(e) = save.apply(world) {
                eprintln!("could not continue the saved game ({e}); starting a new one");
                begin(world, FIRST_ROOM.0, FIRST_ROOM.1, Abilities::default());
            }
        }
        StartMode::New => begin(world, FIRST_ROOM.0, FIRST_ROOM.1, Abilities::default()),
        StartMode::Viewer => {}
        StartMode::Dev { room, entry, all } => {
            let sandbox = world
                .resource::<RoomLibrary>()
                .get(&room)
                .is_some_and(|d| d.theme == Theme::Sandbox);
            let abilities = if all || sandbox {
                Abilities {
                    dash: true,
                    wall_grip: true,
                }
            } else {
                Abilities::default()
            };
            begin(world, &room, &entry, abilities);
        }
    }
}

/// Developer and scripted runs skip the title screen and start immediately.
fn auto_start(mut commands: Commands, mode: Res<StartMode>, screen: Res<crate::menu::Screen>) {
    if *screen != crate::menu::Screen::Playing {
        return;
    }
    let mode = mode.clone();
    commands.queue(move |world: &mut World| begin_game(world, mode));
}

/// Spawns the player and enters `room`; that entry point is also where dying
/// brings you back until you find a bench.
fn begin(world: &mut World, room: &str, entry: &str, abilities: Abilities) {
    spawn_player(world, bevy::math::Vec2::ZERO, abilities);
    let spot = world
        .resource::<RoomLibrary>()
        .get(room)
        .and_then(|d| d.entry(entry).cloned());
    if let Some(e) = spot {
        *world.resource_mut::<Checkpoint>() = Checkpoint {
            room: room.to_string(),
            pos: bevy::math::Vec2::new(e.at.0, e.at.1),
            facing: e.facing,
        };
    }
    if let Err(e) = enter_room(world, room, entry) {
        eprintln!("could not enter start room: {e}");
    }
}

// -------------------------------------------------------------------- fade --

fn spawn_fade(mut commands: Commands) {
    commands.spawn((
        FadeOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        GlobalZIndex(100),
    ));
}

fn update_fade(tr: Res<Transition>, mut q: Query<&mut BackgroundColor, With<FadeOverlay>>) {
    for mut bg in &mut q {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, tr.fade());
    }
}

// --------------------------------------------------------------- sandbox --

/// Only in the sandbox: anything killed comes back after a few seconds.
fn sandbox_respawn(
    time: Res<Time>,
    library: Res<RoomLibrary>,
    current: Res<CurrentRoom>,
    mut died: MessageReader<EnemyDied>,
    mut pending: Local<Vec<(f32, u32)>>,
    mut commands: Commands,
) {
    let sandbox = library
        .get(&current.id)
        .is_some_and(|d| d.theme == Theme::Sandbox);
    if !sandbox {
        pending.clear();
        died.clear();
        return;
    }
    for d in died.read() {
        if let Some(tag) = d.tag {
            pending.push((4.0, tag));
        }
    }
    let dt = time.delta_secs();
    let Some(def) = library.get(&current.id).cloned() else {
        return;
    };
    pending.retain_mut(|(t, tag)| {
        *t -= dt;
        if *t > 0.0 {
            return true;
        }
        if let Some(i) = (0..def.spawns.len()).find(|i| def.spawn_tag(*i) == *tag) {
            let def = def.clone();
            commands.queue(move |world: &mut World| {
                spawn_from_def(world, &def, i);
            });
        }
        false
    });
}
