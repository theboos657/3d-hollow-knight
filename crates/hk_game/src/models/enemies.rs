//! The enemies and the training dummy, built from `rig::meshkit` geometry.
//!
//! * **Husk**: a charred, hunched shell with glowing cracks, a pale skull and
//!   long claws. It rears back to lunge.
//! * **Wisp**: a glass orb with a burning core, a little crown and five
//!   trailing tendrils. It squeezes small, then dives like a comet.
//! * **Shieldbearer**: a barrel of riveted iron behind a tower shield that
//!   stands on the guarded side; a glowing vent on the other side is the
//!   weak spot.
//! * **Spitter**: a warty pod on stub legs with a long stalk and a flared
//!   maw that tracks its target; its belly swells before it spits.
//! * **Dummy**: a straw-and-burlap training post with a target painted on it,
//!   in warm ochre so it never reads as an enemy.
//!
//! Each creature has two per-instance materials: `body` (washed with the tell
//! colour) and `glow` (cracks, eyes, core, weak spot), so what an enemy is
//! about to do stays readable exactly as before (`rig::creature::tell_glow`),
//! and the pose changes as well, so it is not colour alone.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use hk_sim::boss::{Boss, Pendulum};
use hk_sim::combat::{EnemyDied, Guard, Hit, Hurtbox, SimFrozen, Team};
use hk_sim::components::{Aabb, SimPos, Velocity};
use hk_sim::enemy::{Brain, EnemyKind, EnemyState};
use hk_sim::player::Player;
use hk_sim::tuning::{EnemyTuning, Tuning};
use hk_sim::SimTick;

use crate::interp::Interpolated;
use crate::rig::creature::{
    creature_pose, dummy, ease_guard, species_glow, spitter, step_sway, tell_glow, wisp,
    CreatureIn, CreaturePose, Species, BODY_WASH, MAX_JOINTS,
};
use crate::rig::meshkit::{cone, ellipsoid, extrude, lathe, limb, ring, tube, MeshData};
use crate::rig::pose::{angle_diff, Spring};
use crate::rig::{joint, part, posed, ModelRoot, Rest};

pub struct EnemyModelsPlugin;

impl Plugin for EnemyModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Ledger>()
            .add_systems(Startup, build_enemy_assets)
            .add_systems(
                Update,
                (
                    spawn_enemy_models,
                    animate_creatures,
                    super::bosses::attach_boss_models,
                    super::bosses::animate_bosses,
                    // Shards first: the ledger still remembers what died last frame.
                    spawn_shards,
                    record_ledger,
                    fly_shards,
                )
                    .chain()
                    .after(crate::interp::RenderPrepSet),
            );
    }
}

// ---------------------------------------------------------------- geometry --

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

/// Points a `+Y`-up primitive (a cone) along `dir`, based at `p`.
fn aim(p: Vec3, dir: Vec3) -> Mat4 {
    Mat4::from_translation(p) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, dir.normalize()))
}

/// Radius of a lathe profile at height `y` (linear between profile points).
fn profile_r(profile: &[(f32, f32)], y: f32) -> f32 {
    for w in profile.windows(2) {
        let ((r0, y0), (r1, y1)) = (w[0], w[1]);
        if y >= y0 && y <= y1 && (y1 - y0).abs() > 1e-6 {
            return r0 + (r1 - r0) * (y - y0) / (y1 - y0);
        }
    }
    profile.last().map_or(0.0, |p| p.0)
}

const HUSK_SHELL: [(f32, f32); 7] = [
    (0.30, -0.30),
    (0.46, -0.24),
    (0.52, -0.05),
    (0.49, 0.16),
    (0.38, 0.32),
    (0.20, 0.42),
    (0.0, 0.46),
];

/// A crack along the shell, on one of its two flanks (`side` = +1 or -1).
fn husk_crack(side: f32, a0: f32, wobble: [f32; 5]) -> MeshData {
    let ys = [0.38, 0.24, 0.10, -0.04, -0.18];
    let pts: Vec<Vec3> = ys
        .iter()
        .zip(wobble)
        .map(|(&y, w)| {
            let r = profile_r(&HUSK_SHELL, y) * 1.03;
            let a = a0 + w;
            Vec3::new(r * a.cos(), y, side * r * 0.92 * a.sin())
        })
        .collect();
    tube(&pts, |t| 0.022 * (1.0 - 0.55 * t), 5)
}

fn husk_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    // Shell: a domed, charred carapace, darker toward the ground.
    let shell = lathe(&HUSK_SHELL, 22)
        .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.92)))
        .jitter(3, 0.022)
        .recolor(|p| {
            let k = ((p.y + 0.30) / 0.76).clamp(0.0, 1.0);
            let s = 0.55 + 0.45 * k;
            [s, s * 0.95, s * 0.9, 1.0]
        });
    out.push(("husk_shell", shell));

    // Spines down the back.
    let mut spines = MeshData::default();
    for (x, y, h, tilt) in [
        (-0.12f32, 0.42f32, 0.24f32, 0.25f32),
        (-0.27, 0.35, 0.22, 0.6),
        (-0.39, 0.22, 0.20, 0.95),
        (-0.47, 0.06, 0.17, 1.25),
    ] {
        let dir = Vec3::new(-tilt.sin(), tilt.cos(), 0.0);
        spines.merge(&cone(0.065, h, 6).transformed(aim(Vec3::new(x, y, 0.0), dir)));
    }
    out.push(("husk_spines", spines));

    // Glowing cracks on both flanks.
    let mut cracks = MeshData::default();
    for side in [1.0, -1.0] {
        cracks.merge(&husk_crack(side, 0.95, [0.0, 0.10, -0.08, 0.12, -0.05]));
        cracks.merge(&husk_crack(side, 1.55, [0.05, -0.12, 0.10, -0.06, 0.08]));
        cracks.merge(&husk_crack(side, 2.15, [-0.05, 0.08, -0.10, 0.10, -0.04]));
    }
    out.push(("husk_cracks", cracks));

    // Head: a low, forward-slung skull with two horns and a jaw.
    let mut skull = ellipsoid(0.20, 0.16, 0.17, 10, 14).transformed(at(0.08, 0.0, 0.0));
    for side in [-1.0f32, 1.0] {
        skull.merge(&tube(
            &[
                Vec3::new(0.02, 0.10, 0.10 * side),
                Vec3::new(-0.06, 0.22, 0.14 * side),
                Vec3::new(-0.16, 0.28, 0.14 * side),
            ],
            |t| 0.035 * (1.0 - 0.8 * t),
            6,
        ));
    }
    out.push(("husk_skull", skull));
    out.push((
        "husk_jaw",
        ellipsoid(0.15, 0.05, 0.13, 6, 10).transformed(at(0.14, -0.13, 0.0)),
    ));
    let mut eyes = ellipsoid(0.04, 0.05, 0.04, 6, 8).transformed(at(0.24, 0.04, 0.09));
    eyes.merge(&ellipsoid(0.04, 0.05, 0.04, 6, 8).transformed(at(0.24, 0.04, -0.09)));
    out.push(("husk_eyes", eyes));

    // Arms hang down and end in three claws.
    let arm = tube(
        &[
            Vec3::ZERO,
            Vec3::new(0.10, -0.22, 0.0),
            Vec3::new(0.26, -0.42, 0.0),
        ],
        |t| 0.06 - 0.03 * t,
        7,
    );
    out.push(("husk_arm", arm));
    let mut claws = MeshData::default();
    for dz in [-0.05f32, 0.0, 0.05] {
        claws.merge(&cone(0.028, 0.20, 6).transformed(aim(
            Vec3::new(0.26, -0.42, dz),
            Vec3::new(0.45 + dz * 2.0, -0.85, dz * 3.0),
        )));
    }
    out.push(("husk_claws", claws));

    out.push((
        "husk_leg",
        limb(Vec3::ZERO, Vec3::new(0.0, -0.27, 0.0), 0.075, 0.05, 7),
    ));
    out.push((
        "husk_foot",
        ellipsoid(0.13, 0.05, 0.08, 6, 10).transformed(at(0.05, -0.29, 0.0)),
    ));
}

