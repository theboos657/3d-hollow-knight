//! The two bosses, built from `rig::meshkit` geometry.
//!
//! * **The Gutter Matron**: a hunched brute of moss and shelf fungus with
//!   knuckle-dragging arms, a low tusked head, acid-green eyes and a glowing
//!   spore jar at her belt.
//! * **The Bellwarden**: a great bronze bell on stubby feet with a hooded
//!   mask floating above it, chain arms ending in bell-fists, a rune band
//!   and, from the second phase, glowing cracks.
//!
//! They share the enemies' rig and tell language (`models::enemies`): the
//! body is washed with the tell colour, the eyes / cracks / runes carry the
//! glow, and every telegraph, attack and recovery has its own silhouette
//! (`rig::creature::{matron,warden}_pose`).

use std::f32::consts::{PI, TAU as TAU_F};

use bevy::prelude::*;
use hk_sim::boss::{Boss, BossBrain, BossState};
use hk_sim::combat::{Hit, SimFrozen};
use hk_sim::components::{Aabb, SimPos, Velocity};
use hk_sim::enemy::EnemyState;
use hk_sim::tuning::{AttackKind, Tuning};
use hk_sim::{ms_to_ticks, SimTick};

use super::enemies::{
    apply_creature, CreatureAnim, CreatureRig, EnemyAssets, MatBuilder, MeshList,
};
use super::geo::thorn;
use crate::interp::Interpolated;
use crate::look::kits::bell;
use crate::rig::creature::{
    creature_pose, ease_guard, matron, species_glow, tell_glow, warden, BossAtk, CreatureIn,
    Species, MAX_JOINTS,
};
use crate::rig::meshkit::{cone, ellipsoid, hash3, lathe, limb, ring, tube, MeshData};
use crate::rig::{joint, part, Rest};

// ---------------------------------------------------------------- geometry --

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

fn profile_r(profile: &[(f32, f32)], y: f32) -> f32 {
    for w in profile.windows(2) {
        let ((r0, y0), (r1, y1)) = (w[0], w[1]);
        if y >= y0 && y <= y1 && (y1 - y0).abs() > 1e-6 {
            return r0 + (r1 - r0) * (y - y0) / (y1 - y0);
        }
    }
    profile.last().map_or(0.0, |p| p.0)
}

/// A jagged glowing crack running down the outside of a lathe surface.
fn surface_crack(
    profile: &[(f32, f32)],
    z_scale: f32,
    side: f32,
    a0: f32,
    wobble: &[f32],
    ys: &[f32],
    width: f32,
) -> MeshData {
    let pts: Vec<Vec3> = ys
        .iter()
        .zip(wobble)
        .map(|(&y, w)| {
            let r = profile_r(profile, y) * 1.02;
            let a = a0 + w;
            Vec3::new(r * a.cos(), y, side * r * z_scale * a.sin())
        })
        .collect();
    tube(&pts, |t| width * (1.0 - 0.5 * t), 5)
}

const MATRON_BODY: [(f32, f32); 7] = [
    (0.50, -0.75),
    (0.95, -0.50),
    (1.10, -0.10),
    (1.02, 0.40),
    (0.75, 0.80),
    (0.35, 0.98),
    (0.0, 1.02),
];

