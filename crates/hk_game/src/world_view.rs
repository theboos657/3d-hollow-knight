//! Showing the current room: themed level geometry and lighting, props
//! (exits, benches, pickups), the fade overlay for room changes, starting the
//! game, and the sandbox's convenience respawn.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use hk_sim::combat::EnemyDied;
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::world::grid::Tile;
use hk_sim::world::progress::{Checkpoint, SaveData};
use hk_sim::world::room::*;

use crate::interp::RenderPrepSet;
use crate::scene::{spawn_backdrop, MainCamera};

/// How the game begins.
#[derive(Resource, Clone)]
pub enum StartMode {
    /// Pick up a saved game at its last bench.
    Continue(SaveData),
    /// A fresh game in the first room, with no abilities.
    New,
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

// ------------------------------------------------------------- stone look --

/// A small brick pattern (2 bricks wide, 4 rows) that tiles, so a wall reads
/// as made of tiles at the size the player moves in, not as one flat plane.
fn stone_texture(images: &mut Assets<Image>) -> Handle<Image> {
    const N: usize = 64;
    let hash = |a: u32, b: u32| -> f32 {
        let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
        (h & 0xFFFF) as f32 / 65535.0
    };
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let row = y / 16;
            let off = if row % 2 == 0 { 0 } else { 16 };
            let (bx, by) = ((x + off) % 32, y % 16);
            let brick = hash(row as u32, ((x + off) / 32) as u32);
            let shade = if bx < 2 || by < 2 {
                0.42
            } else {
                0.72 + 0.3 * brick + 0.06 * (hash(x as u32, y as u32) - 0.5)
            };
            let v = (shade.clamp(0.0, 1.0) * 255.0) as u8;
            data.extend([v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: N as u32,
            height: N as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    images.add(img)
}

/// A cuboid whose texture coordinates repeat once per two tiles, whatever its size.
fn tiled_cuboid(w: f32, h: f32, d: f32) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(w, h, d));
    let normals: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) => n.clone(),
        _ => return mesh,
    };
    if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
    {
        for (uv, n) in uvs.iter_mut().zip(normals) {
            let (su, sv) = if n[2].abs() > 0.5 {
                (w, h)
            } else if n[0].abs() > 0.5 {
                (d, h)
            } else {
                (w, d)
            };
            uv[0] *= su * 0.5;
            uv[1] *= sv * 0.5;
        }
    }
    mesh
}

fn scaled(c: Color, k: f32) -> Color {
    let s = c.to_srgba();
    Color::srgb(
        (s.red * k).min(1.0),
        (s.green * k).min(1.0),
        (s.blue * k).min(1.0),
    )
}

// -------------------------------------------------------------------- view --

fn rebuild_view(
    mut commands: Commands,
    mut entered: MessageReader<RoomEntered>,
    library: Res<RoomLibrary>,
    old: Query<Entity, With<RoomVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
    let brick = stone_texture(&mut images);
    let stone = mats.add(StandardMaterial {
        base_color: scaled(st.stone, 1.5),
        base_color_texture: Some(brick),
        perceptual_roughness: 0.9,
        ..default()
    });
    // A lighter lip along every walkable surface, so ledges read at a glance.
    let rim = mats.add(StandardMaterial {
        base_color: scaled(st.stone, 2.6),
        emissive: LinearRgba::rgb(0.03, 0.03, 0.03),
        perceptual_roughness: 0.7,
        ..default()
    });
    let one_way = mats.add(solid(scaled(st.one_way, 1.5), 0.8));
    let backdrop = mats.add(solid(scaled(st.backdrop, 0.6), 1.0));
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
            let mesh = if t == Tile::Solid {
                tiled_cuboid(w, h, 4.0)
            } else {
                Mesh::from(Cuboid::new(w, h, 4.0))
            };
            commands.spawn((
                RoomVisual,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat),
                Transform::from_xyz(start as f32 + w * 0.5, j as f32 + y_off, 0.0),
            ));
        }
    }
    // Rims: runs of solid tiles with open air above.
    for j in 0..grid.height() {
        let exposed = |i: i32| grid.get(i, j) == Tile::Solid && grid.get(i, j + 1) != Tile::Solid;
        let mut i = 0;
        while i < grid.width() {
            if !exposed(i) {
                i += 1;
                continue;
            }
            let start = i;
            while i < grid.width() && exposed(i) {
                i += 1;
            }
            let w = (i - start) as f32;
            commands.spawn((
                RoomVisual,
                Mesh3d(meshes.add(Cuboid::new(w, 0.14, 4.04))),
                MeshMaterial3d(rim.clone()),
                Transform::from_xyz(start as f32 + w * 0.5, j as f32 + 0.93, 0.0),
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
