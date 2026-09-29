//! `--viewer`: a stage that shows every model in its poses side by side, with
//! no simulation running. It exists so the art can be checked (and screenshotted
//! headlessly) without having to play up to the right moment.
//!
//! `cargo play -- --viewer` (add `--shots room --shot-prefix v_` for a picture);
//! `--viewer-set knight|enemies|materials` picks the sheet.

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
    /// Every procedural material on a sphere and a slab.
    Materials,
}

impl ViewerSet {
    pub fn parse(v: Option<&str>) -> Self {
        match v {
            Some("enemies") => ViewerSet::Enemies,
            Some("materials") => ViewerSet::Materials,
            Some("husk") => ViewerSet::Species(Species::Husk),
            Some("wisp") => ViewerSet::Species(Species::Wisp),
            Some("shield") => ViewerSet::Species(Species::Shieldbearer),
            Some("spitter") => ViewerSet::Species(Species::Spitter),
            Some("dummy") => ViewerSet::Species(Species::Dummy),
            Some("matron") => ViewerSet::Species(Species::Matron),
            Some("warden") => ViewerSet::Species(Species::Bellwarden),
            _ => ViewerSet::Knight,
        }
    }

    fn is_enemies(self) -> bool {
        matches!(self, ViewerSet::Enemies | ViewerSet::Species(_))
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
                (build_stage, build_enemy_stage, build_material_stage)
                    .after(crate::models::knight::build_knight_assets),
            )
            .add_systems(
                Update,
                (animate_viewer, animate_viewer_creatures, frame_camera),
            );
    }
}

/// One posed knight on the stage.
#[derive(Component)]
pub(crate) struct ViewerKnight {
    pub(crate) input: KnightIn,
    /// Swing direction and the tick to freeze it at.
    pub(crate) swing: Option<(SwingDir, f32)>,
    pub(crate) facing: i8,
    pub(crate) tint: LinearRgba,
}

pub(crate) fn pose(f: impl FnOnce(&mut KnightIn)) -> KnightIn {
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
pub(crate) fn animate_viewer(
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
            ViewerSet::Species(sp) => {
                // Zoomed in when only a few columns are shown.
                let boss = matches!(sp, Species::Matron | Species::Bellwarden);
                let dist = match (boss, cols.0.is_some()) {
                    (false, true) => 8.5,
                    (false, false) => 12.5,
                    (true, true) => 22.0,
                    (true, false) => 40.0,
                };
                Transform::from_xyz(0.0, 0.6, dist).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y)
            }
            ViewerSet::Materials => {
                let (dist, y) = if cols.0.is_some() {
                    (8.5, 0.0)
                } else {
                    (17.5, -0.2)
                };
                Transform::from_xyz(0.0, y, dist).looking_at(Vec3::new(0.0, y, 0.0), Vec3::Y)
            }
        };
    }
}

// --------------------------------------------------------------- materials --

/// Every material as a sphere over a slab, lit from two sides, so the normal,
/// roughness, metal and height maps can be judged by eye.
fn build_material_stage(
    mut commands: Commands,
    set: Res<ViewerSet>,
    cols: Res<ViewerColumns>,
    materials: Res<crate::look::pbr::Materials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    use crate::look::pbr::Kind;
    use crate::rig::meshkit::{ellipsoid, MeshData};

    if *set != ViewerSet::Materials {
        return;
    }
    let kinds: Vec<Kind> = match &cols.0 {
        Some(pick) => pick
            .iter()
            .filter_map(|&i| Kind::ALL.get(i).copied())
            .collect(),
        None => Kind::ALL.to_vec(),
    };
    let per_row = if cols.0.is_some() {
        kinds.len().max(1)
    } else {
        7
    };
    let (step_x, step_y) = (2.3, 4.6);
    let sphere = meshes.add(ellipsoid(0.95, 0.95, 0.95, 40, 64).to_mesh_pbr());
    let mut slab = MeshData::default();
    let (h, white) = (0.95, [1.0; 4]);
    slab.add_quad(
        [
            Vec3::new(-h, -h, 0.0),
            Vec3::new(h, -h, 0.0),
            Vec3::new(h, h, 0.0),
            Vec3::new(-h, h, 0.0),
        ],
        Vec3::Z,
        [
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
        ],
        [white; 4],
    );
    let slab = meshes.add(slab.to_mesh_pbr());
    let rows = kinds.len().div_ceil(per_row);
    for (i, kind) in kinds.iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        let x = (col as f32 - (per_row as f32 - 1.0) / 2.0) * step_x;
        let y = ((rows as f32 - 1.0) / 2.0 - row as f32) * step_y;
        println!("viewer: row {row} column {col}: {kind:?}");
        let material = mats.add(materials.get(*kind).material());
        for (mesh, dy) in [(sphere.clone(), 1.2), (slab.clone(), -1.2)] {
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(x, y + dy, 0.0),
            ));
        }
    }
    for (pos, color, lux) in [
        (
            Vec3::new(-7.0, 5.0, 8.0),
            Color::srgb(1.0, 0.9, 0.75),
            9.0e6,
        ),
        (
            Vec3::new(8.0, -2.0, 6.0),
            Color::srgb(0.55, 0.7, 1.0),
            3.0e6,
        ),
    ] {
        commands.spawn((
            PointLight {
                intensity: lux,
                range: 80.0,
                color,
                ..default()
            },
            Transform::from_translation(pos),
        ));
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
    /// Bosses have no planned length in the viewer: use this many ticks.
    fixed_len: Option<f32>,
}