pub fn matron_meshes(out: &mut MeshList) {
    // Torso: a big mossy hump, lumped like a hillock.
    let body = lathe(&MATRON_BODY, 56)
        .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.85)))
        .sculpted(21, 0.075, 2.4, 4)
        .uv_scaled(4.0, 2.0)
        .recolor(|p| {
            let n = hash3(
                5,
                (p.x * 3.0) as i32,
                (p.y * 3.0) as i32,
                (p.z * 3.0) as i32,
            );
            let k = ((p.y + 0.75) / 1.8).clamp(0.0, 1.0);
            let v = 0.80 + 0.20 * n;
            [v * (0.70 + 0.30 * k), v, v * 0.85, 1.0]
        });
    out.push(("matron_body", body));

    // Moss and grass tufts sprouting over the back and shoulders.
    let mut tufts = MeshData::default();
    for k in 0..70 {
        let h = |s: i32| hash3(61, k, s, 1);
        // A point on the upper, rear half of the hump.
        let y = -0.35 + 1.30 * h(0);
        let r = profile_r(&MATRON_BODY, y);
        let a = PI * (0.55 + 0.9 * h(1)) + if h(2) > 0.7 { 1.5 } else { 0.0 };
        let base = Vec3::new(r * a.cos(), y, r * 0.85 * a.sin());
        let out_dir = Vec3::new(a.cos(), 0.6, 0.85 * a.sin()).normalize();
        tufts.merge(&thorn(
            base * 0.985,
            out_dir,
            Vec3::new(-0.3, -0.5, 0.0) * (0.3 + h(3)),
            0.16 + 0.20 * h(4),
            0.026,
            0.0,
            5,
        ));
    }
    out.push(("matron_tufts", tufts));

    // Shelf fungus down the back and over the shoulders, ridged like growth rings.
    let mut shelves = MeshData::default();
    for (x, y, z, r, tilt) in [
        (-0.78f32, 0.55f32, 0.35f32, 0.36f32, 0.35f32),
        (-0.88, 0.15, -0.30, 0.40, 0.25),
        (-0.55, 0.88, 0.05, 0.34, 0.15),
        (-0.98, -0.20, 0.20, 0.30, 0.4),
        (-0.30, 0.98, 0.32, 0.28, 0.1),
        (-0.42, 0.92, -0.34, 0.30, 0.12),
        (0.20, 0.96, -0.10, 0.26, 0.05),
        (-1.02, 0.36, -0.05, 0.26, 0.5),
        (-0.70, -0.32, -0.28, 0.24, 0.3),
    ] {
        let at_shelf = at(x, y, z) * Mat4::from_rotation_z(tilt);
        shelves.merge(
            &ellipsoid(r, 0.07, r * 0.9, 10, 20)
                .sculpted(37, 0.02, 6.0, 2)
                .transformed(at_shelf),
        );
        for k in 1..4 {
            let f = k as f32 / 4.0;
            shelves.merge(
                &ring(r * f, 0.011, 20, 5)
                    .transformed(at_shelf * at(0.0, 0.06 * (1.0 - f * f).sqrt() + 0.004, 0.0)),
            );
        }
    }
    out.push(("matron_shelves", shelves.uv_scaled(2.0, 2.0)));

    // Acid-green pustules along both flanks, of many sizes.
    let mut spots = MeshData::default();
    for (x, y, z) in [
        (-0.1f32, 0.55f32, 0.78f32),
        (0.35, 0.15, 0.85),
        (-0.4, -0.15, 0.85),
        (0.05, -0.4, 0.78),
        (0.55, 0.5, 0.62),
        (0.2, 0.75, 0.55),
        (-0.55, 0.3, 0.88),
        (0.62, -0.05, 0.78),
    ] {
        for side in [1.0f32, -1.0] {
            let sz = 0.05 + 0.04 * hash3(71, (x * 10.0) as i32, (y * 10.0) as i32, side as i32);
            spots.merge(&ellipsoid(sz, sz, sz * 0.75, 8, 12).transformed(at(x, y, z * side)));
        }
    }
    out.push(("matron_spots", spots));

    // Head: a low, heavy skull with a jaw, curved tusks and branching antlers.
    let mut head = ellipsoid(0.42, 0.32, 0.40, 16, 26)
        .sculpted(41, 0.02, 4.0, 3)
        .transformed(at(0.12, 0.0, 0.0));
    head.merge(
        &ellipsoid(0.36, 0.13, 0.34, 10, 18)
            .sculpted(43, 0.015, 5.0, 2)
            .transformed(at(0.20, -0.28, 0.0)),
    );
    head.merge(&tube(
        &[
            Vec3::new(0.40, 0.14, -0.30),
            Vec3::new(0.50, 0.19, 0.0),
            Vec3::new(0.40, 0.14, 0.30),
        ],
        |t| 0.06 * (1.0 - 0.4 * (2.0 * t - 1.0).abs()),
        8,
    ));
    for side in [-1.0f32, 1.0] {
        head.merge(&thorn(
            Vec3::new(0.46, -0.22, 0.20 * side),
            Vec3::new(0.65, -0.55, 0.05 * side),
            Vec3::new(0.10, 0.85, 0.0),
            0.48,
            0.075,
            0.05,
            9,
        ));
        let root = Vec3::new(0.0, 0.26, 0.20 * side);
        head.merge(&tube(
            &[
                root,
                Vec3::new(-0.10, 0.52, 0.28 * side),
                Vec3::new(-0.05, 0.78, 0.24 * side),
                Vec3::new(0.10, 0.92, 0.20 * side),
            ],
            |t| 0.055 * (1.0 - 0.7 * t),
            8,
        ));
        // Two branches off each antler.
        for (from, dir) in [
            (
                Vec3::new(-0.10, 0.52, 0.28 * side),
                Vec3::new(-0.6, 0.6, 0.5 * side),
            ),
            (
                Vec3::new(-0.05, 0.78, 0.24 * side),
                Vec3::new(0.5, 0.7, 0.4 * side),
            ),
        ] {
            head.merge(&thorn(
                from,
                dir,
                Vec3::new(0.0, 0.4, 0.0),
                0.26,
                0.030,
                0.0,
                6,
            ));
        }
    }
    out.push(("matron_head", head.uv_scaled(2.0, 1.0)));
    let mut eyes = MeshData::default();
    for side in [1.0f32, -1.0] {
        for (y, z, sz) in [
            (0.14f32, 0.16f32, 0.05f32),
            (0.02, 0.22, 0.04),
            (0.20, 0.06, 0.035),
        ] {
            eyes.merge(&ellipsoid(sz * 0.6, sz * 1.1, sz, 6, 10).transformed(at(
                0.50,
                y,
                z * side,
            )));
        }
    }
    out.push(("matron_eyes", eyes));

    // Arms: long and thick with a swell of muscle, ending in knuckled fists.
    let mut arm = tube(
        &[
            Vec3::ZERO,
            Vec3::new(0.10, -0.32, 0.0),
            Vec3::new(0.22, -0.65, 0.0),
            Vec3::new(0.36, -0.98, 0.0),
            Vec3::new(0.48, -1.25, 0.0),
        ],
        |t| 0.30 - 0.08 * t + 0.05 * (t * PI).sin(),
        14,
    )
    .sculpted(45, 0.03, 4.0, 3);
    arm.merge(&thorn(
        Vec3::new(0.22, -0.65, 0.0),
        Vec3::new(-0.8, 0.4, 0.0),
        Vec3::new(-0.1, 0.5, 0.0),
        0.30,
        0.07,
        0.0,
        7,
    ));
    out.push(("matron_arm", arm.uv_scaled(2.0, 3.0)));
    let mut fist = ellipsoid(0.36, 0.32, 0.34, 12, 20)
        .sculpted(47, 0.03, 5.0, 2)
        .transformed(at(0.52, -1.42, 0.0));
    for dz in [-0.19f32, -0.065, 0.065, 0.19] {
        fist.merge(&ellipsoid(0.085, 0.085, 0.085, 6, 10).transformed(at(0.80, -1.36, dz)));
        fist.merge(&thorn(
            Vec3::new(0.84, -1.36, dz),
            Vec3::new(1.0, -0.15, dz),
            Vec3::new(0.0, -0.5, 0.0),
            0.20,
            0.05,
            0.0,
            6,
        ));
    }
    out.push(("matron_fist", fist));

    let mut leg = limb(Vec3::ZERO, Vec3::new(0.05, -0.62, 0.0), 0.32, 0.26, 12);
    leg.merge(&ellipsoid(0.30, 0.16, 0.30, 8, 12).transformed(at(0.02, -0.02, 0.0)));
    out.push((
        "matron_leg",
        leg.sculpted(49, 0.025, 4.0, 3).uv_scaled(2.0, 2.0),
    ));
    let mut foot = ellipsoid(0.44, 0.14, 0.32, 8, 14).transformed(at(0.16, -0.66, 0.0));
    for dz in [-0.16f32, 0.0, 0.16] {
        foot.merge(&thorn(
            Vec3::new(0.52, -0.66, dz),
            Vec3::new(1.0, -0.1, dz),
            Vec3::new(0.0, -0.4, 0.0),
            0.18,
            0.05,
            0.0,
            6,
        ));
    }
    out.push(("matron_foot", foot.sculpted(51, 0.02, 5.0, 2)));

    // A spore jar hanging at the belt, with a stopper and a rope collar.
    out.push((
        "matron_jar",
        ellipsoid(0.16, 0.22, 0.16, 12, 18).transformed(at(0.0, -0.22, 0.0)),
    ));
    let mut cap = cone(0.10, 0.12, 10).transformed(at(0.0, -0.02, 0.0));
    cap.merge(&ring(0.085, 0.018, 14, 5).transformed(at(0.0, -0.03, 0.0)));
    out.push(("matron_jar_cap", cap));
}

