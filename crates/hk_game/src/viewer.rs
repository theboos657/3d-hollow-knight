//! `--viewer`: a stage that shows every model in its poses side by side, with
//! no simulation running. It exists so the art can be checked (and screenshotted
//! headlessly) without having to play up to the right moment.
//!
//! `cargo play -- --viewer` (add `--shots room --shot-prefix v_` for a picture);
//! `--viewer-set knight|enemies` picks the sheet.

use bevy::prelude::*;
use hk_sim::tuning::Tuning;

use hk_sim::enemy::EnemyState;

use crate::models::enemies::{
    apply_creature, make_assets, spawn_creature, state_len, CreatureAnim, CreatureRig,
};
use crate::models::knight::{
    apply, spawn_knight, swing_timing, KnightAnim, KnightAssets, KnightRig, Out,
};
use crate::rig::creature::{creature_pose, tell_glow, CreatureIn, Species};
use crate::rig::pose::{knight_pose, step_cape, swing_pose, KnightIn, SwingDir};
use crate::scene::MainCamera;

/// Which sheet the viewer lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Resource)]
pub enum ViewerSet {
    #[default]
    Knight,
    /// Every enemy in every state, small.
    Enemies,
    /// One enemy in every state, large.
    Species(Species),
}

impl ViewerSet {
    pub fn parse(v: Option<&str>) -> Self {
        match v {
            Some("enemies") => ViewerSet::Enemies,
            Some("husk") => ViewerSet::Species(Species::Husk),
            Some("wisp") => ViewerSet::Species(Species::Wisp),
            Some("shield") => ViewerSet::Species(Species::Shieldbearer),
            Some("spitter") => ViewerSet::Species(Species::Spitter),
            Some("dummy") => ViewerSet::Species(Species::Dummy),
            _ => ViewerSet::Knight,
        }
    }

    fn is_enemies(self) -> bool {
        !matches!(self, ViewerSet::Knight)
    }
}

pub struct ViewerPlugin {
    pub set: ViewerSet,
    /// `--viewer-cols 3,4`: only these columns of an enemy sheet, zoomed in.
    pub columns: Option<Vec<usize>>,
}

#[derive(Resource, Default)]
struct ViewerColumns(Option<Vec<usize>>);

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.set)
            .insert_resource(ViewerColumns(self.columns.clone()))
            .add_systems(
                PostStartup,
                (build_stage, build_enemy_stage).after(crate::models::knight::build_knight_assets),
            )
            .add_systems(
                Update,
                (animate_viewer, animate_viewer_creatures, frame_camera),
            );
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
    set: Res<ViewerSet>,
    assets: Res<KnightAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if *set != ViewerSet::Knight {
        return;
    }
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

fn frame_camera(
    set: Res<ViewerSet>,
    cols: Res<ViewerColumns>,
    mut cam: Query<&mut Transform, With<MainCamera>>,
) {
    for mut t in &mut cam {
        *t = match *set {
            ViewerSet::Knight => {
                Transform::from_xyz(0.0, -1.2, 17.5).looking_at(Vec3::new(0.0, -1.4, 0.0), Vec3::Y)
            }
            ViewerSet::Enemies => {
                Transform::from_xyz(0.0, -5.2, 27.0).looking_at(Vec3::new(0.0, -5.4, 0.0), Vec3::Y)
            }
            ViewerSet::Species(_) => {
                // Zoomed in when only a few columns are shown.
                let dist = if cols.0.is_some() { 8.5 } else { 12.5 };
                Transform::from_xyz(0.0, 0.6, dist).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y)
            }
        };
    }
}

// ----------------------------------------------------------------- enemies --

/// One posed creature on the stage.
#[derive(Component)]
struct ViewerCreature {
    input: CreatureIn,
    facing: i8,
    /// Freeze `t` at this fraction of the state's length.
    at: f32,
    walking: bool,
}

const ENEMY_COLUMNS: [(&str, EnemyState, f32, bool); 7] = [
    ("idle", EnemyState::Idle, 0.0, false),
    ("walk", EnemyState::Chase, 0.0, true),
    ("notice", EnemyState::Notice, 1.0, false),
    ("windup", EnemyState::Windup, 1.0, false),
    ("attack", EnemyState::Attack, 0.4, false),
    ("recover", EnemyState::Recover, 0.05, false),
    ("stagger", EnemyState::Stagger, 0.0, false),
];

/// Species, half height of its hurtbox, and how far off the floor it hovers.
const ENEMY_ROWS: [(Species, f32, f32); 5] = [
    (Species::Husk, 0.6, 0.0),
    (Species::Wisp, 0.45, 0.8),
    (Species::Shieldbearer, 0.75, 0.0),
    (Species::Spitter, 0.6, 0.0),
    (Species::Dummy, 0.6, 0.0),
];

