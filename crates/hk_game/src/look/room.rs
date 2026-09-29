//! Building the current room's scenery when it is entered.

use bevy::light::NotShadowCaster;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use hk_sim::world::room::{RoomEntered, RoomLibrary};

use super::decor::build_decor;
use super::kits::{build_kit, WALL_Z};
use super::level::{build_level, wall_mesh};
use super::props::{brazier_meshes, pick_spots, Flame};
use super::style::style;
use super::texture::stone_grain;
use super::{KeyLight, Mote, RimLight, RoomVisual};
use crate::rig::meshkit::hash3;
use crate::scene::MainCamera;

/// A stable seed from a room's id, so a room always looks the same.
pub fn room_seed(id: &str) -> u32 {
    id.bytes().fold(0x811C_9DC5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

/// Spawns a level's stone blocks, floor lips and one-way planks in `st`'s
/// palette, tagged with `marker`. Returns the stone-grain texture, so the caller
/// can texture the wall the same way.
#[allow(clippy::too_many_arguments)]
pub fn spawn_level<M: Bundle + Clone>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    st: &super::style::LookStyle,
    grid: &hk_sim::world::grid::TileGrid,
    seed: u32,
    marker: M,
) -> Handle<Image> {
    let grain = stone_grain(images);
    let mut textured = |base: Color, rough: f32| {
        mats.add(StandardMaterial {
            base_color: base,
            base_color_texture: Some(grain.clone()),
            perceptual_roughness: rough,
            ..default()
        })
    };
    let stone = textured(st.stone, 0.92);
    let cap = textured(st.cap, 0.75);
    let plank = textured(st.one_way, 0.85);
    let geo = build_level(grid, seed);
    for (first, m) in geo.chunks {
        commands.spawn((
            marker.clone(),
            Name::new(format!("stone {first}")),
            // The rock does not cast shadows: a whole ceiling's shadow lands on the
            // wall as a heavy black bar. Actors and props still ground themselves.
            NotShadowCaster,
            Mesh3d(meshes.add(m.to_mesh())),
            MeshMaterial3d(stone.clone()),
            Transform::IDENTITY,
        ));
    }
    commands.spawn((
        marker.clone(),
        NotShadowCaster,
        Mesh3d(meshes.add(geo.caps.to_mesh())),
        MeshMaterial3d(cap),
        Transform::IDENTITY,
    ));
    commands.spawn((
        marker,
        // Planks would throw floating bars of shadow on the far wall.
        NotShadowCaster,
        Mesh3d(meshes.add(geo.planks.to_mesh())),
        MeshMaterial3d(plank),
        Transform::IDENTITY,
    ));
    grain
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn rebuild_room(
    mut commands: Commands,
    mut entered: MessageReader<RoomEntered>,
    library: Res<RoomLibrary>,
    old: Query<Entity, With<RoomVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut cam: Query<(&mut DistanceFog, &mut ColorGrading), With<MainCamera>>,
    mut key: Query<&mut DirectionalLight, With<KeyLight>>,
    mut rim: Query<&mut DirectionalLight, (With<RimLight>, Without<KeyLight>)>,
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
    let seed = room_seed(&def.id);

    ambient.color = st.ambient;
    ambient.brightness = st.ambient_brightness;
    clear.0 = st.fog;
    for (mut fog, mut grading) in &mut cam {
        fog.color = st.fog;
        fog.falloff = bevy::pbr::FogFalloff::Linear {
            start: 22.0,
            end: 80.0,
        };
        grading.global.temperature = st.temperature;
        grading.global.post_saturation = st.saturation;
        grading.midtones.contrast = st.contrast;
    }
    for mut l in &mut key {
        l.color = st.key;
        l.illuminance = st.key_lux;
    }
    for mut l in &mut rim {
        l.color = st.rim;
    }

    let grid = def.grid();
    let grain = spawn_level(
        &mut commands,
        &mut meshes,
        &mut mats,
        &mut images,
        &st,
        &grid,
        seed,
        RoomVisual,
    );
    let wall = mats.add(StandardMaterial {
        base_color: st.wall,
        base_color_texture: Some(grain.clone()),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        RoomVisual,
        NotShadowCaster,
        Mesh3d(
            meshes
                .add(wall_mesh(grid.width() as f32, grid.height() as f32, WALL_Z, seed).to_mesh()),
        ),
        MeshMaterial3d(wall),
        Transform::IDENTITY,
    ));

    // The architecture in front of the wall: stone, glowing panes and light shafts.
    let kit = build_kit(def.theme, grid.width() as f32, grid.height() as f32, seed);
    let kit_stone = mats.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5).mix(&st.stone, 0.6),
        perceptual_roughness: 1.0,
        ..default()
    });
    let kit_glow = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let kit_beam = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    for (m, material, casts) in [(kit.dark, kit_stone, false), (kit.glow, kit_glow, false)] {
        if m.vertex_count() == 0 {
            continue;
        }
        let mut e = commands.spawn((
            RoomVisual,
            Mesh3d(meshes.add(m.to_mesh())),
            MeshMaterial3d(material),
            Transform::IDENTITY,
        ));
        if !casts {
            e.insert(NotShadowCaster);
        }
    }
    if kit.beams.vertex_count() > 0 {
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(kit.beams.to_mesh())),
            MeshMaterial3d(kit_beam),
            Transform::IDENTITY,
        ));
    }

    // Braziers: iron stands with a flame and a flickering pool of light.
    let (stand, flame) = brazier_meshes();
    let stand = meshes.add(stand.to_mesh());
    let flame = meshes.add(flame.to_mesh());
    let iron = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.11, 0.12),
        perceptual_roughness: 0.6,
        metallic: 0.6,
        ..default()
    });
    let fire = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.12, 0.05),
        emissive: st.flame,
        ..default()
    });
    for (i, j) in pick_spots(&grid, seed, 5) {
        let at = Vec3::new(i as f32 + 0.5, j as f32 + 1.0, 0.6);
        let phase = hash3(seed, i, j, 31) * std::f32::consts::TAU;
        commands
            .spawn((
                RoomVisual,
                Transform::from_translation(at),
                Visibility::default(),
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(stand.clone()),
                    MeshMaterial3d(iron.clone()),
                    Transform::IDENTITY,
                ));
                p.spawn((
                    NotShadowCaster,
                    Mesh3d(flame.clone()),
                    MeshMaterial3d(fire.clone()),
                    Transform::IDENTITY,
                ));
                p.spawn((
                    Flame {
                        base: 420_000.0,
                        phase,
                    },
                    PointLight {
                        intensity: 420_000.0,
                        range: 13.0,
                        color: st.flame_light,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 1.1, 0.5),
                ));
            });
    }

    // Growth, rubble, stalactites and chains.
    let decor = build_decor(&grid, def.theme, seed);
    for (m, base) in [
        (decor.growth, Color::WHITE),
        (decor.rubble, Color::srgb(0.85, 0.85, 0.9)),
        (decor.hangers, Color::srgb(0.9, 0.9, 0.95)),
    ] {
        if m.vertex_count() == 0 {
            continue;
        }
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(m.to_mesh())),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: base,
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::IDENTITY,
        ));
    }

    // Motes drifting through the air.
    let mote_mesh = meshes.add(Sphere::new(0.07));
    let mote = mats.add(StandardMaterial {
        base_color: st.mote,
        emissive: st.mote_emissive,
        unlit: true,
        ..default()
    });
    let count = ((grid.width() as f32 * grid.height() as f32) / 40.0).clamp(24.0, 70.0) as i32;
    for k in 0..count {
        let x = hash3(seed, k, 0, 41) * grid.width() as f32;
        let span = 4.0 + hash3(seed, k, 1, 41) * 6.0;
        let y = 1.5 + hash3(seed, k, 3, 41) * (grid.height() as f32 - span - 1.0).max(1.0);
        let z = -0.5 - hash3(seed, k, 2, 41) * 2.6;
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mote {
                base: Vec3::new(x, y, z),
                phase: hash3(seed, k, 4, 41),
                rise: 0.18 + 0.3 * hash3(seed, k, 5, 41),
                span,
            },
            Mesh3d(mote_mesh.clone()),
            MeshMaterial3d(mote.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_seeds_are_stable_and_differ() {
        assert_eq!(room_seed("A1"), room_seed("A1"));
        assert_ne!(room_seed("A1"), room_seed("A2"));
    }
}