const FRAC_PI_2_F: f32 = std::f32::consts::FRAC_PI_2;

const WARDEN_BELL: [(f32, f32); 9] = [
    (1.50, -1.30),
    (1.56, -1.20),
    (1.38, -1.08),
    (1.30, -0.80),
    (1.08, -0.20),
    (0.78, 0.40),
    (0.52, 0.80),
    (0.36, 1.00),
    (0.0, 1.06),
];

pub fn warden_meshes(out: &mut MeshList) {
    let squash = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.9));
    // The bell: cast bronze, its profile resampled smooth, with the small
    // irregularities of a casting and a polished lip.
    let profile: Vec<(f32, f32)> = (0..=48)
        .map(|k| {
            let y = -1.30 + 2.36 * k as f32 / 48.0;
            (profile_r(&WARDEN_BELL, y), y)
        })
        .collect();
    let bell_body = lathe(&profile, 72)
        .transformed(squash)
        .sculpted(31, 0.022, 2.6, 3)
        .uv_scaled(4.0, 2.0)
        .recolor(|p| {
            let k = ((p.y + 1.3) / 2.4).clamp(0.0, 1.0);
            let n = hash3(
                9,
                (p.x * 4.0) as i32,
                (p.y * 4.0) as i32,
                (p.z * 4.0) as i32,
            );
            let v = 0.88 + 0.12 * n;
            [
                v * (1.0 - 0.25 * k),
                v * (0.95 + 0.05 * k),
                v * (0.9 + 0.1 * k),
                1.0,
            ]
        });
    out.push(("warden_bell", bell_body));
    // Raised bands around the bell, studded with rivets, and a crown ring on top.
    let mut bands = MeshData::default();
    let mut studs = MeshData::default();
    for (y, w) in [(-1.16f32, 0.06f32), (-0.05, 0.05), (0.6, 0.045)] {
        let r = profile_r(&WARDEN_BELL, y);
        bands.merge(
            &ring(r + 0.01, w, 72, 8)
                .transformed(squash)
                .transformed(at(0.0, y, 0.0)),
        );
        let n = (r * 26.0) as i32;
        for i in 0..n {
            let a = TAU_F * i as f32 / n as f32;
            studs.merge(&ellipsoid(0.04, 0.04, 0.04, 5, 8).transformed(at(
                (r + 0.045) * a.cos(),
                y,
                (r + 0.045) * 0.9 * a.sin(),
            )));
        }
    }
    bands.merge(&ring(0.28, 0.07, 24, 8).transformed(at(0.0, 1.08, 0.0)));
    out.push(("warden_bands", bands.uv_scaled(6.0, 1.0)));
    out.push(("warden_studs", studs));
    // The rune band: a glowing ring with notches.
    let mut runes = ring(profile_r(&WARDEN_BELL, -0.55) + 0.02, 0.045, 60, 6)
        .transformed(squash)
        .transformed(at(0.0, -0.55, 0.0));
    for k in 0..12 {
        let a = k as f32 / 12.0 * 2.0 * PI;
        let r = profile_r(&WARDEN_BELL, -0.55);
        runes.merge(&ellipsoid(0.06, 0.16, 0.05, 6, 10).transformed(
            at(r * a.cos() * 1.03, -0.55, r * 0.9 * a.sin() * 1.03) * Mat4::from_rotation_y(-a),
        ));
    }
    out.push(("warden_runes", runes));
    // Cracks (glowing), on both faces.
    let mut cracks = MeshData::default();
    let ys = [0.75, 0.45, 0.10, -0.25, -0.60, -0.95];
    for side in [1.0f32, -1.0] {
        cracks.merge(&surface_crack(
            &WARDEN_BELL,
            0.9,
            side,
            1.10,
            &[0.0, 0.12, -0.10, 0.10, -0.08, 0.06],
            &ys,
            0.04,
        ));
        cracks.merge(&surface_crack(
            &WARDEN_BELL,
            0.9,
            side,
            1.85,
            &[0.05, -0.10, 0.12, -0.06, 0.10, -0.04],
            &ys,
            0.04,
        ));
        cracks.merge(&surface_crack(
            &WARDEN_BELL,
            0.9,
            side,
            2.55,
            &[-0.05, 0.08, -0.12, 0.08, -0.10, 0.05],
            &ys,
            0.04,
        ));
    }
    out.push(("warden_cracks", cracks));

    // The floating mask: a folded hood, a bone mask with a brow and cheekbones,
    // hollow eyes and ribbed horns.
    let hood = lathe(&[(0.46, -0.40), (0.44, 0.0), (0.32, 0.45), (0.0, 0.80)], 32)
        .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.85)))
        .sculpted(53, 0.035, 3.5, 3)
        .transformed(at(-0.20, 0.0, 0.0))
        .uv_scaled(4.0, 2.0);
    out.push(("warden_hood", hood));
    let mut face = ellipsoid(0.34, 0.46, 0.27, 16, 26)
        .sculpted(55, 0.012, 6.0, 2)
        .transformed(at(0.30, 0.0, 0.0));
    face.merge(&tube(
        &[
            Vec3::new(0.52, 0.20, -0.20),
            Vec3::new(0.62, 0.23, 0.0),
            Vec3::new(0.52, 0.20, 0.20),
        ],
        |t| 0.035 * (1.0 - 0.4 * (2.0 * t - 1.0).abs()),
        7,
    ));
    face.merge(&ellipsoid(0.06, 0.20, 0.05, 6, 10).transformed(at(0.62, -0.04, 0.0)));
    for side in [-1.0f32, 1.0] {
        face.merge(&ellipsoid(0.07, 0.05, 0.07, 6, 10).transformed(at(0.56, -0.10, 0.19 * side)));
        face.merge(&thorn(
            Vec3::new(0.10, 0.34, 0.14 * side),
            Vec3::new(-0.15, 1.0, 0.35 * side),
            Vec3::new(-0.9, -0.2, 0.0),
            0.98,
            0.055,
            0.10,
            9,
        ));
    }
    out.push(("warden_face", face.uv_scaled(2.0, 1.0)));
    let mut sockets = ellipsoid(0.05, 0.06, 0.085, 6, 10).transformed(at(0.575, 0.10, 0.12));
    sockets.merge(&ellipsoid(0.05, 0.06, 0.085, 6, 10).transformed(at(0.575, 0.10, -0.12)));
    out.push(("warden_sockets", sockets));
    let mut eyes = ellipsoid(0.03, 0.03, 0.06, 6, 10).transformed(at(0.61, 0.10, 0.12));
    eyes.merge(&ellipsoid(0.03, 0.03, 0.06, 6, 10).transformed(at(0.61, 0.10, -0.12)));
    out.push(("warden_eyes", eyes));

    // Chain arms: real linked chain, ending in bell-fists.
    let mut chain_arm = MeshData::default();
    let (a0, a1) = (Vec3::ZERO, Vec3::new(0.28, -0.9, 0.0));
    let links = 7;
    for k in 0..links {
        let f = (k as f32 + 0.5) / links as f32;
        let p = a0.lerp(a1, f);
        let dir = a1 - a0;
        // Alternate links turn a quarter so they interlock.
        let turn = if k % 2 == 0 { 0.0 } else { FRAC_PI_2_F };
        chain_arm.merge(
            &ring(0.075, 0.026, 12, 6)
                .transformed(Mat4::from_rotation_z(FRAC_PI_2_F))
                .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0)))
                .transformed(Mat4::from_rotation_x(turn))
                .transformed(Mat4::from_rotation_z(-(dir.x).atan2(-dir.y)) * Mat4::IDENTITY)
                .transformed(at(p.x, p.y, p.z)),
        );
    }
    out.push(("warden_chain", chain_arm));
    out.push(("warden_fist", bell(0.28, -0.9, 0.44, 0.0)));

    let mut leg = limb(Vec3::ZERO, Vec3::new(0.04, -0.32, 0.0), 0.15, 0.11, 10);
    leg.merge(&ring(0.14, 0.03, 14, 5).transformed(at(0.0, -0.05, 0.0)));
    out.push(("warden_leg", leg.sculpted(57, 0.01, 6.0, 2)));
    out.push((
        "warden_foot",
        ellipsoid(0.26, 0.08, 0.2, 8, 14)
            .transformed(at(0.08, -0.34, 0.0))
            .sculpted(59, 0.01, 6.0, 2),
    ));
}

