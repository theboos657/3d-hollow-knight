//! M0 greybox: a lit 2.5D test room. Gameplay sits on z = 0; background
//! slabs at real depths give true parallax from the perspective camera.
//! The camera rig proper arrives in M4.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Hdr;
use hk_sim::components::{PrevPos, SimPos, Velocity};
use hk_sim::input::InputState;
use hk_sim::{SimSet, DT};

use crate::interp::Interpolated;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.35, 0.42, 0.6),
            brightness: 140.0,
            ..default()
        })
        .add_systems(Startup, spawn_greybox)
        // Throwaway mover so M0 can prove input -> sim -> interpolation ->
        // render end to end. Replaced by the real controller in M1.
        .add_systems(FixedUpdate, demo_mover.in_set(SimSet::Motion));
    }
}

/// Visual FOV and distance chosen so ~16 world units are visible vertically
/// at the z = 0 lane: d = 8 / tan(fov / 2).
const FOV_DEG: f32 = 38.0;
const CAM_DIST: f32 = 23.2;

#[derive(Component)]
struct DemoMover;

fn spawn_greybox(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let stone = mats.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.18, 0.24),
        perceptual_roughness: 0.9,
        ..default()
    });
    let far = mats.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.1, 0.16),
        perceptual_roughness: 1.0,
        ..default()
    });
    let glow = mats.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.6, 0.9),
        emissive: LinearRgba::rgb(0.6, 2.4, 4.0),
        ..default()
    });
    let hero = mats.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.92, 1.0),
        emissive: LinearRgba::rgb(0.15, 0.15, 0.2),
        ..default()
    });

    // Solid geometry on the gameplay lane (depth 4 u, centred on z = 0).
    let solids: &[(Vec2, Vec2)] = &[
        (Vec2::new(0.0, -1.0), Vec2::new(40.0, 2.0)),  // floor
        (Vec2::new(-8.0, 3.0), Vec2::new(5.0, 0.6)),   // ledge
        (Vec2::new(7.0, 5.5), Vec2::new(6.0, 0.6)),    // high ledge
        (Vec2::new(-19.5, 6.0), Vec2::new(1.0, 14.0)), // left wall
        (Vec2::new(19.5, 6.0), Vec2::new(1.0, 14.0)),  // right wall
    ];
    for (c, size) in solids {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, 4.0))),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(c.x, c.y, 0.0),
        ));
    }

    // Parallax layers at real depths.
    for (z, count, h) in [(-6.0, 7, 9.0), (-14.0, 6, 14.0), (-30.0, 5, 24.0)] {
        for i in 0..count {
            let t = i as f32 / (count - 1) as f32;
            let x = (t - 0.5) * 70.0 + (i as f32 * 3.7).sin() * 4.0;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(4.0 + (i % 3) as f32 * 2.0, h, 3.0))),
                MeshMaterial3d(far.clone()),
                Transform::from_xyz(x, h * 0.5 - 1.0, z),
            ));
        }
    }
    // A few glowing motes to catch bloom.
    for (x, y, z) in [(-12.0, 4.0, -5.0), (4.0, 8.0, -9.0), (14.0, 3.0, -7.0)] {
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(0.18))),
            MeshMaterial3d(glow.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }

    // Placeholder hero: box centred on the sim position.
    let start = Vec2::new(0.0, 0.75);
    commands
        .spawn((
            DemoMover,
            SimPos(start),
            PrevPos(start),
            Velocity::default(),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
            Mesh3d(meshes.add(Cuboid::new(0.8, 1.5, 0.8))),
            MeshMaterial3d(hero),
            Transform::from_xyz(start.x, start.y, 0.0),
        ))
        .with_children(|p| {
            // Follow lantern.
            p.spawn((
                PointLight {
                    intensity: 900_000.0,
                    range: 24.0,
                    color: Color::srgb(1.0, 0.85, 0.6),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.6, 2.5),
            ));
        });

    // Cool rim/key light from behind-above.
    commands.spawn((
        DirectionalLight {
            illuminance: 2500.0,
            color: Color::srgb(0.5, 0.65, 1.0),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(-4.0, 8.0, -6.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));

    commands.spawn((
        Camera3d::default(),
        Hdr,
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEG.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, 7.0, CAM_DIST).looking_at(Vec3::new(0.0, 7.0, 0.0), Vec3::Y),
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        DistanceFog {
            color: Color::srgb(0.015, 0.02, 0.035),
            falloff: FogFalloff::Linear {
                start: 22.0,
                end: 75.0,
            },
            ..default()
        },
    ));
}

fn demo_mover(input: Res<InputState>, mut q: Query<(&mut SimPos, &mut Velocity), With<DemoMover>>) {
    for (mut pos, mut vel) in &mut q {
        vel.0.x = input.axis_x() as f32 * 9.0;
        pos.0.x = (pos.0.x + vel.0.x * DT).clamp(-18.5, 18.5);
    }
}