fn wisp_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    out.push(("wisp_orb", ellipsoid(0.38, 0.38, 0.38, 12, 22)));
    out.push(("wisp_core", ellipsoid(0.16, 0.16, 0.16, 8, 12)));
    // A tilted halo around the orb, and a little crown of flame-like horns.
    out.push((
        "wisp_halo",
        ring(0.50, 0.018, 28, 6)
            .transformed(Mat4::from_rotation_x(1.25) * Mat4::from_rotation_z(0.25)),
    ));
    let mut crown = MeshData::default();
    for k in 0..5 {
        let phi = k as f32 / 5.0 * std::f32::consts::TAU + 0.3;
        let o = Vec3::new(phi.cos(), 0.0, phi.sin());
        crown.merge(&cone(0.05, 0.20, 6).transformed(aim(
            Vec3::new(o.x * 0.17, 0.31, o.z * 0.17),
            o * 0.5 + Vec3::Y,
        )));
    }
    out.push(("wisp_crown", crown));
    out.push((
        "wisp_tendril",
        tube(
            &[
                Vec3::ZERO,
                Vec3::new(0.02, -0.16, 0.0),
                Vec3::new(-0.02, -0.32, 0.0),
                Vec3::new(0.03, -0.48, 0.0),
            ],
            |t| 0.05 * (1.0 - t * 0.9),
            6,
        ),
    ));
}

const BARREL: [(f32, f32); 6] = [
    (0.30, -0.42),
    (0.46, -0.32),
    (0.52, -0.05),
    (0.50, 0.22),
    (0.40, 0.40),
    (0.30, 0.46),
];

fn shield_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    let squash_z = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.9));
    out.push((
        "shield_barrel",
        lathe(&BARREL, 22)
            .transformed(squash_z)
            .jitter(5, 0.012)
            .recolor(|p| {
                let k = ((p.y + 0.42) / 0.9).clamp(0.0, 1.0);
                let s = 0.6 + 0.4 * k;
                [s, s, s, 1.0]
            }),
    ));
    let mut bands = MeshData::default();
    for y in [-0.28, 0.02, 0.30] {
        bands.merge(
            &ring(profile_r(&BARREL, y) + 0.005, 0.032, 22, 6)
                .transformed(squash_z)
                .transformed(at(0.0, y, 0.0)),
        );
    }
    out.push(("shield_bands", bands));

    // The pot helm: squat, with a slit visor and a short crest.
    let mut helm = lathe(
        &[
            (0.0, 0.0),
            (0.24, 0.0),
            (0.28, 0.10),
            (0.26, 0.22),
            (0.17, 0.32),
            (0.0, 0.36),
        ],
        18,
    );
    helm.merge(&tube(
        &[
            Vec3::new(0.0, 0.34, 0.0),
            Vec3::new(-0.10, 0.44, 0.0),
            Vec3::new(-0.24, 0.42, 0.0),
        ],
        |t| 0.04 * (1.0 - 0.7 * t),
        6,
    ));
    out.push(("shield_helm", helm));
    out.push((
        "shield_visor",
        ellipsoid(0.04, 0.03, 0.17, 6, 10).transformed(at(0.25, 0.15, 0.0)),
    ));
    let mut eyes = ellipsoid(0.03, 0.03, 0.035, 5, 8).transformed(at(0.285, 0.15, 0.07));
    eyes.merge(&ellipsoid(0.03, 0.03, 0.035, 5, 8).transformed(at(0.285, 0.15, -0.07)));
    out.push(("shield_eyes", eyes));

    // The tower shield: a slab standing in the Y-Z plane (its faces look
    // along X), with a rim, a boss and a cross of ridges on both faces.
    let outline = [
        Vec2::new(-0.40, 0.66),
        Vec2::new(-0.22, 0.76),
        Vec2::new(0.22, 0.76),
        Vec2::new(0.40, 0.66),
        Vec2::new(0.42, 0.0),
        Vec2::new(0.34, -0.42),
        Vec2::new(0.0, -0.80),
        Vec2::new(-0.34, -0.42),
        Vec2::new(-0.42, 0.0),
    ];
    // Turned a little toward the camera, so the side view still shows a face.
    let upright = Mat4::from_rotation_y(FRAC_PI_2 - 0.35);
    let scaled = |k: f32| outline.map(|p| p * k);
    out.push(("shield_rim", extrude(&outline, 0.16).transformed(upright)));
    out.push((
        "shield_face",
        extrude(&scaled(0.84), 0.21).transformed(upright),
    ));
    let mut trim = MeshData::default();
    for side in [-1.0f32, 1.0] {
        trim.merge(&ellipsoid(0.07, 0.15, 0.15, 6, 10).transformed(at(0.105 * side, 0.08, 0.0)));
        trim.merge(
            &extrude(
                &[
                    Vec2::new(-0.03, 0.62),
                    Vec2::new(0.03, 0.62),
                    Vec2::new(0.03, -0.62),
                    Vec2::new(-0.03, -0.62),
                ],
                0.25,
            )
            .transformed(upright),
        );
        trim.merge(
            &extrude(
                &[
                    Vec2::new(-0.34, 0.05),
                    Vec2::new(0.34, 0.05),
                    Vec2::new(0.34, 0.11),
                    Vec2::new(-0.34, 0.11),
                ],
                0.25,
            )
            .transformed(upright),
        );
    }
    out.push(("shield_trim", trim));

    // The weak spot: a glowing vent, embedded in the barrel's flank.
    out.push((
        "shield_vent",
        ellipsoid(0.07, 0.20, 0.20, 8, 12).transformed(at(0.0, 0.08, 0.0)),
    ));
    out.push((
        "shield_leg",
        limb(Vec3::ZERO, Vec3::new(0.0, -0.32, 0.0), 0.13, 0.10, 8),
    ));
    out.push((
        "shield_boot",
        ellipsoid(0.17, 0.07, 0.13, 6, 10).transformed(at(0.04, -0.35, 0.0)),
    ));
}

