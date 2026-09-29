//! `--viewer`: a stage that shows every model in its poses side by side, with
//! no simulation running. It exists so the art can be checked (and screenshotted
//! headlessly) without having to play up to the right moment.
//!
//! `cargo play -- --viewer` (add `--shots room --shot-prefix v_` for a picture).

use bevy::prelude::*;
use hk_sim::tuning::Tuning;

use crate::models::knight::{
    apply, spawn_knight, swing_timing, KnightAnim, KnightAssets, KnightRig, Out,
};
use crate::rig::pose::{knight_pose, step_cape, swing_pose, KnightIn, SwingDir};
use crate::scene::MainCamera;

pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostStartup,
            build_stage.after(crate::models::knight::build_knight_assets),
        )
        .add_systems(Update, (animate_viewer, frame_camera));
    }
}

/// One posed knight on the stage.
#[derive(Component)]
struct ViewerKnight {
    input: KnightIn,
    /// Swing direction and the tick to freeze it at.
    swing: Option<(SwingDir, f32)>,
    facing: i8,
    tint: LinearRgba,
}

fn pose(f: impl FnOnce(&mut KnightIn)) -> KnightIn {
    let mut k = KnightIn {
        grounded: true,
        soul: 0.7,
        ..Default::default()
    };
    f(&mut k);
    k
}

/// Every pose worth checking, in reading order (two rows).
fn poses() -> Vec<(&'static str, ViewerKnight)> {
    let v = |name: &'static str, input: KnightIn, swing: Option<(SwingDir, f32)>| {
        (
            name,
            ViewerKnight {
                input,
                swing,
                facing: 1,
                tint: LinearRgba::rgb(0.05, 0.05, 0.05),
            },
        )
    };
    vec![
        v("idle", pose(|_| {}), None),
        v(
            "run",
            pose(|k| {
                k.vx = 9.0;
                k.run_phase = 1.0;
            }),
            None,
        ),
        v(
            "jump",
            pose(|k| {
                k.grounded = false;
                k.vy = 15.0;
            }),
            None,
        ),
        v(
            "fall",
            pose(|k| {
                k.grounded = false;
                k.vy = -15.0;
            }),
            None,
        ),
        v(
            "dash",
            pose(|k| {
                k.dashing = true;
                k.vx = 24.0;
            }),
            None,
        ),
        v(
            "wall-slide",
            pose(|k| {
                k.grounded = false;
                k.wall_slide = true;
                k.vy = -3.5;
            }),
            None,
        ),
        v("focus", pose(|k| k.focusing = true), None),
        v("hurt", pose(|k| k.hurt = true), None),
        v("dead", pose(|k| k.dead = true), None),
        v(
            "swing-fwd-coil",
            pose(|_| {}),
            Some((SwingDir::Forward, 3.0)),
        ),
        v(
            "swing-fwd-mid",
            pose(|_| {}),
            Some((SwingDir::Forward, 9.0)),
        ),
        v("swing-up-mid", pose(|_| {}), Some((SwingDir::Up, 9.0))),
        v(
            "swing-down-mid",
            pose(|k| k.grounded = false),
            Some((SwingDir::Down, 9.0)),
        ),
        v(
            "swing-fwd-settle",
            pose(|_| {}),
            Some((SwingDir::Forward, 19.0)),
        ),
    ]
}

fn build_stage(
    mut commands: Commands,
    assets: Res<KnightAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let floor = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.27, 0.34),
        perceptual_roughness: 0.9,
        ..default()
    });
    let poses = poses();
    let per_row = 7usize;
    let step = 2.7;
    for row in 0..2 {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(per_row as f32 * step + 2.0, 0.2, 3.0))),
            MeshMaterial3d(floor.clone()),
            Transform::from_xyz(0.0, -0.1 - row as f32 * 4.4, 0.0),
        ));
    }
    commands.spawn((
        PointLight {
            intensity: 3_000_000.0,
            range: 60.0,
            color: Color::srgb(1.0, 0.92, 0.8),
            ..default()
        },
        Transform::from_xyz(0.0, 3.0, 9.0),
    ));
    for (i, (name, mut vk)) in poses.into_iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        let x = (col as f32 - (per_row as f32 - 1.0) / 2.0) * step;
        let y = -(row as f32) * 4.4;
        println!("viewer: row {row} column {col}: {name}");
        // A plain anchor at the body's centre (the rig's feet are 0.75 below).
        let anchor = commands
            .spawn((Transform::from_xyz(x, y + 0.75, 0.0), Visibility::default()))
            .id();
        let rig = spawn_knight(&mut commands, &assets, anchor, 0.75);
        vk.input.clock = 0.4;
        commands
            .entity(anchor)
            .insert((rig, KnightAnim::default(), vk));
    }
}

#[allow(clippy::type_complexity)]
fn animate_viewer(
    time: Res<Time>,
    tuning: Res<Tuning>,
    assets: Option<Res<KnightAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut knights: Query<(&KnightRig, &mut KnightAnim, &mut ViewerKnight), Without<Camera3d>>,
    mut transforms: Query<
        (&mut Transform, Option<&crate::rig::Rest>),
        Without<hk_sim::player::Player>,
    >,
    mut visibility: Query<&mut Visibility, Without<hk_sim::player::Player>>,
    mut trail_mesh: Query<&mut Mesh3d, Without<hk_sim::player::Player>>,
) {
    let Some(assets) = assets else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    let timing = swing_timing(&tuning);
    for (rig, mut anim, mut vk) in &mut knights {
        vk.input.clock += dt;
        step_cape(&mut anim.cape, vk.input.vx, vk.input.vy, dt);
        vk.input.cape = [anim.cape[0].x, anim.cape[1].x, anim.cape[2].x];
        let swing = vk.swing.and_then(|(dir, t)| swing_pose(dir, t, timing));
        vk.input.swing = swing;
        let p = knight_pose(&vk.input);
        let mut out = Out {
            pose: &p,
            facing: vk.facing,
            swing,
            dir: vk.swing.map(|(d, _)| d),
            flicker: false,
            state_tint: vk.tint,
            flash: 0.0,
        };
        apply(
            &mut out,
            rig,
            &mut anim,
            &assets,
            &mut mats,
            &mut transforms,
            &mut visibility,
            &mut trail_mesh,
            dt,
        );
    }
}

fn frame_camera(mut cam: Query<&mut Transform, With<MainCamera>>) {
    for mut t in &mut cam {
        *t = Transform::from_xyz(0.0, -1.2, 17.5).looking_at(Vec3::new(0.0, -1.4, 0.0), Vec3::Y);
    }
}