pub fn boss_materials(m: &mut MatBuilder<'_>) {
    use crate::look::pbr::Kind;
    let c = Color::srgb;
    m.glow("matron_glow", Species::Matron);
    // The Matron: a hide of dense moss, soft fungus, leathery limbs, old bone.
    m.skin("matron_body", Kind::Moss, c(0.58, 0.68, 0.48), 1.0, 0.0);
    m.skin("matron_tuft", Kind::Moss, c(0.75, 0.9, 0.55), 1.0, 0.0);
    m.skin("matron_shelf", Kind::Flesh, c(1.05, 0.95, 0.70), 0.7, 0.35);
    m.skin("matron_flesh", Kind::Moss, c(0.46, 0.54, 0.36), 1.0, 0.0);
    m.skin("matron_bone", Kind::Bone, c(1.0, 0.96, 0.82), 0.9, 0.15);
    m.glow("warden_glow", Species::Bellwarden);
    // The Bellwarden: cast bronze and black iron, a cloth hood, a bone mask.
    m.skin("warden_bell", Kind::Bronze, c(1.0, 0.86, 0.66), 0.8, 0.5);
    m.skin("warden_iron", Kind::Iron, c(0.75, 0.72, 0.70), 1.0, 0.0);
    m.skin_with(
        "warden_cloth",
        Kind::Cloth,
        c(0.16, 0.14, 0.22),
        1.0,
        0.0,
        |mat| {
            mat.cull_mode = None;
            mat.double_sided = true;
        },
    );
    m.skin("warden_mask", Kind::Bone, c(1.0, 0.97, 0.90), 0.8, 0.3);
    m.skin("warden_fist", Kind::Bronze, c(1.0, 0.90, 0.70), 0.7, 0.4);
    m.lit("warden_dark", c(0.02, 0.02, 0.025), 0.8, 0.0);
}

