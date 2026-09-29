//! Stage dressing shared by every scene: lights, camera, parallax backdrop and
//! the material palette. Gameplay lives on z = 0; the backdrop sits at real
//! depths so the perspective camera gives true parallax.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Hdr;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.4, 0.46, 0.65),
            brightness: 220.0,
            ..default()
        })
        .add_systems(Startup, (spawn_palette, spawn_camera_and_lights));
    }
}

/// Camera field of view and distance, chosen so ~16 world units are visible
/// vertically at the z = 0 lane: d = 8 / tan(fov / 2).
pub const FOV_DEG: f32 = 38.0;
pub const CAM_DIST: f32 = 23.2;

#[derive(Component)]
pub struct MainCamera;

/// Materials for things that look the same in every theme (actors, effects).
#[derive(Resource, Clone)]
pub struct Palette {
    pub hazard: Handle<StandardMaterial>,
    pub slash: Handle<StandardMaterial>,
    pub bolt: Handle<StandardMaterial>,
    pub marker: Handle<StandardMaterial>,
}

fn spawn_palette(mut commands: Commands, mut mats: ResMut<Assets<StandardMaterial>>) {
    let marker = mats.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.08),
        perceptual_roughness: 0.5,
        ..default()
    });
    let mut emissive = |base: Color, e: LinearRgba, alpha: f32| {
        mats.add(StandardMaterial {
            base_color: base.with_alpha(alpha),
            emissive: e,
            alpha_mode: if alpha < 1.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            unlit: alpha < 1.0,
            ..default()
        })
    };
    let hazard = emissive(
        Color::srgb(0.6, 0.2, 0.7),
        LinearRgba::rgb(0.8, 0.2, 1.2),
        1.0,
    );
    let slash = emissive(
        Color::srgb(0.9, 0.95, 1.0),
        LinearRgba::rgb(2.0, 2.2, 2.6),
        0.35,
    );
    let bolt = emissive(
        Color::srgb(1.0, 0.6, 0.2),
        LinearRgba::rgb(3.0, 1.4, 0.3),
        0.6,
    );

    commands.insert_resource(Palette {
        hazard,
        slash,
        bolt,
        marker,
    });
}

fn spawn_camera_and_lights(mut commands: Commands) {
    // Cool rim/key light from behind-above.
    commands.spawn((
        DirectionalLight {
            illuminance: 3500.0,
            color: Color::srgb(0.55, 0.68, 1.0),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(-4.0, 8.0, 6.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));

    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Hdr,
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEG.to_radians(),
            ..default()
        }),
        Transform::from_xyz(4.0, 8.0, CAM_DIST).looking_at(Vec3::new(4.0, 8.0, 0.0), Vec3::Y),
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        DistanceFog {
            color: Color::srgb(0.015, 0.02, 0.035),
            falloff: FogFalloff::Linear {
                start: 22.0,
                end: 80.0,
            },
            ..default()
        },
    ));
}

/// Layered background slabs and glowing motes across `width` world units.
/// Every spawned entity also gets `marker` so the caller can tear it down.
pub fn spawn_backdrop<M: Bundle + Clone>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    backdrop: &Handle<StandardMaterial>,
    glow: &Handle<StandardMaterial>,
    marker: M,
    width: f32,
) {
    for (z, count, h) in [(-6.0, 12, 10.0), (-14.0, 10, 16.0), (-30.0, 8, 26.0)] {
        for i in 0..count {
            let t = i as f32 / (count - 1) as f32;
            let x = -10.0 + t * (width + 20.0) + (i as f32 * 3.7).sin() * 3.0;
            let w = 4.0 + (i % 3) as f32 * 2.5;
            commands.spawn((
                marker.clone(),
                Mesh3d(meshes.add(Cuboid::new(w, h, 3.0))),
                MeshMaterial3d(backdrop.clone()),
                Transform::from_xyz(x, h * 0.5 - 1.0, z),
            ));
        }
    }
    for i in 0..14 {
        let x = i as f32 * width / 13.0;
        let y = 4.0 + ((i * 7) % 11) as f32;
        commands.spawn((
            marker.clone(),
            Mesh3d(meshes.add(Sphere::new(0.16))),
            MeshMaterial3d(glow.clone()),
            Transform::from_xyz(x, y, -4.0 - (i % 4) as f32 * 2.0),
        ));
    }
}
