//! Showing the current room: themed level geometry and lighting, props
//! (exits, benches, pickups), the fade overlay for room changes, starting the
//! game, and the sandbox's convenience respawn.

use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use hk_sim::combat::EnemyDied;
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::world::grid::Tile;
use hk_sim::world::room::*;

use crate::interp::RenderPrepSet;
use crate::scene::{spawn_backdrop, MainCamera};

/// Which room to start in (`--room ID`, default `sandbox`).
#[derive(Resource, Clone)]
pub struct StartRoom {
    pub room: String,
    pub entry: String,
}

pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_fade)
            .add_systems(PostStartup, start_game)
            .add_systems(
                Update,
                (rebuild_view, attach_props, update_fade, sandbox_respawn).after(RenderPrepSet),
            );
    }
}

#[derive(Component, Clone, Copy)]
struct RoomVisual;

#[derive(Component)]
struct FadeOverlay;

// ------------------------------------------------------------------ themes --

struct ThemeStyle {
    ambient: Color,
    brightness: f32,
    fog: Color,
    stone: Color,
    one_way: Color,
    backdrop: Color,
    glow: Color,
    glow_emissive: LinearRgba,
}

fn style(t: Theme) -> ThemeStyle {
    let c = Color::srgb;
    let e = LinearRgba::rgb;
    match t {
        Theme::Sandbox => ThemeStyle {
            ambient: c(0.4, 0.46, 0.65),
            brightness: 220.0,
            fog: c(0.015, 0.02, 0.035),
            stone: c(0.22, 0.25, 0.33),
            one_way: c(0.35, 0.30, 0.20),
            backdrop: c(0.10, 0.12, 0.20),
            glow: c(0.2, 0.6, 0.9),
            glow_emissive: e(0.6, 2.4, 4.0),
        },
        Theme::Ashen => ThemeStyle {
            ambient: c(0.6, 0.5, 0.45),
            brightness: 200.0,
            fog: c(0.03, 0.025, 0.025),
            stone: c(0.28, 0.26, 0.25),
            one_way: c(0.4, 0.33, 0.25),
            backdrop: c(0.13, 0.12, 0.12),
            glow: c(0.9, 0.6, 0.3),
            glow_emissive: e(3.0, 1.6, 0.5),
        },
        Theme::Warrens => ThemeStyle {
            ambient: c(0.4, 0.6, 0.5),
            brightness: 200.0,
            fog: c(0.01, 0.03, 0.02),
            stone: c(0.2, 0.27, 0.22),
            one_way: c(0.35, 0.32, 0.2),
            backdrop: c(0.08, 0.13, 0.11),
            glow: c(0.4, 0.9, 0.5),
            glow_emissive: e(1.0, 3.0, 1.2),
        },
        Theme::Cistern => ThemeStyle {
            ambient: c(0.4, 0.6, 0.75),
            brightness: 210.0,
            fog: c(0.01, 0.03, 0.05),
            stone: c(0.18, 0.28, 0.34),
            one_way: c(0.3, 0.34, 0.3),
            backdrop: c(0.06, 0.12, 0.18),
            glow: c(0.3, 0.8, 0.9),
            glow_emissive: e(0.6, 2.8, 3.4),
        },
        Theme::Spire => ThemeStyle {
            ambient: c(0.55, 0.5, 0.75),
            brightness: 210.0,
            fog: c(0.03, 0.02, 0.05),
            stone: c(0.3, 0.26, 0.36),
            one_way: c(0.42, 0.34, 0.25),
            backdrop: c(0.14, 0.10, 0.20),
            glow: c(0.9, 0.75, 0.4),
            glow_emissive: e(3.0, 2.2, 0.8),
        },
        Theme::Throne => ThemeStyle {
            ambient: c(0.7, 0.4, 0.4),
            brightness: 190.0,
            fog: c(0.05, 0.01, 0.015),
            stone: c(0.32, 0.16, 0.18),
            one_way: c(0.4, 0.25, 0.2),
            backdrop: c(0.16, 0.05, 0.07),
            glow: c(0.95, 0.3, 0.3),
            glow_emissive: e(3.5, 0.6, 0.5),
        },
    }
}

// ---------------------------------------------------------------- starting --