// --------------------------------------------------------------------- rigs --

type Mat = Handle<StandardMaterial>;

fn t(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

pub fn body_glow_keys(species: Species) -> (&'static str, &'static str) {
    match species {
        Species::Matron => ("matron_body", "matron_glow"),
        _ => ("warden_bell", "warden_glow"),
    }
}

pub fn build_matron(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use matron::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let id = Transform::IDENTITY;
    let body = joint(c, lean, t(0.0, 1.0, 0.0));
    j[BODY] = body;
    part(c, body, a.m("matron_body"), body_m.clone(), id);
    part(c, body, a.m("matron_tufts"), a.mat("matron_tuft"), id);
    part(c, body, a.m("matron_shelves"), a.mat("matron_shelf"), id);
    part(c, body, a.m("matron_spots"), glow_m.clone(), id);

    let head = joint(c, body, t(0.92, 0.12, 0.0));
    j[HEAD] = head;
    part(c, head, a.m("matron_head"), a.mat("matron_flesh"), id);
    part(c, head, a.m("matron_eyes"), glow_m.clone(), id);

    for (idx, z) in [(ARM_FRONT, 0.80), (ARM_BACK, -0.80)] {
        let arm = joint(c, body, t(0.35, 0.55, z));
        j[idx] = arm;
        part(c, arm, a.m("matron_arm"), a.mat("matron_flesh"), id);
        part(c, arm, a.m("matron_fist"), a.mat("matron_bone"), id);
    }
    for (idx, z) in [(LEG_FRONT, 0.42), (LEG_BACK, -0.42)] {
        let leg = joint(c, lean, t(0.3, 0.70, z));
        j[idx] = leg;
        part(c, leg, a.m("matron_leg"), a.mat("matron_flesh"), id);
        part(c, leg, a.m("matron_foot"), a.mat("matron_bone"), id);
    }
    let jar = joint(c, body, t(0.78, -0.30, 0.52));
    j[JAR] = jar;
    part(c, jar, a.m("matron_jar"), glow_m.clone(), id);
    part(c, jar, a.m("matron_jar_cap"), a.mat("matron_bone"), id);
    j
}

pub fn build_warden(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use warden::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let id = Transform::IDENTITY;
    let body = joint(c, lean, t(0.0, 1.45, 0.0));
    j[BODY] = body;
    part(c, body, a.m("warden_bell"), body_m.clone(), id);
    part(c, body, a.m("warden_bands"), a.mat("warden_iron"), id);
    part(c, body, a.m("warden_studs"), a.mat("warden_fist"), id);
    part(c, body, a.m("warden_runes"), glow_m.clone(), id);
    let cracks = joint(c, body, Transform::IDENTITY);
    j[CRACKS] = cracks;
    part(c, cracks, a.m("warden_cracks"), glow_m.clone(), id);

    let mask = joint(c, body, t(0.15, 1.75, 0.0));
    j[MASK] = mask;
    part(c, mask, a.m("warden_hood"), a.mat("warden_cloth"), id);
    part(c, mask, a.m("warden_face"), a.mat("warden_mask"), id);
    part(c, mask, a.m("warden_sockets"), a.mat("warden_dark"), id);
    part(c, mask, a.m("warden_eyes"), glow_m.clone(), id);

    for (idx, z) in [(ARM_FRONT, 0.95), (ARM_BACK, -0.95)] {
        let arm = joint(c, body, t(0.85, 0.5, z));
        j[idx] = arm;
        part(c, arm, a.m("warden_chain"), a.mat("warden_iron"), id);
        part(c, arm, a.m("warden_fist"), a.mat("warden_fist"), id);
    }
    for (idx, z) in [(LEG_FRONT, 0.45), (LEG_BACK, -0.45)] {
        let leg = joint(
            c,
            lean,
            t(if idx == LEG_FRONT { 0.5 } else { -0.5 }, 0.36, z),
        );
        j[idx] = leg;
        part(c, leg, a.m("warden_leg"), a.mat("warden_iron"), id);
        part(c, leg, a.m("warden_foot"), a.mat("warden_iron"), id);
    }
    j
}

// ----------------------------------------------------------------- systems --

fn species_of(id: &str) -> Species {
    match id {
        "matron" => Species::Matron,
        _ => Species::Bellwarden,
    }
}

/// Gives every new boss its model.
#[allow(clippy::type_complexity)]
pub fn attach_boss_models(
    mut commands: Commands,
    assets: Option<Res<EnemyAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &Boss, &BossBrain, &Aabb, &SimPos), Added<Boss>>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (e, boss, brain, aabb, pos) in &q {
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Visibility::default(),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
        let rig = super::enemies::spawn_creature(
            &mut commands,
            &mut mats,
            &assets,
            e,
            species_of(&boss.id),
            aabb.half.y,
        );
        commands
            .entity(e)
            .insert((rig, CreatureAnim::new(brain.facing, 1.0)));
    }
}