fn build_enemy_stage(
    mut commands: Commands,
    set: Res<ViewerSet>,
    cols: Res<ViewerColumns>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if !set.is_enemies() {
        return;
    }
    let assets = make_assets(&mut meshes, &mut mats);
    let floor = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.27, 0.34),
        perceptual_roughness: 0.9,
        ..default()
    });
    let step = 2.5;
    let pitch = 3.3;
    let rows: Vec<(Species, f32, f32)> = ENEMY_ROWS
        .into_iter()
        .filter(|(sp, _, _)| match *set {
            ViewerSet::Species(only) => *sp == only,
            _ => true,
        })
        .collect();
    for (row, (species, half_y, lift)) in rows.into_iter().enumerate() {
        let y = -(row as f32) * pitch;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(7.0 * step + 2.0, 0.2, 3.0))),
            MeshMaterial3d(floor.clone()),
            Transform::from_xyz(0.0, y - 0.1, 0.0),
        ));
        for (col, (name, state, at, walking)) in ENEMY_COLUMNS.into_iter().enumerate() {
            let shown = cols.0.as_ref().map_or(0.0, |c| {
                if c.contains(&col) {
                    c.iter().position(|k| *k == col).unwrap_or(0) as f32
                } else {
                    -1.0
                }
            });
            if shown < 0.0 {
                continue;
            }
            // The dummy has no states: show it at rest and swaying both ways.
            if species == Species::Dummy && !matches!(col, 0 | 2 | 3) {
                continue;
            }
            let sway = match (species, col) {
                (Species::Dummy, 2) => 0.30,
                (Species::Dummy, 3) => -0.30,
                _ => 0.0,
            };
            let x = match &cols.0 {
                Some(c) => (shown - (c.len() as f32 - 1.0) / 2.0) * step,
                None => (col as f32 - 3.0) * step,
            };
            println!("viewer: {species:?} {name}");
            let anchor = commands
                .spawn((
                    Transform::from_xyz(x, y + half_y + lift, 0.0),
                    Visibility::default(),
                ))
                .id();
            let rig = spawn_creature(&mut commands, &mut mats, &assets, anchor, species, half_y);
            let aim = match species {
                Species::Wisp => -0.6,
                Species::Spitter => 0.35,
                _ => 0.0,
            };
            commands.entity(anchor).insert((
                rig,
                CreatureAnim::new(1, 1.0),
                ViewerCreature {
                    input: CreatureIn {
                        species,
                        state,
                        aim,
                        sway,
                        vx: if walking { 3.0 } else { 0.0 },
                        ..Default::default()
                    },
                    facing: 1,
                    at,
                    walking,
                },
            ));
        }
    }
    commands.insert_resource(assets);
    // A soft front light so colours can be judged (the game lights rooms itself).
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            ..default()
        },
        Transform::from_xyz(-4.0, 8.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        PointLight {
            intensity: 4_000_000.0,
            range: 80.0,
            color: Color::srgb(1.0, 0.92, 0.8),
            ..default()
        },
        Transform::from_xyz(0.0, 3.0, 10.0),
    ));
}

#[allow(clippy::type_complexity)]
fn animate_viewer_creatures(
    time: Res<Time>,
    tuning: Res<Tuning>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut creatures: Query<(&CreatureRig, &mut CreatureAnim, &mut ViewerCreature)>,
    mut transforms: Query<(&mut Transform, Option<&crate::rig::Rest>), Without<CreatureRig>>,
) {
    let dt = time.delta_secs().min(0.05);
    // Windup flashes on the sim tick; here, on the wall clock.
    let tick = (time.elapsed_secs() * 120.0) as u64;
    for (rig, mut anim, mut vc) in &mut creatures {
        anim.clock += dt;
        if vc.walking {
            anim.walk += 9.0 * dt;
        }
        let kind = match rig.species {
            Species::Husk => hk_sim::enemy::EnemyKind::Husk,
            Species::Wisp => hk_sim::enemy::EnemyKind::Wisp,
            Species::Shieldbearer => hk_sim::enemy::EnemyKind::Shieldbearer,
            _ => hk_sim::enemy::EnemyKind::Spitter,
        };
        let len = state_len(kind, vc.input.state, &tuning.enemies);
        vc.input.len = len;
        vc.input.t = len * vc.at;
        vc.input.clock = anim.clock;
        vc.input.walk = anim.walk;
        vc.input.guard = anim.guard;
        let pose = creature_pose(&vc.input);
        let glow = if rig.species == Species::Dummy {
            [0.0; 3]
        } else {
            tell_glow(rig.species, vc.input.state, tick)
        };
        let face = vc.facing;
        apply_creature(
            &pose,
            face,
            glow,
            0.0,
            rig,
            &mut anim,
            &mut mats,
            &mut transforms,
            dt,
        );
    }
}