fn spitter_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    let pod = lathe(
        &[
            (0.05, -0.30),
            (0.26, -0.28),
            (0.42, -0.14),
            (0.48, 0.06),
            (0.42, 0.24),
            (0.26, 0.36),
            (0.12, 0.40),
            (0.0, 0.41),
        ],
        22,
    )
    .jitter(9, 0.02)
    .recolor(|p| {
        let k = ((p.y + 0.30) / 0.7).clamp(0.0, 1.0);
        [0.62 + 0.38 * k, 0.72 + 0.28 * k, 0.6 + 0.4 * k, 1.0]
    });
    out.push(("spitter_pod", pod));
    // Warts that glow: spores on both flanks.
    let mut spots = MeshData::default();
    for (x, y, z) in [
        (-0.18, 0.18, 0.40),
        (0.06, 0.28, 0.34),
        (-0.34, -0.02, 0.32),
        (-0.06, -0.12, 0.44),
        (0.20, 0.05, 0.42),
    ] {
        for side in [1.0f32, -1.0] {
            spots.merge(&ellipsoid(0.05, 0.05, 0.05, 5, 8).transformed(at(x, y, z * side)));
        }
    }
    out.push(("spitter_spots", spots));
    // The stalk and the flared maw (revolved about +X).
    out.push((
        "spitter_neck",
        tube(
            &[
                Vec3::ZERO,
                Vec3::new(0.14, 0.06, 0.0),
                Vec3::new(0.30, 0.16, 0.0),
                Vec3::new(0.42, 0.18, 0.0),
            ],
            |t| 0.12 - 0.04 * t,
            8,
        ),
    ));
    let flare = lathe(
        &[
            (0.06, 0.0),
            (0.10, 0.05),
            (0.17, 0.16),
            (0.21, 0.24),
            (0.18, 0.27),
        ],
        16,
    )
    .transformed(at(0.40, 0.18, 0.0) * Mat4::from_rotation_z(-FRAC_PI_2));
    out.push(("spitter_maw", flare));
    out.push((
        "spitter_mouth",
        ellipsoid(0.02, 0.13, 0.13, 6, 10).transformed(at(0.62, 0.18, 0.0)),
    ));
    out.push(("spitter_belly", ellipsoid(0.20, 0.20, 0.20, 8, 12)));
    out.push((
        "spitter_leg",
        tube(
            &[
                Vec3::ZERO,
                Vec3::new(0.05, -0.10, 0.0),
                Vec3::new(0.02, -0.22, 0.0),
            ],
            |t| 0.07 - 0.03 * t,
            6,
        ),
    ));
    out.push((
        "spitter_toe",
        ellipsoid(0.10, 0.04, 0.07, 5, 8).transformed(at(0.04, -0.23, 0.0)),
    ));
}

fn dummy_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    out.push((
        "dummy_post",
        limb(
            Vec3::new(0.0, 0.02, 0.0),
            Vec3::new(0.0, 0.92, 0.0),
            0.13,
            0.10,
            8,
        ),
    ));
    out.push((
        "dummy_base",
        ellipsoid(0.36, 0.08, 0.36, 6, 14).transformed(at(0.0, 0.05, 0.0)),
    ));
    out.push((
        "dummy_bar",
        limb(
            Vec3::new(-0.50, 0.0, 0.0),
            Vec3::new(0.50, 0.0, 0.0),
            0.05,
            0.05,
            6,
        ),
    ));
    let mut straw = MeshData::default();
    for (x, sgn) in [(-0.50f32, -1.0f32), (0.50, 1.0)] {
        for (dy, dz) in [(0.0f32, 0.0f32), (0.04, 0.05), (-0.04, -0.05)] {
            straw.merge(&cone(0.05, 0.26, 5).transformed(aim(
                Vec3::new(x, dy, dz),
                Vec3::new(sgn, 0.25 + dy * 3.0, dz * 3.0),
            )));
        }
    }
    out.push(("dummy_straw", straw));
    out.push((
        "dummy_head",
        ellipsoid(0.20, 0.22, 0.20, 10, 14)
            .jitter(4, 0.012)
            .transformed(at(0.0, 0.10, 0.0)),
    ));
    let mut face = ellipsoid(0.03, 0.035, 0.03, 5, 8).transformed(at(0.185, 0.16, 0.07));
    face.merge(&ellipsoid(0.03, 0.035, 0.03, 5, 8).transformed(at(0.185, 0.16, -0.07)));
    face.merge(&limb(
        Vec3::new(0.19, 0.06, -0.08),
        Vec3::new(0.19, 0.06, 0.08),
        0.012,
        0.012,
        5,
    ));
    out.push(("dummy_face", face));
    let mut tuft = MeshData::default();
    for k in 0..4 {
        let phi = k as f32 / 4.0 * std::f32::consts::TAU;
        tuft.merge(&cone(0.045, 0.2, 5).transformed(aim(
            Vec3::new(phi.cos() * 0.08, 0.28, phi.sin() * 0.08),
            Vec3::new(phi.cos() * 0.5, 1.0, phi.sin() * 0.5),
        )));
    }
    out.push(("dummy_tuft", tuft));
    // The target: three flat discs on the camera-facing side of the chest.
    let mut cream = MeshData::default();
    let mut red = MeshData::default();
    cream.merge(&ellipsoid(0.20, 0.20, 0.03, 8, 16).transformed(at(0.0, 0.58, 0.115)));
    red.merge(&ellipsoid(0.135, 0.135, 0.036, 8, 16).transformed(at(0.0, 0.58, 0.117)));
    cream.merge(&ellipsoid(0.07, 0.07, 0.04, 6, 12).transformed(at(0.0, 0.58, 0.12)));
    out.push(("dummy_target_cream", cream));
    out.push(("dummy_target_red", red));
}