fn atk_of(kind: &AttackKind) -> BossAtk {
    match kind {
        AttackKind::Slam { .. } => BossAtk::Slam,
        AttackKind::Charge { .. } => BossAtk::Charge,
        AttackKind::Bells { .. } => BossAtk::Bells,
        AttackKind::Sweep { .. } => BossAtk::Sweep,
        AttackKind::Pendulums { .. } => BossAtk::Pendulums,
        AttackKind::Toll { .. } => BossAtk::Toll,
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_bosses(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    frozen: Res<SimFrozen>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut hits: MessageReader<Hit>,
    mut bosses: Query<(
        Entity,
        &Boss,
        &BossBrain,
        &Velocity,
        &CreatureRig,
        &mut CreatureAnim,
    )>,
    mut transforms: Query<(&mut Transform, Option<&Rest>), Without<CreatureRig>>,
) {
    let dt = time.delta_secs().min(0.05);
    let live = !frozen.0;
    let alpha = if live { fixed.overstep_fraction() } else { 1.0 };
    let hit_events: Vec<Hit> = hits.read().copied().collect();
    for (e, boss, b, vel, rig, mut anim) in &mut bosses {
        if live {
            anim.clock += dt;
        }
        for _ in hit_events.iter().filter(|h| h.victim == e) {
            anim.hit = 1.0;
        }
        anim.hit = (anim.hit - dt * 4.5).max(0.0);

        let def = tuning.bosses.get(&boss.id);
        let attack = b.attack.and_then(|k| def.and_then(|d| d.attacks.get(k)));
        let atk = attack.map_or(BossAtk::None, |a| atk_of(&a.kind));
        let face = b.facing;
        let fwd = face as f32;
        let vx = vel.x * fwd;
        if live && vx.abs() > 0.3 {
            anim.walk += (4.5 + vx.abs() * 1.4).min(30.0) * dt;
        }
        // The planned length of the current state.
        let len = match b.state {
            BossState::Telegraph => attack.map_or(0, |a| ms_to_ticks(a.telegraph_ms)),
            BossState::Active => attack.map_or(0, |a| ms_to_ticks(a.active_ms)),
            BossState::Recover => b.recover_ticks,
            BossState::Intro => def.map_or(0, |d| ms_to_ticks(d.intro_ms)),
            BossState::Transition => def.map_or(0, |d| ms_to_ticks(d.transition_ms)),
            BossState::Dying => def.map_or(1, |d| ms_to_ticks(d.death_ms)),
            _ => 0,
        } as f32;
        let t_ticks = (b.timer as f32 - 1.0 + alpha).max(0.0);
        // The boss states, in the enemies' vocabulary.
        let state = match b.state {
            BossState::Sleeping | BossState::Dying => EnemyState::Idle,
            BossState::Intro | BossState::Transition => EnemyState::Notice,
            BossState::Choose | BossState::Approach => EnemyState::Chase,
            BossState::Telegraph => EnemyState::Windup,
            BossState::Active => EnemyState::Attack,
            BossState::Recover => EnemyState::Recover,
        };
        let input = CreatureIn {
            species: rig.species,
            state,
            t: t_ticks,
            len,
            clock: anim.clock,
            walk: anim.walk,
            vx,
            aim: 0.0,
            guard: 1.0,
            hit: anim.hit,
            sway: 0.0,
            atk,
            phase: b.phase,
            second: b.second_tell,
            airborne: !b.was_grounded,
            wall: b.hit_wall,
            sleeping: b.state == BossState::Sleeping,
            dying: if b.state == BossState::Dying {
                (t_ticks / len.max(1.0)).clamp(0.0, 1.0)
            } else {
                0.0
            },
        };
        let pose = creature_pose(&input);
        let base = species_glow(rig.species);
        let phase_k = 0.9 + 0.35 * (b.phase.saturating_sub(1)) as f32;
        let glow = match b.state {
            BossState::Sleeping => [base[0] * 0.15, base[1] * 0.15, base[2] * 0.15],
            BossState::Intro | BossState::Transition => {
                let p = 0.5 + 0.5 * (tick.0 as f32 * 0.09).sin();
                [2.0 * p + 0.3, 1.6 * p + 0.3, 0.6 * p]
            }
            BossState::Choose | BossState::Approach => {
                [base[0] * phase_k, base[1] * phase_k, base[2] * phase_k]
            }
            BossState::Telegraph => tell_glow(rig.species, EnemyState::Windup, tick.0),
            BossState::Active => tell_glow(rig.species, EnemyState::Attack, tick.0),
            BossState::Recover => tell_glow(rig.species, EnemyState::Recover, tick.0),
            BossState::Dying => {
                let k = input.dying;
                [1.0 + 3.0 * k; 3]
            }
        };
        // Keep the eased guard helper linked (the boss has no shield).
        anim.guard = ease_guard(anim.guard, 1.0, dt);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> MeshList {
        let mut v = Vec::new();
        matron_meshes(&mut v);
        warden_meshes(&mut v);
        v
    }

    #[test]
    fn every_boss_mesh_is_well_formed() {
        for (name, m) in all() {
            m.validate()
                .unwrap_or_else(|e| panic!("mesh `{name}` is malformed: {e}"));
        }
    }

    #[test]
    fn the_bosses_fit_the_boxes_they_are_hit_in() {
        let meshes: std::collections::HashMap<_, _> = all().into_iter().collect();
        // Matron: half (1.1, 1.0); Bellwarden: half (1.5, 1.4).
        let (lo, hi) = meshes["matron_body"].bounds();
        assert!(
            hi.x <= 1.1 * 1.15 && lo.x >= -1.1 * 1.15,
            "matron torso width"
        );
        // Body joint sits at y = 1.0: the torso spans 0.25..2.05 in the box's 0..2.
        assert!(1.0 + hi.y <= 2.0 * 1.1 && 1.0 + lo.y >= 0.1);
        let (lo, hi) = meshes["warden_bell"].bounds();
        assert!(hi.x <= 1.5 * 1.15 && lo.x >= -1.5 * 1.15, "bell width");
        assert!(
            1.45 + lo.y >= 0.05 && 1.45 + hi.y <= 2.8 * 1.05,
            "bell height"
        );
    }

    #[test]
    fn the_bosses_have_their_own_glow_colours() {
        assert_ne!(
            species_glow(Species::Matron),
            species_glow(Species::Bellwarden)
        );
    }
}
