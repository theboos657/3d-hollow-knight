//! Stage dressing shared by every scene: lights, camera, parallax backdrop and
//! the material palette. Gameplay lives on z = 0; the backdrop sits at real
//! depths so the perspective camera gives true parallax.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, Hdr};

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

/// Camera field of view and distance, chosen so ~13.4 world units are visible
/// vertically at the z = 0 lane: d = 6.7 / tan(fov / 2).
pub const FOV_DEG: f32 = 38.0;
pub const CAM_DIST: f32 = 19.5;

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
    // The key light: from above-left and in front, so ledges catch a highlight
    // and everything on the play lane throws a shadow on the wall behind.
    // (Its colour and strength are set per area by `look::room`.)
    commands.spawn((
        crate::look::KeyLight,
        DirectionalLight {
            illuminance: 5000.0,
            color: Color::srgb(1.0, 0.9, 0.8),
            shadows_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 0.1,
            maximum_distance: 60.0,
            first_cascade_far_bound: 60.0,
            overlap_proportion: 0.0,
        }
        .build(),
        Transform::from_xyz(-3.0, 6.5, 10.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
    ));
    // A weak cool light from the other side, so silhouettes keep an edge.
    commands.spawn((
        crate::look::RimLight,
        DirectionalLight {
            illuminance: 900.0,
            color: Color::srgb(0.5, 0.6, 1.0),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(6.0, 3.0, -4.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
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
        ColorGrading::default(),
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