fn shard_meshes(out: &mut Vec<(&'static str, MeshData)>) {
    // A splinter: a three-sided spike, used for every species' death burst.
    out.push(("shard", cone(0.10, 0.34, 3)));
}

/// Every mesh of every creature, by name (also what the tests validate).
pub fn enemy_meshes() -> Vec<(&'static str, MeshData)> {
    let mut out = Vec::new();
    shard_meshes(&mut out);
    husk_meshes(&mut out);
    wisp_meshes(&mut out);
    shield_meshes(&mut out);
    spitter_meshes(&mut out);
    dummy_meshes(&mut out);
    super::bosses::matron_meshes(&mut out);
    super::bosses::warden_meshes(&mut out);
    out
}

// ------------------------------------------------------------------ assets --

#[derive(Resource)]
pub struct EnemyAssets {
    meshes: HashMap<&'static str, Handle<Mesh>>,
    mats: HashMap<&'static str, Handle<StandardMaterial>>,
}

/// A list of named meshes (what the geometry builders return).
pub type MeshList = Vec<(&'static str, MeshData)>;

/// Adds named materials to the asset map (used by the boss module too).
pub struct MatBuilder<'a> {
    mats: &'a mut Assets<StandardMaterial>,
    map: &'a mut HashMap<&'static str, Handle<StandardMaterial>>,
}

impl MatBuilder<'_> {
    pub fn add(&mut self, k: &'static str, s: StandardMaterial) {
        self.map.insert(k, self.mats.add(s));
    }
    pub fn lit(&mut self, k: &'static str, c: Color, rough: f32, metallic: f32) {
        self.add(k, lit(c, rough, metallic));
    }
    pub fn glow(&mut self, k: &'static str, species: Species) {
        self.add(k, glow_material(species));
    }
}

impl EnemyAssets {
    pub(super) fn m(&self, k: &str) -> Handle<Mesh> {
        self.meshes
            .get(k)
            .unwrap_or_else(|| panic!("no enemy mesh `{k}`"))
            .clone()
    }
    pub(super) fn mat(&self, k: &str) -> Handle<StandardMaterial> {
        self.mats
            .get(k)
            .unwrap_or_else(|| panic!("no enemy material `{k}`"))
            .clone()
    }
}

fn lit(base: Color, rough: f32, metallic: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: base,
        perceptual_roughness: rough,
        metallic,
        ..default()
    }
}

/// A material for glowing parts: dark base, emissive driven per instance.
fn glow_material(species: Species) -> StandardMaterial {
    let g = species_glow(species);
    StandardMaterial {
        base_color: Color::srgb(0.10, 0.06, 0.04),
        perceptual_roughness: 0.5,
        emissive: LinearRgba::rgb(g[0], g[1], g[2]),
        ..default()
    }
}

pub fn build_enemy_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(make_assets(&mut meshes, &mut mats));
}

pub fn make_assets(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> EnemyAssets {
    let mesh_map = enemy_meshes()
        .into_iter()
        .map(|(k, d)| (k, meshes.add(d.to_mesh())))
        .collect();
    let mut m: HashMap<&'static str, Handle<StandardMaterial>> = HashMap::new();
    let mut add_std = |k: &'static str, s: StandardMaterial| {
        m.insert(k, mats.add(s));
    };
    let mut add = |k: &'static str, s: StandardMaterial| add_std(k, s);
    // Husk.
    add("husk_shell", lit(Color::srgb(0.20, 0.13, 0.11), 0.92, 0.0));
    add("husk_glow", glow_material(Species::Husk));
    add("husk_flesh", lit(Color::srgb(0.30, 0.16, 0.12), 0.8, 0.0));
    add("husk_bone", lit(Color::srgb(0.70, 0.64, 0.54), 0.6, 0.0));
    add("husk_char", lit(Color::srgb(0.10, 0.08, 0.08), 0.9, 0.0));
    // Wisp.
    add(
        "wisp_glass",
        StandardMaterial {
            base_color: Color::srgba(0.55, 0.42, 0.95, 0.62),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.12,
            reflectance: 0.6,
            ..default()
        },
    );
    add("wisp_glow", glow_material(Species::Wisp));
    add("wisp_flesh", lit(Color::srgb(0.20, 0.13, 0.34), 0.7, 0.0));
    // Shieldbearer.
    add(
        "shield_barrel",
        lit(Color::srgb(0.22, 0.32, 0.35), 0.5, 0.55),
    );
    add("shield_glow", glow_material(Species::Shieldbearer));
    add(
        "shield_iron",
        lit(Color::srgb(0.16, 0.22, 0.25), 0.45, 0.65),
    );
    add(
        "shield_face",
        lit(Color::srgb(0.58, 0.68, 0.72), 0.35, 0.65),
    );
    // Spitter.
    add("spitter_pod", lit(Color::srgb(0.26, 0.44, 0.17), 0.7, 0.0));
    add("spitter_glow", glow_material(Species::Spitter));
    add(
        "spitter_stalk",
        lit(Color::srgb(0.34, 0.52, 0.22), 0.65, 0.0),
    );
    add("spitter_leg", lit(Color::srgb(0.18, 0.30, 0.13), 0.8, 0.0));
    // Death shards: the creature's own material, still glowing a little.
    let shard = |c: Color, e: LinearRgba| StandardMaterial {
        emissive: e,
        ..lit(c, 0.7, 0.0)
    };
    add(
        "shard_husk",
        shard(
            Color::srgb(0.22, 0.14, 0.11),
            LinearRgba::rgb(0.9, 0.3, 0.06),
        ),
    );
    add(
        "shard_wisp",
        shard(
            Color::srgb(0.55, 0.42, 0.95),
            LinearRgba::rgb(0.6, 0.35, 1.4),
        ),
    );
    add(
        "shard_shield",
        shard(
            Color::srgb(0.30, 0.42, 0.46),
            LinearRgba::rgb(0.02, 0.06, 0.07),
        ),
    );
    add(
        "shard_spitter",
        shard(
            Color::srgb(0.30, 0.50, 0.20),
            LinearRgba::rgb(0.2, 0.7, 0.1),
        ),
    );
    // Dummy: warm ochre, distinct from every enemy.
    add(
        "dummy_burlap",
        lit(Color::srgb(0.74, 0.56, 0.32), 0.95, 0.0),
    );
    add("dummy_wood", lit(Color::srgb(0.36, 0.24, 0.13), 0.95, 0.0));
    add("dummy_straw", lit(Color::srgb(0.88, 0.74, 0.36), 0.9, 0.0));
    add("dummy_red", lit(Color::srgb(0.72, 0.16, 0.12), 0.8, 0.0));
    add("dummy_cream", lit(Color::srgb(0.93, 0.87, 0.72), 0.85, 0.0));
    add("dummy_dark", lit(Color::srgb(0.10, 0.07, 0.05), 0.9, 0.0));
    super::bosses::boss_materials(&mut MatBuilder { mats, map: &mut m });
    EnemyAssets {
        meshes: mesh_map,
        mats: m,
    }
}