fn start_game(mut commands: Commands, start: Res<StartRoom>) {
    let (room, entry) = (start.room.clone(), start.entry.clone());
    commands.queue(move |world: &mut World| {
        let theme = world.resource::<RoomLibrary>().get(&room).map(|d| d.theme);
        // The sandbox unlocks every move; real progression comes from pickups.
        let abilities = if theme == Some(Theme::Sandbox) {
            Abilities {
                dash: true,
                wall_grip: true,
            }
        } else {
            Abilities::default()
        };
        spawn_player(world, bevy::math::Vec2::ZERO, abilities);
        if let Err(e) = enter_room(world, &room, &entry) {
            eprintln!("could not enter start room: {e}");
        }
    });
}

// -------------------------------------------------------------------- view --

fn rebuild_view(
    mut commands: Commands,
    mut entered: MessageReader<RoomEntered>,
    library: Res<RoomLibrary>,
    old: Query<Entity, With<RoomVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut fog: Query<&mut DistanceFog, With<MainCamera>>,
) {
    let Some(ev) = entered.read().last().cloned() else {
        return;
    };
    let Some(def) = library.get(&ev.id) else {
        return;
    };
    for e in &old {
        commands.entity(e).despawn();
    }

    let st = style(def.theme);
    ambient.color = st.ambient;
    ambient.brightness = st.brightness;
    clear.0 = st.fog;
    for mut f in &mut fog {
        f.color = st.fog;
    }

    let solid = |c: Color, rough: f32| StandardMaterial {
        base_color: c,
        perceptual_roughness: rough,
        ..default()
    };
    let stone = mats.add(solid(st.stone, 0.9));
    let one_way = mats.add(solid(st.one_way, 0.8));
    let backdrop = mats.add(solid(st.backdrop, 1.0));
    let glow = mats.add(StandardMaterial {
        base_color: st.glow,
        emissive: st.glow_emissive,
        ..default()
    });

    let grid = def.grid();
    for j in 0..grid.height() {
        let mut i = 0;
        while i < grid.width() {
            let t = grid.get(i, j);
            if !matches!(t, Tile::Solid | Tile::OneWay) {
                i += 1;
                continue;
            }
            let start = i;
            while i < grid.width() && grid.get(i, j) == t {
                i += 1;
            }
            let w = (i - start) as f32;
            let (h, y_off, mat) = match t {
                Tile::OneWay => (0.25, 0.875, one_way.clone()),
                _ => (1.0, 0.5, stone.clone()),
            };
            commands.spawn((
                RoomVisual,
                Mesh3d(meshes.add(Cuboid::new(w, h, 4.0))),
                MeshMaterial3d(mat),
                Transform::from_xyz(start as f32 + w * 0.5, j as f32 + y_off, 0.0),
            ));
        }
    }
    spawn_backdrop(
        &mut commands,
        &mut meshes,
        &backdrop,
        &glow,
        RoomVisual,
        grid.width() as f32,
    );
}

/// Exits, benches and pickups are simulation entities; give them a look.
fn attach_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    exits: Query<(Entity, &RoomExit, &hk_sim::components::SimPos), Added<RoomExit>>,
    benches: Query<(Entity, &Bench, &hk_sim::components::SimPos), Added<Bench>>,
    pickups: Query<(Entity, &Pickup, &hk_sim::components::SimPos), Added<Pickup>>,
) {
    for (e, x, pos) in &exits {
        // A faint doorway glow so the way onward is visible.
        let m = mats.add(StandardMaterial {
            base_color: Color::srgba(0.6, 0.85, 1.0, 0.10),
            emissive: LinearRgba::rgb(0.3, 0.6, 1.0),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        commands.entity(e).insert((
            Mesh3d(meshes.add(Cuboid::new(x.half.x * 2.0, x.half.y * 2.0, 0.3))),
            MeshMaterial3d(m),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
        ));
    }
    for (e, b, pos) in &benches {
        let m = mats.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.5, 0.3),
            emissive: LinearRgba::rgb(1.2, 0.8, 0.3),
            ..default()
        });
        commands.entity(e).insert((
            Mesh3d(meshes.add(Cuboid::new(b.half.x * 2.0, b.half.y * 1.2, 0.8))),
            MeshMaterial3d(m),
            Transform::from_xyz(pos.0.x, pos.0.y - b.half.y * 0.4, 0.0),
        ));
    }
    for (e, p, pos) in &pickups {
        let m = mats.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.5),
            emissive: LinearRgba::rgb(3.0, 2.4, 0.8),
            ..default()
        });
        commands.entity(e).insert((
            Mesh3d(meshes.add(Sphere::new(p.half.x * 0.7))),
            MeshMaterial3d(m),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
        ));
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