/// The poses of a boss sheet.
fn boss_columns() -> Vec<(&'static str, CreatureIn, f32)> {
    use crate::rig::creature::BossAtk;
    let base = CreatureIn::default();
    let with = |f: &dyn Fn(&mut CreatureIn)| {
        let mut c = base;
        f(&mut c);
        c
    };
    vec![
        ("sleep", with(&|c| c.sleeping = true), 0.0),
        ("roar", with(&|c| c.state = EnemyState::Notice), 1.0),
        (
            "walk",
            with(&|c| {
                c.state = EnemyState::Chase;
                c.vx = 3.0;
            }),
            0.0,
        ),
        (
            "tell-slam",
            with(&|c| {
                c.state = EnemyState::Windup;
                c.atk = BossAtk::Slam;
            }),
            1.0,
        ),
        (
            "slam-air",
            with(&|c| {
                c.state = EnemyState::Attack;
                c.atk = BossAtk::Slam;
                c.airborne = true;
            }),
            0.3,
        ),
        (
            "slam-land",
            with(&|c| {
                c.state = EnemyState::Attack;
                c.atk = BossAtk::Slam;
            }),
            0.7,
        ),
        (
            "tell-charge",
            with(&|c| {
                c.state = EnemyState::Windup;
                c.atk = BossAtk::Charge;
            }),
            1.0,
        ),
        (
            "charge",
            with(&|c| {
                c.state = EnemyState::Attack;
                c.atk = BossAtk::Charge;
                c.vx = 12.0;
            }),
            0.5,
        ),
        (
            "tell-sweep",
            with(&|c| {
                c.state = EnemyState::Windup;
                c.atk = BossAtk::Sweep;
            }),
            1.0,
        ),
        (
            "sweep",
            with(&|c| {
                c.state = EnemyState::Attack;
                c.atk = BossAtk::Sweep;
            }),
            0.4,
        ),
        (
            "toll",
            with(&|c| {
                c.state = EnemyState::Attack;
                c.atk = BossAtk::Toll;
            }),
            0.5,
        ),
        ("recover", with(&|c| c.state = EnemyState::Recover), 0.05),
        (
            "wall-stun",
            with(&|c| {
                c.state = EnemyState::Recover;
                c.wall = true;
            }),
            0.3,
        ),
        (
            "phase-2",
            with(&|c| {
                c.state = EnemyState::Chase;
                c.phase = 2;
            }),
            0.0,
        ),
        ("dying", with(&|c| c.dying = 0.7), 0.0),
    ]
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
const ENEMY_ROWS: [(Species, f32, f32); 7] = [
    (Species::Husk, 0.6, 0.0),
    (Species::Wisp, 0.45, 0.8),
    (Species::Shieldbearer, 0.75, 0.0),
    (Species::Spitter, 0.6, 0.0),
    (Species::Dummy, 0.6, 0.0),
    (Species::Matron, 1.0, 0.0),
    (Species::Bellwarden, 1.4, 0.0),
];

fn rows_are_bosses(set: &ViewerSet) -> bool {
    matches!(
        set,
        ViewerSet::Species(Species::Matron | Species::Bellwarden)
    )
}

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
    let boss_row = rows_are_bosses(&set);
    let step = if boss_row { 4.4 } else { 2.5 };
    let pitch = 3.3;
    let rows: Vec<(Species, f32, f32)> = ENEMY_ROWS
        .into_iter()
        .filter(|(sp, _, _)| match *set {
            ViewerSet::Species(only) => *sp == only,
            // The overview shows the small ones; the bosses have sheets of their own.
            _ => !matches!(sp, Species::Matron | Species::Bellwarden),
        })
        .collect();
    for (row, (species, half_y, lift)) in rows.into_iter().enumerate() {
        let y = -(row as f32) * pitch;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(7.0 * step + 2.0, 0.2, 4.0))),
            MeshMaterial3d(floor.clone()),
            Transform::from_xyz(0.0, y - 0.1, 0.0),
        ));
        let is_boss = matches!(species, Species::Matron | Species::Bellwarden);
        let columns: Vec<(&str, CreatureIn, f32, bool)> = if is_boss {
            boss_columns()
                .into_iter()
                .map(|(n, c, at)| (n, c, at, c.vx.abs() > 0.5))
                .collect()
        } else {
            ENEMY_COLUMNS
                .into_iter()
                .map(|(n, st, at, w)| {
                    (
                        n,
                        CreatureIn {
                            state: st,
                            vx: if w { 3.0 } else { 0.0 },
                            ..Default::default()
                        },
                        at,
                        w,
                    )
                })
                .collect()
        };
        for (col, (name, patch, at, walking)) in columns.into_iter().enumerate() {
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
                None => (col as f32 - if is_boss { 7.0 } else { 3.0 }) * step,
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
                        aim,
                        sway,
                        ..patch
                    },
                    facing: 1,
                    at,
                    walking,
                    fixed_len: is_boss.then_some(60.0),
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
        let len = vc
            .fixed_len
            .unwrap_or_else(|| state_len(kind, vc.input.state, &tuning.enemies));
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