// --------------------------------------------------------------------- rig --

/// The entities of one creature.
#[derive(Component)]
pub struct CreatureRig {
    pub species: Species,
    pub squash: Entity,
    pub facing: Entity,
    pub lean: Entity,
    pub joints: [Entity; MAX_JOINTS],
    /// Per-instance materials: the body (washed by the tell) and the glow.
    pub body: Handle<StandardMaterial>,
    pub glow: Handle<StandardMaterial>,
}

/// Per-creature animation state.
#[derive(Component)]
pub struct CreatureAnim {
    pub clock: f32,
    pub walk: f32,
    pub yaw: f32,
    pub hit: f32,
    pub sway: Spring,
    pub guard: f32,
    last_glow: [f32; 3],
    last_body: [f32; 3],
}

impl CreatureAnim {
    pub fn new(face: i8, guard: f32) -> Self {
        Self {
            clock: 0.0,
            walk: 0.0,
            yaw: facing_yaw(face),
            hit: 0.0,
            sway: Spring::default(),
            guard,
            last_glow: [-1.0; 3],
            last_body: [-1.0; 3],
        }
    }
}

fn facing_yaw(face: i8) -> f32 {
    if face >= 0 {
        -0.30
    } else {
        PI + 0.30
    }
}

fn clone_mat(
    mats: &mut Assets<StandardMaterial>,
    of: &Handle<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let m = mats.get(of).cloned().unwrap_or_default();
    mats.add(m)
}

/// Builds the hierarchy of one creature under `anchor` (whose sim position
/// is the centre of the creature's box, `half_y` above its feet).
pub fn spawn_creature(
    commands: &mut Commands,
    mats: &mut Assets<StandardMaterial>,
    a: &EnemyAssets,
    anchor: Entity,
    species: Species,
    half_y: f32,
) -> CreatureRig {
    let (body_key, glow_key) = match species {
        Species::Husk => ("husk_shell", "husk_glow"),
        Species::Wisp => ("wisp_glass", "wisp_glow"),
        Species::Shieldbearer => ("shield_barrel", "shield_glow"),
        Species::Spitter => ("spitter_pod", "spitter_glow"),
        Species::Dummy => ("dummy_burlap", "husk_glow"),
        Species::Matron | Species::Bellwarden => super::bosses::body_glow_keys(species),
    };
    let body = clone_mat(mats, &a.mat(body_key));
    let glow = clone_mat(mats, &a.mat(glow_key));

    let model_root = commands
        .spawn((
            ModelRoot,
            Transform::from_xyz(0.0, -half_y, 0.0),
            Visibility::default(),
        ))
        .id();
    commands.entity(anchor).add_child(model_root);
    let node = |commands: &mut Commands, parent: Entity| {
        let e = commands
            .spawn((Transform::default(), Visibility::default()))
            .id();
        commands.entity(parent).add_child(e);
        e
    };
    let squash = node(commands, model_root);
    let facing = node(commands, squash);
    let lean = node(commands, facing);

    let joints = match species {
        Species::Husk => build_husk(commands, a, lean, &body, &glow),
        Species::Wisp => build_wisp(commands, a, lean, &body, &glow),
        Species::Shieldbearer => build_shield(commands, a, lean, &body, &glow),
        Species::Spitter => build_spitter(commands, a, lean, &body, &glow),
        Species::Dummy => build_dummy(commands, a, lean, &body),
        Species::Matron => super::bosses::build_matron(commands, a, lean, &body, &glow),
        Species::Bellwarden => super::bosses::build_warden(commands, a, lean, &body, &glow),
    };
    CreatureRig {
        species,
        squash,
        facing,
        lean,
        joints,
        body,
        glow,
    }
}

type Mat = Handle<StandardMaterial>;

fn t(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn build_husk(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use crate::rig::creature::husk::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.55, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("husk_shell"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("husk_spines"),
        a.mat("husk_char"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("husk_cracks"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let head = joint(c, body, t(0.34, 0.06, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("husk_skull"),
        a.mat("husk_bone"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("husk_jaw"),
        a.mat("husk_flesh"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("husk_eyes"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(ARM_FRONT, 0.20, 0.30), (ARM_BACK, 0.08, -0.30)] {
        let arm = joint(c, body, t(x, -0.02, z));
        j[idx] = arm;
        part(
            c,
            arm,
            a.m("husk_arm"),
            a.mat("husk_flesh"),
            Transform::IDENTITY,
        );
        part(
            c,
            arm,
            a.m("husk_claws"),
            a.mat("husk_bone"),
            Transform::IDENTITY,
        );
    }
    for (idx, x, z) in [(LEG_FRONT, 0.14, 0.16), (LEG_BACK, -0.14, -0.16)] {
        let leg = joint(c, lean, t(x, 0.30, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("husk_leg"),
            a.mat("husk_flesh"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("husk_foot"),
            a.mat("husk_char"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_wisp(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let orb = joint(c, lean, t(0.0, 0.45, 0.0));
    j[wisp::ORB] = orb;
    part(c, orb, a.m("wisp_orb"), body_m.clone(), Transform::IDENTITY);
    part(
        c,
        orb,
        a.m("wisp_halo"),
        a.mat("wisp_flesh"),
        Transform::IDENTITY,
    );
    part(
        c,
        orb,
        a.m("wisp_crown"),
        a.mat("wisp_flesh"),
        Transform::IDENTITY,
    );
    let core = joint(c, orb, Transform::IDENTITY);
    j[wisp::CORE] = core;
    part(
        c,
        core,
        a.m("wisp_core"),
        glow_m.clone(),
        Transform::IDENTITY,
    );
    let xs = [-0.20, -0.10, 0.0, 0.10, 0.20];
    let zs = [0.10, -0.12, 0.14, -0.10, 0.08];
    let lens = [1.0, 1.25, 1.45, 1.2, 0.95];
    for k in 0..wisp::TENDRILS {
        let e = joint(
            c,
            orb,
            t(xs[k], -0.30, zs[k]).with_scale(Vec3::new(1.0, lens[k], 1.0)),
        );
        j[wisp::TENDRIL + k] = e;
        part(
            c,
            e,
            a.m("wisp_tendril"),
            a.mat("wisp_flesh"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_shield(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use crate::rig::creature::shield::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.85, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("shield_barrel"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_bands"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );

    let head = joint(c, body, t(0.05, 0.44, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("shield_helm"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("shield_visor"),
        a.mat("husk_char"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("shield_eyes"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    // Placed by the pose (it slides to whichever side is guarded).
    let sh = joint(c, body, t(0.0, 0.0, 0.0));
    j[SHIELD] = sh;
    part(
        c,
        sh,
        a.m("shield_rim"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );
    part(
        c,
        sh,
        a.m("shield_face"),
        a.mat("shield_face"),
        Transform::IDENTITY,
    );
    part(
        c,
        sh,
        a.m("shield_trim"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );

    let weak = joint(c, body, t(0.0, 0.0, 0.0));
    j[WEAK] = weak;
    part(
        c,
        weak,
        a.m("shield_vent"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(LEG_FRONT, 0.16, 0.20), (LEG_BACK, -0.16, -0.20)] {
        let leg = joint(c, lean, t(x, 0.42, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("shield_leg"),
            a.mat("shield_iron"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("shield_boot"),
            a.mat("husk_char"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_spitter(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use spitter::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.45, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("spitter_pod"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("spitter_spots"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let maw = joint(c, body, t(0.15, 0.30, 0.0));
    j[MAW] = maw;
    // The neck and maw are authored around the joint at the stalk's root.
    part(
        c,
        maw,
        a.m("spitter_neck"),
        a.mat("spitter_stalk"),
        Transform::IDENTITY,
    );
    part(
        c,
        maw,
        a.m("spitter_maw"),
        a.mat("spitter_stalk"),
        Transform::IDENTITY,
    );
    part(
        c,
        maw,
        a.m("spitter_mouth"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let belly = joint(c, body, t(0.30, -0.06, 0.0));
    j[BELLY] = belly;
    part(
        c,
        belly,
        a.m("spitter_belly"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(LEG_FRONT, 0.16, 0.16), (LEG_BACK, -0.16, -0.16)] {
        let leg = joint(c, lean, t(x, 0.24, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("spitter_leg"),
            a.mat("spitter_leg"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("spitter_toe"),
            a.mat("spitter_leg"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_dummy(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use dummy::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    // The whole post pivots about its base.
    let post = joint(c, lean, Transform::IDENTITY);
    j[POST] = post;
    part(
        c,
        post,
        a.m("dummy_post"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_base"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_target_cream"),
        a.mat("dummy_cream"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_target_red"),
        a.mat("dummy_red"),
        Transform::IDENTITY,
    );

    let arms = joint(c, post, t(0.0, 0.76, 0.0));
    j[ARMS] = arms;
    part(
        c,
        arms,
        a.m("dummy_bar"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        arms,
        a.m("dummy_straw"),
        a.mat("dummy_straw"),
        Transform::IDENTITY,
    );

    let head = joint(c, post, t(0.0, 0.98, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("dummy_head"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("dummy_face"),
        a.mat("dummy_dark"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("dummy_tuft"),
        a.mat("dummy_straw"),
        Transform::IDENTITY,
    );
    j
}

// ----------------------------------------------------------------- systems --

/// Gives every new enemy and dummy its model.
#[allow(clippy::type_complexity)]
pub fn spawn_enemy_models(
    mut commands: Commands,
    assets: Option<Res<EnemyAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    enemies: Query<(Entity, &Brain, &Aabb, &SimPos, Option<&Guard>), Added<Brain>>,
    dummies: Query<
        (Entity, &Hurtbox, &Aabb, &SimPos),
        (
            Added<Hurtbox>,
            Without<Brain>,
            Without<Boss>,
            Without<Pendulum>,
            Without<Player>,
        ),
    >,
    grid: Option<Res<hk_sim::world::grid::TileGrid>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(assets) = assets else {
        return;
    };
    let prep = |commands: &mut Commands, e: Entity, pos: &SimPos| {
        // Transform and Visibility first, so children never see a bare parent.
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Visibility::default(),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
    };
    for (e, brain, aabb, pos, guard) in &enemies {
        prep(&mut commands, e, pos);
        let rig = spawn_creature(
            &mut commands,
            &mut mats,
            &assets,
            e,
            brain.kind.into(),
            aabb.half.y,
        );
        let g = guard.map_or(1.0, |g| (g.facing * brain.facing) as f32);
        commands
            .entity(e)
            .insert((rig, CreatureAnim::new(brain.facing, g)));
    }
    for (e, hurt, aabb, pos) in &dummies {
        if hurt.team != Team::Enemy {
            continue;
        }
        prep(&mut commands, e, pos);
        let rig = spawn_creature(
            &mut commands,
            &mut mats,
            &assets,
            e,
            Species::Dummy,
            aabb.half.y,
        );
        // A dummy with no ground under it (a pogo target over a pit) hangs from
        // a chain, so it never looks like it is floating.
        let feet = pos.0.y - aabb.half.y;
        let hanging = grid.as_ref().is_some_and(|g| {
            let (i, j) = (pos.0.x.floor() as i32, (feet - 0.15).floor() as i32);
            !matches!(
                g.get(i, j),
                hk_sim::world::grid::Tile::Solid | hk_sim::world::grid::Tile::OneWay
            )
        });
        if hanging {
            let chain = crate::look::kits::chain(0.0, 1.3, 60.0, 0.0);
            let chain_e = commands
                .spawn((
                    Mesh3d(meshes.add(chain.to_mesh())),
                    MeshMaterial3d(assets.mat("dummy_dark")),
                    Transform::IDENTITY,
                    Visibility::default(),
                ))
                .id();
            commands.entity(rig.lean).add_child(chain_e);
        }
        commands.entity(e).insert((rig, CreatureAnim::new(1, 1.0)));
    }
}

/// The planned length in ticks of the current state (0 when open-ended).
pub fn state_len(kind: EnemyKind, state: EnemyState, t: &EnemyTuning) -> f32 {
    let n = match (kind, state) {
        (EnemyKind::Husk, EnemyState::Notice) => t.husk.notice_ticks(),
        (EnemyKind::Husk, EnemyState::Windup) => t.husk.windup_ticks(),
        (EnemyKind::Husk, EnemyState::Attack) => t.husk.lunge_ticks(),
        (EnemyKind::Husk, EnemyState::Recover) => t.husk.recover_ticks(),
        (EnemyKind::Wisp, EnemyState::Notice) => t.wisp.notice_ticks(),
        (EnemyKind::Wisp, EnemyState::Windup) => t.wisp.windup_ticks(),
        (EnemyKind::Wisp, EnemyState::Attack) => t.wisp.dive_ticks(),
        (EnemyKind::Wisp, EnemyState::Recover) => t.wisp.recover_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Notice) => t.shield.notice_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Windup) => t.shield.windup_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Attack) => t.shield.bash_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Recover) => t.shield.recover_ticks(),
        (EnemyKind::Spitter, EnemyState::Notice) => t.spitter.notice_ticks(),
        (EnemyKind::Spitter, EnemyState::Windup) => t.spitter.windup_ticks(),
        (EnemyKind::Spitter, EnemyState::Recover) => t.spitter.recover_ticks(),
        _ => 0,
    };
    n as f32
}

fn sign(v: f32, fallback: i8) -> i8 {
    if v > 0.2 {
        1
    } else if v < -0.2 {
        -1
    } else {
        fallback
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_creatures(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    frozen: Res<SimFrozen>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut hits: MessageReader<Hit>,
    mut creatures: Query<
        (
            Entity,
            Option<&Brain>,
            Option<&Guard>,
            &Velocity,
            &CreatureRig,
            &mut CreatureAnim,
        ),
        Without<hk_sim::boss::BossBrain>,
    >,
    mut transforms: Query<(&mut Transform, Option<&Rest>), Without<CreatureRig>>,
) {
    let dt = time.delta_secs().min(0.05);
    let live = !frozen.0;
    let alpha = if live { fixed.overstep_fraction() } else { 1.0 };
    let hit_events: Vec<Hit> = hits.read().copied().collect();
    for (e, brain, guard, vel, rig, mut anim) in &mut creatures {
        if live {
            anim.clock += dt;
        }
        for h in hit_events.iter().filter(|h| h.victim == e) {
            anim.hit = 1.0;
            if rig.species == Species::Dummy {
                anim.sway.v += h.dir as f32 * 6.0;
            }
        }
        anim.hit = (anim.hit - dt * 4.5).max(0.0);
        if rig.species == Species::Dummy && live {
            step_sway(&mut anim.sway, dt);
        }

        let (state, timer, facing, aim_v) = brain
            .map(|b| (b.state, b.timer, b.facing, b.aim))
            .unwrap_or((EnemyState::Idle, 0, 1, Vec2::X));
        // Wisps and Spitters turn to face what they are about to attack.
        let attacking = matches!(state, EnemyState::Windup | EnemyState::Attack);
        let face = match rig.species {
            Species::Wisp | Species::Spitter if attacking => sign(aim_v.x, facing),
            _ => facing,
        };
        let fwd = face as f32;
        let vx = vel.x * fwd;
        if live && vx.abs() > 0.3 {
            anim.walk += (5.0 + vx.abs() * 2.2).min(34.0) * dt;
        }
        if let Some(g) = guard {
            let target = (g.facing * facing) as f32;
            anim.guard = ease_guard(anim.guard, target, dt);
        }
        let len = brain
            .map(|b| state_len(b.kind, state, &tuning.enemies))
            .unwrap_or(0.0);
        let input = CreatureIn {
            species: rig.species,
            state,
            t: (timer as f32 - 1.0 + alpha).max(0.0),
            len,
            clock: anim.clock,
            walk: anim.walk,
            vx,
            aim: aim_v.y.atan2((aim_v.x * fwd).max(0.05)),
            guard: anim.guard,
            hit: anim.hit,
            sway: anim.sway.x,
            ..Default::default()
        };
        let pose = creature_pose(&input);
        let glow = if rig.species == Species::Dummy {
            [0.0; 3]
        } else {
            tell_glow(rig.species, state, tick.0)
        };
        apply_creature(
            &pose,
            face,
            glow,
            anim.hit,
            rig,
            &mut anim,
            &mut mats,
            &mut transforms,
            dt,
        );
    }
}

/// Puts a pose on a creature's rig: joints, facing, lean, squash and the two
/// tell materials. Shared by the game and the viewer.
#[allow(clippy::too_many_arguments)]
pub fn apply_creature(
    pose: &CreaturePose,
    face: i8,
    glow: [f32; 3],
    hit: f32,
    rig: &CreatureRig,
    anim: &mut CreatureAnim,
    mats: &mut Assets<StandardMaterial>,
    transforms: &mut Query<(&mut Transform, Option<&Rest>), Without<CreatureRig>>,
    dt: f32,
) {
    for (k, e) in rig.joints.iter().enumerate() {
        if *e == Entity::PLACEHOLDER {
            continue;
        }
        if let Ok((mut tr, Some(rest))) = transforms.get_mut(*e) {
            *tr = posed(&rest.0, &pose.joints[k]);
        }
    }
    let target = facing_yaw(face);
    anim.yaw += angle_diff(anim.yaw, target) * (1.0 - (-22.0 * dt).exp());
    if let Ok((mut tr, _)) = transforms.get_mut(rig.facing) {
        tr.rotation = Quat::from_rotation_y(anim.yaw);
    }
    if let Ok((mut tr, _)) = transforms.get_mut(rig.lean) {
        tr.rotation = Quat::from_rotation_z(-pose.lean);
        tr.translation = Vec3::new(0.0, pose.drop, 0.0);
    }
    if let Ok((mut tr, _)) = transforms.get_mut(rig.squash) {
        tr.scale = Vec3::new(pose.squash[0], pose.squash[1], pose.squash[0]);
    }

    // A fresh hit flashes the whole body white for a moment.
    let flash = hit * hit;
    let glow_e = [
        glow[0] + flash * 2.5,
        glow[1] + flash * 2.5,
        glow[2] + flash * 2.5,
    ];
    let body_e = [
        glow[0] * BODY_WASH + flash * 0.8,
        glow[1] * BODY_WASH + flash * 0.8,
        glow[2] * BODY_WASH + flash * 0.8,
    ];
    let differs = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 0.01);
    if differs(anim.last_glow, glow_e) {
        if let Some(m) = mats.get_mut(&rig.glow) {
            m.emissive = LinearRgba::rgb(glow_e[0], glow_e[1], glow_e[2]);
        }
        anim.last_glow = glow_e;
    }
    if differs(anim.last_body, body_e) {
        if let Some(m) = mats.get_mut(&rig.body) {
            m.emissive = LinearRgba::rgb(body_e[0], body_e[1], body_e[2]);
        }
        anim.last_body = body_e;
    }
}

// ------------------------------------------------------------ death shards --

/// Which species each live creature is, so a death (reported a frame after the
/// entity is gone) can still be drawn as the right splinters.
#[derive(Resource, Default)]
pub struct Ledger(HashMap<Entity, Species>);

fn record_ledger(mut ledger: ResMut<Ledger>, q: Query<(Entity, &CreatureRig)>) {
    ledger.0.retain(|e, _| q.contains(*e));
    for (e, rig) in &q {
        ledger.0.insert(e, rig.species);
    }
}

#[derive(Component)]
struct Shard {
    vel: Vec3,
    spin: Vec3,
    life: f32,
    max: f32,
    size: f32,
}

fn spawn_shards(
    mut commands: Commands,
    assets: Option<Res<EnemyAssets>>,
    mut ledger: ResMut<Ledger>,
    mut died: MessageReader<EnemyDied>,
    mut seed: Local<u32>,
) {
    let Some(assets) = assets else {
        died.clear();
        return;
    };
    let mut rand = || {
        // xorshift: purely visual randomness.
        *seed = (*seed).max(0x9E37_79B9);
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for d in died.read() {
        let Some(species) = ledger.0.remove(&d.entity) else {
            continue;
        };
        let key = match species {
            Species::Husk => "shard_husk",
            Species::Wisp => "shard_wisp",
            Species::Shieldbearer => "shard_shield",
            Species::Spitter => "shard_spitter",
            Species::Dummy | Species::Matron | Species::Bellwarden => continue,
        };
        for _ in 0..11 {
            let ang = rand() * std::f32::consts::TAU;
            let speed = 3.0 + rand() * 6.0;
            let life = 0.55 + rand() * 0.5;
            let size = 0.7 + rand() * 0.9;
            commands.spawn((
                Shard {
                    vel: Vec3::new(
                        ang.cos() * speed,
                        ang.sin().abs() * speed + 2.5,
                        (rand() - 0.5) * 3.0,
                    ),
                    spin: Vec3::new(rand() - 0.5, rand() - 0.5, rand() - 0.5) * 16.0,
                    life,
                    max: life,
                    size,
                },
                Mesh3d(assets.m("shard")),
                MeshMaterial3d(assets.mat(key)),
                Transform::from_xyz(d.pos.x, d.pos.y, 0.2)
                    .with_rotation(Quat::from_euler(
                        EulerRot::XYZ,
                        rand() * 6.0,
                        rand() * 6.0,
                        rand() * 6.0,
                    ))
                    .with_scale(Vec3::splat(size)),
            ));
        }
    }
}

fn fly_shards(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Shard, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut s, mut t) in &mut q {
        s.life -= dt;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        s.vel.y -= 26.0 * dt;
        t.translation += s.vel * dt;
        let spin = s.spin * dt;
        t.rotation = Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z) * t.rotation;
        // Shrink away over the last part of their life.
        t.scale = Vec3::splat(s.size * (s.life / s.max * 2.0).min(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_creature_mesh_is_well_formed() {
        for (name, m) in enemy_meshes() {
            m.validate()
                .unwrap_or_else(|e| panic!("mesh `{name}` is malformed: {e}"));
        }
    }

    #[test]
    fn the_creatures_fit_the_boxes_they_are_hit_in() {
        // Sizes are the (half width, half height) of each creature's hurtbox in
        // `assets/tuning/enemies.ron`; a model may overhang a little (spines,
        // claws, tendrils) but must not be visibly bigger than what you hit.
        let meshes: HashMap<_, _> = enemy_meshes().into_iter().collect();
        let width = |names: &[&str]| {
            names
                .iter()
                .map(|n| {
                    let (lo, hi) = meshes[n].bounds();
                    lo.x.abs().max(hi.x.abs()).max(lo.z.abs()).max(hi.z.abs())
                })
                .fold(0.0f32, f32::max)
        };
        // The dominant body part of each: shell, orb, barrel, pod.
        assert!(width(&["husk_shell"]) < 0.5 * 1.35);
        assert!(width(&["wisp_orb"]) < 0.45 * 1.2);
        assert!(width(&["shield_barrel"]) < 0.55 * 1.2);
        assert!(width(&["spitter_pod"]) < 0.5 * 1.2);
    }

    #[test]
    fn every_state_has_a_planned_length_where_it_should() {
        let t = Tuning::default().enemies;
        for kind in [
            EnemyKind::Husk,
            EnemyKind::Wisp,
            EnemyKind::Shieldbearer,
            EnemyKind::Spitter,
        ] {
            assert!(state_len(kind, EnemyState::Windup, &t) > 10.0);
            assert!(state_len(kind, EnemyState::Recover, &t) > 10.0);
            assert_eq!(state_len(kind, EnemyState::Chase, &t), 0.0);
        }
    }
}
