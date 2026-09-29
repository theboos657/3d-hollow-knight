//! The geometry of the ordinary creatures and the training dummy.
//!
//! Every shape is built from `rig::meshkit` primitives, then *sculpted* (noise
//! displacement with matching normals) so no surface is a clean maths solid,
//! and given texture coordinates scaled for its material's map. The joint
//! layout (what moves with what) is set in `models::enemies`; this module only
//! decides what each piece looks like.
//!
//! * **Husk**: a charred beetle-brute. A carapace of six overlapping plates
//!   with a ridge of curved thorns, a bone skull with hollow sockets and
//!   pincers, jointed claw arms and insect legs.
//! * **Wisp**: a glass orb with a burning nucleus, veins inside it, curved
//!   horns and five long beaded tendrils.
//! * **Shieldbearer**: riveted plate armour on a barrel body, a belt and
//!   pauldrons, a great helm with a plume, greaves, and a tower shield with a
//!   bronze rim and a bell sigil; a barred furnace vent is the weak spot.
//! * **Spitter**: a wet, pustuled pod with a ringed stalk that ends in a
//!   five-lobed, toothed maw.
//! * **Dummy**: a lashed wooden post with a burlap sack head, straw and a
//!   painted target.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use crate::rig::meshkit::{
    cone, ellipsoid, extrude, hash3, lathe, limb, ribbon, ring, tube, MeshData,
};

/// A list of named meshes (what the geometry builders return).
pub type MeshList = Vec<(&'static str, MeshData)>;

pub fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

/// Points a `+Y`-up primitive (a cone) along `dir`, based at `p`.
pub fn aim(p: Vec3, dir: Vec3) -> Mat4 {
    Mat4::from_translation(p) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, dir.normalize()))
}

/// Radius of a lathe profile at height `y` (linear between profile points).
pub fn profile_r(profile: &[(f32, f32)], y: f32) -> f32 {
    for w in profile.windows(2) {
        let ((r0, y0), (r1, y1)) = (w[0], w[1]);
        if y >= y0 && y <= y1 && (y1 - y0).abs() > 1e-6 {
            return r0 + (r1 - r0) * (y - y0) / (y1 - y0);
        }
    }
    profile.last().map_or(0.0, |p| p.0)
}

/// A tapered, curved spike: from `base` along `dir`, bending toward `bend`
/// (a thorn, a horn, a claw). `ribs` adds growth rings along it.
pub fn thorn(
    base: Vec3,
    dir: Vec3,
    bend: Vec3,
    len: f32,
    r0: f32,
    ribs: f32,
    sides: usize,
) -> MeshData {
    let dir = dir.normalize();
    let pts: Vec<Vec3> = (0..=6)
        .map(|i| {
            let t = i as f32 / 6.0;
            base + dir * len * t + bend * len * t * t
        })
        .collect();
    tube(
        &pts,
        move |t| r0 * (1.0 - t).powf(0.85) * (1.0 + ribs * (t * 22.0).sin()),
        sides,
    )
}

// ------------------------------------------------------------------- husk --

const HUSK_SHELL: [(f32, f32); 7] = [
    (0.30, -0.30),
    (0.46, -0.24),
    (0.52, -0.05),
    (0.49, 0.16),
    (0.38, 0.32),
    (0.20, 0.42),
    (0.0, 0.46),
];

/// The carapace's profile: six overlapping plates up the dome, each proud at
/// its lower lip and tucked in toward the next.
fn husk_plates() -> Vec<(f32, f32)> {
    let (y0, y1, bands) = (-0.30f32, 0.46f32, 6);
    let mut pts = Vec::new();
    for b in 0..bands {
        let ya = y0 + (y1 - y0) * b as f32 / bands as f32;
        let yb = y0 + (y1 - y0) * (b + 1) as f32 / bands as f32;
        for k in 0..=4 {
            let f = k as f32 / 4.0;
            let y = ya + (yb - ya) * f;
            let lip = 0.06 * (1.0 - f).powi(2);
            pts.push((profile_r(&HUSK_SHELL, y) * (0.965 + lip), y));
        }
    }
    // The last point is the pole.
    if let Some(last) = pts.last_mut() {
        last.0 = 0.0;
    }
    pts
}

/// A crack along the shell, on one of its two flanks (`side` = +1 or -1).
fn husk_crack(side: f32, a0: f32, wobble: [f32; 5]) -> MeshData {
    let ys = [0.38, 0.24, 0.10, -0.04, -0.18];
    let pts: Vec<Vec3> = ys
        .iter()
        .zip(wobble)
        .map(|(&y, w)| {
            let r = profile_r(&HUSK_SHELL, y) * 1.045;
            let a = a0 + w;
            Vec3::new(r * a.cos(), y, side * r * 0.92 * a.sin())
        })
        .collect();
    tube(&pts, |t| 0.017 * (1.0 - 0.55 * t), 5)
}

fn husk_meshes(out: &mut MeshList) {
    // Shell: overlapping plates, lumped and pitted, darker toward the ground.
    let shell = lathe(&husk_plates(), 44)
        .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.92)))
        .sculpted(3, 0.018, 6.0, 3)
        .uv_scaled(2.0, 1.0)
        .recolor(|p| {
            let k = ((p.y + 0.30) / 0.76).clamp(0.0, 1.0);
            let s = 0.55 + 0.45 * k;
            [s, s * 0.95, s * 0.9, 1.0]
        });
    out.push(("husk_shell", shell));

    // Soft underbelly, so the hull is never hollow from below.
    out.push((
        "husk_belly",
        ellipsoid(0.34, 0.12, 0.30, 12, 20)
            .transformed(at(0.04, -0.24, 0.0))
            .sculpted(11, 0.02, 7.0, 2),
    ));

    // A ridge of curved thorns down the back, big at the neck, small at the tail.
    let mut spines = MeshData::default();
    for (x, y, h, tilt) in [
        (-0.10f32, 0.44f32, 0.30f32, 0.20f32),
        (-0.22, 0.39, 0.27, 0.45),
        (-0.33, 0.30, 0.24, 0.72),
        (-0.42, 0.18, 0.21, 1.0),
        (-0.48, 0.04, 0.17, 1.28),
        (-0.49, -0.11, 0.12, 1.5),
    ] {
        let dir = Vec3::new(-tilt.sin(), tilt.cos(), 0.0);
        let bend = Vec3::new(-0.35, -0.10, 0.0);
        spines.merge(&thorn(Vec3::new(x, y, 0.0), dir, bend, h, 0.062, 0.0, 7));
        // A collar of flesh at the root of each.
        spines.merge(&ellipsoid(0.075, 0.04, 0.075, 5, 10).transformed(at(x, y - 0.005, 0.0)));
    }
    // Side thorns along the shoulder line, each flank.
    for side in [-1.0f32, 1.0] {
        for (x, y, h) in [
            (-0.05f32, 0.28f32, 0.14f32),
            (-0.24, 0.20, 0.13),
            (-0.38, 0.06, 0.11),
        ] {
            let r = profile_r(&HUSK_SHELL, y) * 0.86;
            let out_dir = Vec3::new(-0.35, 0.35, side * 1.0);
            spines.merge(&thorn(
                Vec3::new(x, y, side * r),
                out_dir,
                Vec3::new(-0.2, -0.1, 0.0),
                h,
                0.035,
                0.0,
                6,
            ));
        }
    }
    out.push(("husk_spines", spines.sculpted(4, 0.006, 14.0, 2)));

    // Glowing cracks on both flanks, and a few short branches off them.
    let mut cracks = MeshData::default();
    for side in [1.0, -1.0] {
        cracks.merge(&husk_crack(side, 0.95, [0.0, 0.10, -0.08, 0.12, -0.05]));
        cracks.merge(&husk_crack(side, 1.55, [0.05, -0.12, 0.10, -0.06, 0.08]));
        cracks.merge(&husk_crack(side, 2.15, [-0.05, 0.08, -0.10, 0.10, -0.04]));
        for (y, a, len) in [
            (0.20f32, 1.20f32, 0.14f32),
            (0.02, 1.85, 0.16),
            (-0.10, 1.05, 0.12),
        ] {
            let r = profile_r(&HUSK_SHELL, y) * 1.045;
            let p = Vec3::new(r * a.cos(), y, side * r * 0.92 * a.sin());
            let q = p + Vec3::new(0.05 * (a * 5.0).sin(), -len, side * 0.03);
            let m = p.lerp(q, 0.5) + Vec3::new(0.03, 0.0, side * 0.02);
            cracks.merge(&tube(&[p, m, q], |t| 0.011 * (1.0 - 0.8 * t), 4));
        }
    }
    out.push(("husk_cracks", cracks));

    // Head: a heavy skull with a brow ridge, cheekbones, a muzzle, curved
    // horns, sockets ringed in bone, and pincers.
    let mut skull = ellipsoid(0.22, 0.16, 0.17, 16, 28)
        .transformed(at(0.09, 0.0, 0.0))
        .sculpted(5, 0.012, 9.0, 2);
    skull.merge(&ellipsoid(0.11, 0.075, 0.105, 8, 14).transformed(at(0.25, -0.055, 0.0)));
    skull.merge(&tube(
        &[
            Vec3::new(0.20, 0.085, -0.135),
            Vec3::new(0.255, 0.105, 0.0),
            Vec3::new(0.20, 0.085, 0.135),
        ],
        |t| 0.032 * (1.0 - 0.5 * (2.0 * t - 1.0).abs()),
        7,
    ));
    for side in [-1.0f32, 1.0] {
        skull.merge(&ellipsoid(0.055, 0.04, 0.045, 6, 10).transformed(at(
            0.17,
            -0.065,
            0.135 * side,
        )));
        // A raised orbital rim around each socket.
        skull.merge(
            &ring(0.056, 0.014, 18, 6)
                .transformed(Mat4::from_rotation_z(-FRAC_PI_2))
                .transformed(at(0.262, 0.035, 0.088 * side)),
        );
        skull.merge(&thorn(
            Vec3::new(0.04, 0.11, 0.10 * side),
            Vec3::new(-0.35, 1.0, 0.3 * side),
            Vec3::new(-0.6, -0.2, 0.15 * side),
            0.30,
            0.040,
            0.10,
            8,
        ));
    }
    out.push(("husk_skull", skull.uv_scaled(2.0, 1.0)));
    let mut jaw = ellipsoid(0.15, 0.05, 0.13, 8, 14).transformed(at(0.15, -0.135, 0.0));
    for side in [-1.0f32, 1.0] {
        jaw.merge(&thorn(
            Vec3::new(0.22, -0.115, 0.08 * side),
            Vec3::new(0.7, -0.5, -0.25 * side),
            Vec3::new(0.15, 0.55, -0.35 * side),
            0.22,
            0.032,
            0.0,
            6,
        ));
    }
    out.push(("husk_jaw", jaw));
    // Deep sockets, and in each a narrow burning slit.
    let mut sockets = ellipsoid(0.05, 0.065, 0.055, 8, 12).transformed(at(0.215, 0.035, 0.088));
    sockets.merge(&ellipsoid(0.05, 0.065, 0.055, 8, 12).transformed(at(0.215, 0.035, -0.088)));
    out.push(("husk_sockets", sockets));
    let mut eyes = ellipsoid(0.014, 0.05, 0.02, 6, 10).transformed(at(0.262, 0.035, 0.088));
    eyes.merge(&ellipsoid(0.014, 0.05, 0.02, 6, 10).transformed(at(0.262, 0.035, -0.088)));
    out.push(("husk_eyes", eyes));

    // Arms: shoulder, elbow spike, forearm, and three hooked claws.
    let mut arm = tube(
        &[
            Vec3::ZERO,
            Vec3::new(0.05, -0.13, 0.0),
            Vec3::new(0.08, -0.24, 0.0),
            Vec3::new(0.16, -0.34, 0.0),
            Vec3::new(0.26, -0.43, 0.0),
        ],
        |t| 0.070 - 0.034 * t + 0.014 * (t * PI).sin(),
        10,
    )
    .sculpted(6, 0.01, 10.0, 2);
    arm.merge(&ellipsoid(0.06, 0.06, 0.06, 6, 10).transformed(at(0.08, -0.24, 0.0)));
    arm.merge(&thorn(
        Vec3::new(0.08, -0.24, 0.0),
        Vec3::new(-0.8, 0.5, 0.0),
        Vec3::new(-0.2, 0.5, 0.0),
        0.16,
        0.032,
        0.0,
        6,
    ));
    out.push(("husk_arm", arm.uv_scaled(1.0, 2.0)));
    let mut claws = MeshData::default();
    for dz in [-0.055f32, 0.0, 0.055] {
        claws.merge(&thorn(
            Vec3::new(0.26, -0.43, dz),
            Vec3::new(0.30 + dz, -1.0, dz * 3.0),
            Vec3::new(0.6, -0.1, 0.0),
            0.26,
            0.030,
            0.0,
            6,
        ));
    }
    out.push(("husk_claws", claws));

    // Legs: thigh, knee, shin, and a foot with two toe claws.
    let mut leg = limb(Vec3::ZERO, Vec3::new(0.07, -0.13, 0.0), 0.082, 0.062, 9);
    leg.merge(&ellipsoid(0.065, 0.065, 0.065, 6, 10).transformed(at(0.07, -0.13, 0.0)));
    leg.merge(&limb(
        Vec3::new(0.07, -0.13, 0.0),
        Vec3::new(0.0, -0.265, 0.0),
        0.056,
        0.040,
        8,
    ));
    out.push((
        "husk_leg",
        leg.sculpted(8, 0.008, 12.0, 2).uv_scaled(1.0, 2.0),
    ));
    let mut foot = ellipsoid(0.10, 0.045, 0.075, 6, 12).transformed(at(0.03, -0.285, 0.0));
    for dz in [-0.045f32, 0.045] {
        foot.merge(&thorn(
            Vec3::new(0.10, -0.285, dz),
            Vec3::new(1.0, -0.15, dz * 2.0),
            Vec3::new(0.0, -0.4, 0.0),
            0.13,
            0.022,
            0.0,
            5,
        ));
    }
    out.push(("husk_foot", foot));
}

// ------------------------------------------------------------------- wisp --

fn wisp_meshes(out: &mut MeshList) {
    out.push((
        "wisp_orb",
        ellipsoid(0.38, 0.38, 0.38, 26, 44).sculpted(21, 0.008, 5.0, 2),
    ));
    // The burning nucleus, with a ring around it.
    out.push(("wisp_core", ellipsoid(0.16, 0.16, 0.16, 12, 20)));
    // Veins curling from the nucleus toward the glass, seen through it.
    let mut veins = ring(0.105, 0.010, 30, 6)
        .transformed(Mat4::from_rotation_x(0.9) * Mat4::from_rotation_z(0.4));
    for k in 0..5 {
        let phi = k as f32 / 5.0 * TAU + 0.5;
        let out_dir = Vec3::new(phi.cos(), 0.25 * (k as f32 - 2.0), phi.sin()).normalize();
        let side = out_dir.cross(Vec3::Y).normalize_or_zero();
        let pts: Vec<Vec3> = (0..=6)
            .map(|i| {
                let t = i as f32 / 6.0;
                out_dir * (0.14 + 0.20 * t) + side * 0.06 * (t * 9.0 + k as f32).sin() * t
            })
            .collect();
        veins.merge(&tube(&pts, |t| 0.011 * (1.0 - 0.7 * t), 5));
    }
    out.push(("wisp_veins", veins));
    // A tilted halo around the orb.
    out.push((
        "wisp_halo",
        ring(0.50, 0.014, 40, 8)
            .transformed(Mat4::from_rotation_x(1.25) * Mat4::from_rotation_z(0.25)),
    ));
    // A crown of curved horns.
    let mut crown = MeshData::default();
    for k in 0..5 {
        let phi = k as f32 / 5.0 * TAU + 0.3;
        let o = Vec3::new(phi.cos(), 0.0, phi.sin());
        crown.merge(&thorn(
            Vec3::new(o.x * 0.19, 0.30, o.z * 0.19),
            o * 0.45 + Vec3::Y,
            o * 0.35,
            0.24,
            0.045,
            0.12,
            8,
        ));
    }
    out.push(("wisp_crown", crown));
    // Long tendrils, beaded and swaying, thinning to a thread.
    let path: Vec<Vec3> = (0..=9)
        .map(|i| {
            let t = i as f32 / 9.0;
            Vec3::new(0.035 * (t * 7.0).sin(), -0.58 * t, 0.025 * (t * 5.0).cos())
        })
        .collect();
    out.push((
        "wisp_tendril",
        tube(
            &path,
            |t| 0.048 * (1.0 - 0.9 * t) * (1.0 + 0.32 * (t * 24.0).sin()),
            8,
        )
        .uv_scaled(1.0, 3.0),
    ));
}

// ------------------------------------------------------------ shieldbearer --

const BARREL: [(f32, f32); 6] = [
    (0.30, -0.42),
    (0.46, -0.32),
    (0.52, -0.05),
    (0.50, 0.22),
    (0.40, 0.40),
    (0.30, 0.46),
];

fn shield_meshes(out: &mut MeshList) {
    let squash_z = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.9));
    // The cuirass: a barrel of plate, with a swell between each band and a
    // hammered, dented surface.
    let mut profile = Vec::new();
    for k in 0..=26 {
        let y = -0.42 + 0.88 * k as f32 / 26.0;
        let swell = 0.014 * (y * 36.0).sin();
        profile.push((profile_r(&BARREL, y) + swell, y));
    }
    out.push((
        "shield_barrel",
        lathe(&profile, 40)
            .transformed(squash_z)
            .sculpted(5, 0.010, 7.0, 3)
            .uv_scaled(3.0, 1.5)
            .recolor(|p| {
                let k = ((p.y + 0.42) / 0.9).clamp(0.0, 1.0);
                let s = 0.6 + 0.4 * k;
                [s, s, s, 1.0]
            }),
    ));
    let mut bands = MeshData::default();
    let mut rivets = MeshData::default();
    for y in [-0.28, 0.02, 0.30] {
        let r = profile_r(&BARREL, y);
        bands.merge(
            &ring(r + 0.005, 0.034, 40, 8)
                .transformed(squash_z)
                .transformed(at(0.0, y, 0.0)),
        );
        // Rivet heads all the way round the band.
        for i in 0..18 {
            let a = TAU * i as f32 / 18.0 + 0.1 * y;
            let p = Vec3::new((r + 0.038) * a.cos(), y, (r + 0.038) * 0.9 * a.sin());
            rivets.merge(&ellipsoid(0.017, 0.017, 0.017, 4, 8).transformed(at(p.x, p.y, p.z)));
        }
    }
    out.push(("shield_bands", bands.uv_scaled(6.0, 1.0)));
    out.push(("shield_rivets", rivets));
    // A leather belt with a bronze buckle, and a gorget where the neck meets it.
    out.push((
        "shield_belt",
        ring(profile_r(&BARREL, -0.12) + 0.012, 0.05, 40, 8)
            .transformed(squash_z)
            .transformed(at(0.0, -0.12, 0.0))
            .uv_scaled(8.0, 1.0),
    ));
    out.push((
        "shield_buckle",
        extrude(
            &[
                Vec2::new(-0.045, -0.05),
                Vec2::new(0.045, -0.05),
                Vec2::new(0.045, 0.05),
                Vec2::new(-0.045, 0.05),
            ],
            0.03,
        )
        .transformed(Mat4::from_rotation_y(FRAC_PI_2) * Mat4::IDENTITY)
        .transformed(at(profile_r(&BARREL, -0.12) + 0.05, -0.12, 0.0)),
    ));
    // Pauldrons: domed shoulder plates.
    let mut pauldrons = MeshData::default();
    for side in [-1.0f32, 1.0] {
        pauldrons.merge(
            &ellipsoid(0.17, 0.09, 0.19, 8, 16)
                .transformed(Mat4::from_rotation_x(0.35 * side))
                .transformed(at(0.0, 0.33, 0.44 * side)),
        );
    }
    out.push(("shield_pauldrons", pauldrons.sculpted(9, 0.006, 8.0, 2)));

    // The great helm: squat, riveted, with a slit visor and a flowing plume.
    let mut helm = lathe(
        &[
            (0.0, 0.0),
            (0.24, 0.0),
            (0.28, 0.10),
            (0.26, 0.22),
            (0.17, 0.32),
            (0.0, 0.36),
        ],
        28,
    );
    for i in 0..10 {
        let a = FRAC_PI_2 * 0.9 * (i as f32 / 9.0 * 2.0 - 1.0);
        helm.merge(&ellipsoid(0.014, 0.014, 0.014, 4, 8).transformed(at(
            0.275 * a.cos(),
            0.245,
            0.275 * a.sin(),
        )));
    }
    out.push((
        "shield_helm",
        helm.sculpted(15, 0.006, 8.0, 2).uv_scaled(2.0, 1.0),
    ));
    out.push((
        "shield_plume",
        ribbon(
            &[
                Vec3::new(0.0, 0.34, 0.0),
                Vec3::new(-0.12, 0.47, 0.0),
                Vec3::new(-0.28, 0.45, 0.0),
                Vec3::new(-0.42, 0.32, 0.0),
                Vec3::new(-0.50, 0.14, 0.0),
            ],
            |t| 0.11 * (1.0 - 0.75 * t),
            Vec3::Z,
        ),
    ));
    out.push((
        "shield_visor",
        ellipsoid(0.04, 0.03, 0.17, 6, 12).transformed(at(0.25, 0.15, 0.0)),
    ));
    let mut eyes = ellipsoid(0.03, 0.03, 0.035, 5, 8).transformed(at(0.285, 0.15, 0.07));
    eyes.merge(&ellipsoid(0.03, 0.03, 0.035, 5, 8).transformed(at(0.285, 0.15, -0.07)));
    out.push(("shield_eyes", eyes));

    // The tower shield: a slab standing in the Y-Z plane (its faces look
    // along X), with a rim, a boss, a cross of ridges and a bell sigil.
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
    out.push((
        "shield_rim",
        extrude(&outline, 0.16)
            .transformed(upright)
            .box_mapped(0.9, Vec2::ZERO),
    ));
    out.push((
        "shield_face",
        extrude(&scaled(0.84), 0.21)
            .transformed(upright)
            .box_mapped(0.9, Vec2::new(0.3, 0.1)),
    ));
    let mut trim = MeshData::default();
    let mut sigil = MeshData::default();
    for side in [-1.0f32, 1.0] {
        // The boss.
        trim.merge(&ellipsoid(0.07, 0.15, 0.15, 8, 14).transformed(at(0.105 * side, 0.08, 0.0)));
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
        // A little bell embossed above the boss, on each face.
        let bell = lathe(
            &[
                (0.0, 0.0),
                (0.075, 0.0),
                (0.06, 0.04),
                (0.045, 0.10),
                (0.022, 0.135),
                (0.0, 0.145),
            ],
            14,
        )
        .transformed(Mat4::from_rotation_z(-FRAC_PI_2 * side))
        .transformed(at(0.112 * side, 0.36, 0.0));
        sigil.merge(&bell.transformed(upright));
    }
    out.push(("shield_trim", trim.box_mapped(0.9, Vec2::new(0.6, 0.2))));
    out.push(("shield_sigil", sigil));

    // The weak spot: a glowing furnace vent set into the flank, behind bars.
    out.push((
        "shield_vent",
        ellipsoid(0.07, 0.20, 0.20, 10, 16).transformed(at(0.0, 0.08, 0.0)),
    ));
    let mut grille = MeshData::default();
    for side in [-1.0f32, 1.0] {
        for dz in [-0.16f32, -0.055, 0.055, 0.16] {
            grille.merge(&limb(
                Vec3::new(0.085 * side, -0.12, dz),
                Vec3::new(0.085 * side, 0.28, dz),
                0.013,
                0.013,
                5,
            ));
        }
        for dy in [-0.06f32, 0.22] {
            grille.merge(&limb(
                Vec3::new(0.085 * side, dy, -0.19),
                Vec3::new(0.085 * side, dy, 0.19),
                0.012,
                0.012,
                5,
            ));
        }
    }
    out.push(("shield_grille", grille));

    // Greaves and boots.
    let mut leg = limb(Vec3::ZERO, Vec3::new(0.0, -0.32, 0.0), 0.13, 0.10, 12);
    leg.merge(&ellipsoid(0.13, 0.09, 0.14, 8, 14).transformed(at(0.02, -0.005, 0.0)));
    out.push((
        "shield_leg",
        leg.sculpted(17, 0.006, 8.0, 2).uv_scaled(2.0, 1.0),
    ));
    out.push((
        "shield_boot",
        ellipsoid(0.17, 0.07, 0.13, 8, 14)
            .transformed(at(0.04, -0.35, 0.0))
            .sculpted(19, 0.006, 8.0, 2),
    ));
}

// ---------------------------------------------------------------- spitter --

fn spitter_meshes(out: &mut MeshList) {
    // A wet, swollen pod.
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
        40,
    )
    .sculpted(9, 0.035, 5.0, 3)
    .uv_scaled(2.0, 1.0)
    .recolor(|p| {
        let k = ((p.y + 0.30) / 0.7).clamp(0.0, 1.0);
        [0.62 + 0.38 * k, 0.72 + 0.28 * k, 0.6 + 0.4 * k, 1.0]
    });
    out.push(("spitter_pod", pod));
    // Glowing pustules of every size, clustered on both flanks.
    let mut spots = MeshData::default();
    for i in 0..15 {
        for side in [1.0f32, -1.0] {
            let h = |k: i32| hash3(31, i, k, (side as i32) + 2);
            let (x, y) = (-0.38 + 0.66 * h(0), -0.16 + 0.44 * h(1));
            // Sit on the pod's surface (a little sunk in).
            let r = profile_r(
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
                y,
            );
            let z = (r * r - x * x).max(0.0).sqrt() * 0.98;
            let size = 0.028 + 0.036 * h(2);
            spots.merge(&ellipsoid(size, size, size * 0.9, 6, 10).transformed(at(x, y, z * side)));
        }
    }
    out.push(("spitter_spots", spots));
    // The stalk: ringed like a windpipe, ending in a flared, toothed maw.
    let stalk_path = [
        Vec3::ZERO,
        Vec3::new(0.14, 0.06, 0.0),
        Vec3::new(0.30, 0.16, 0.0),
        Vec3::new(0.42, 0.18, 0.0),
    ];
    let mut neck = tube(&stalk_path, |t| 0.12 - 0.04 * t, 14).sculpted(12, 0.006, 10.0, 2);
    for i in 1..8 {
        let t = i as f32 / 8.0;
        let idx = ((t * 3.0) as usize).min(2);
        let f = t * 3.0 - idx as f32;
        let p = stalk_path[idx].lerp(stalk_path[idx + 1], f);
        let dir = stalk_path[idx + 1] - stalk_path[idx];
        let r = 0.12 - 0.04 * t + 0.012;
        neck.merge(&ring(r, 0.016, 16, 5).transformed(aim(p, dir)));
    }
    out.push(("spitter_neck", neck.uv_scaled(1.0, 3.0)));
    let centre = Vec3::new(0.40, 0.18, 0.0);
    let mut petals = MeshData::default();
    for k in 0..5 {
        let a = TAU * k as f32 / 5.0;
        let (c, s) = (a.cos(), a.sin());
        let path: Vec<Vec3> = [
            (0.00, 0.075),
            (0.06, 0.11),
            (0.13, 0.165),
            (0.20, 0.215),
            (0.24, 0.245),
        ]
        .iter()
        .map(|&(dx, r)| centre + Vec3::new(dx, r * c, r * s))
        .collect();
        // Lobes that fatten toward the tip, like a tulip's.
        petals.merge(&tube(
            &path,
            |t| 0.024 + 0.034 * t.powf(0.8) * (1.0 - 0.3 * t * t),
            9,
        ));
    }
    // A collar where the petals join.
    petals.merge(
        &ring(0.085, 0.024, 20, 6)
            .transformed(Mat4::from_translation(centre) * Mat4::from_rotation_z(-FRAC_PI_2)),
    );
    out.push(("spitter_maw", petals.sculpted(13, 0.006, 12.0, 2)));
    // Teeth ringing the throat, pointing in and forward.
    let mut teeth = MeshData::default();
    for k in 0..10 {
        let a = TAU * k as f32 / 10.0 + 0.15;
        let (c, s) = (a.cos(), a.sin());
        let base = Vec3::new(0.50, 0.18 + 0.105 * c, 0.105 * s);
        teeth.merge(
            &cone(0.022, 0.10, 5).transformed(aim(base, Vec3::new(0.6, -0.8 * c, -0.8 * s))),
        );
    }
    out.push(("spitter_teeth", teeth));
    out.push((
        "spitter_mouth",
        ellipsoid(0.03, 0.10, 0.10, 8, 14).transformed(at(0.60, 0.18, 0.0)),
    ));
    out.push(("spitter_belly", ellipsoid(0.20, 0.20, 0.20, 12, 20)));
    // Insect legs: thigh, knee, shin and a clawed toe.
    let mut leg = limb(Vec3::ZERO, Vec3::new(0.06, -0.09, 0.0), 0.075, 0.055, 8);
    leg.merge(&ellipsoid(0.05, 0.05, 0.05, 5, 8).transformed(at(0.06, -0.09, 0.0)));
    leg.merge(&limb(
        Vec3::new(0.06, -0.09, 0.0),
        Vec3::new(0.02, -0.22, 0.0),
        0.048,
        0.030,
        7,
    ));
    out.push(("spitter_leg", leg.uv_scaled(1.0, 2.0)));
    let mut toe = ellipsoid(0.08, 0.035, 0.06, 5, 10).transformed(at(0.03, -0.235, 0.0));
    for dz in [-0.03f32, 0.03] {
        toe.merge(&thorn(
            Vec3::new(0.08, -0.235, dz),
            Vec3::new(1.0, -0.2, dz * 3.0),
            Vec3::new(0.0, -0.4, 0.0),
            0.10,
            0.018,
            0.0,
            5,
        ));
    }
    out.push(("spitter_toe", toe));
}

// ------------------------------------------------------------------ dummy --

fn dummy_meshes(out: &mut MeshList) {
    out.push((
        "dummy_post",
        limb(
            Vec3::new(0.0, 0.02, 0.0),
            Vec3::new(0.0, 0.92, 0.0),
            0.13,
            0.10,
            14,
        )
        .sculpted(23, 0.008, 6.0, 2)
        .uv_scaled(2.0, 2.0),
    ));
    // A turned wooden foot with a chamfered rim.
    out.push((
        "dummy_base",
        lathe(
            &[
                (0.0, 0.0),
                (0.38, 0.0),
                (0.41, 0.03),
                (0.37, 0.09),
                (0.20, 0.115),
                (0.13, 0.13),
                (0.0, 0.13),
            ],
            28,
        )
        .sculpted(25, 0.008, 6.0, 2)
        .uv_scaled(2.0, 1.0),
    ));
    let mut bar = limb(
        Vec3::new(-0.50, 0.0, 0.0),
        Vec3::new(0.50, 0.0, 0.0),
        0.05,
        0.05,
        10,
    );
    for x in [-0.50f32, 0.50] {
        bar.merge(&ellipsoid(0.06, 0.055, 0.055, 6, 10).transformed(at(x, 0.0, 0.0)));
    }
    out.push((
        "dummy_bar",
        bar.sculpted(27, 0.006, 8.0, 2).uv_scaled(3.0, 1.0),
    ));
    // Rope lashings: one round the chest (in the post's frame), and the
    // crossbar's, in the crossbar joint's own frame (its origin is on the post).
    out.push((
        "dummy_rope",
        ring(0.132, 0.018, 20, 6)
            .transformed(at(0.0, 0.52, 0.0))
            .uv_scaled(4.0, 1.0),
    ));
    let mut lash = MeshData::default();
    for y in [-0.006f32, 0.016] {
        lash.merge(&ring(0.134, 0.018, 20, 6).transformed(at(0.0, y, 0.0)));
    }
    for x in [-0.30f32, 0.30] {
        lash.merge(
            &ring(0.062, 0.016, 12, 5)
                .transformed(at(x, 0.0, 0.0) * Mat4::from_rotation_z(FRAC_PI_2)),
        );
    }
    out.push(("dummy_lash", lash.uv_scaled(4.0, 1.0)));
    // Straw, poking out of the sleeves in loose, curved bundles.
    let mut straw = MeshData::default();
    for (x, sgn) in [(-0.53f32, -1.0f32), (0.53, 1.0)] {
        for k in 0..9 {
            let h = |s: i32| hash3(41, k, s, (sgn as i32) + 3) - 0.5;
            let dir = Vec3::new(sgn, 0.20 + 0.7 * h(0), h(1) * 1.6);
            straw.merge(&thorn(
                Vec3::new(x, h(2) * 0.05, h(3) * 0.09),
                dir,
                Vec3::new(0.0, -0.35 - 0.3 * h(4).abs(), h(5) * 0.3),
                0.22 + 0.10 * h(6).abs(),
                0.011,
                0.0,
                4,
            ));
        }
    }
    out.push(("dummy_straw", straw));
    // The head: a stuffed burlap sack, tied off at the neck and the crown.
    let mut head = ellipsoid(0.20, 0.22, 0.20, 16, 28)
        .sculpted(4, 0.018, 7.0, 3)
        .transformed(at(0.0, 0.10, 0.0));
    head.merge(&ring(0.105, 0.022, 20, 6).transformed(at(0.0, -0.08, 0.0)));
    head.merge(&cone(0.07, 0.11, 8).transformed(at(0.0, 0.30, 0.0)));
    out.push(("dummy_head", head.uv_scaled(2.0, 1.0)));
    // Button eyes and a stitched mouth, in dark thread.
    let mut face = ellipsoid(0.03, 0.035, 0.03, 6, 10).transformed(at(0.185, 0.16, 0.07));
    face.merge(&ellipsoid(0.03, 0.035, 0.03, 6, 10).transformed(at(0.185, 0.16, -0.07)));
    for z in [-0.07f32, 0.07] {
        for s in [-1.0f32, 1.0] {
            face.merge(&limb(
                Vec3::new(0.196, 0.16 - 0.035, z - 0.035 * s),
                Vec3::new(0.196, 0.16 + 0.035, z + 0.035 * s),
                0.006,
                0.006,
                4,
            ));
        }
    }
    face.merge(&limb(
        Vec3::new(0.19, 0.06, -0.08),
        Vec3::new(0.19, 0.06, 0.08),
        0.010,
        0.010,
        5,
    ));
    for k in 0..5 {
        let z = -0.07 + 0.035 * k as f32;
        face.merge(&limb(
            Vec3::new(0.19, 0.045, z),
            Vec3::new(0.19, 0.075, z),
            0.006,
            0.006,
            4,
        ));
    }
    out.push(("dummy_face", face));
    // Straw at the crown.
    let mut tuft = MeshData::default();
    for k in 0..9 {
        let phi = k as f32 / 9.0 * TAU;
        tuft.merge(&thorn(
            Vec3::new(phi.cos() * 0.05, 0.36, phi.sin() * 0.05),
            Vec3::new(phi.cos() * 0.4, 1.0, phi.sin() * 0.4),
            Vec3::new(phi.cos() * 0.5, -0.3, phi.sin() * 0.5),
            0.20,
            0.012,
            0.0,
            4,
        ));
    }
    out.push(("dummy_tuft", tuft));
    // The target: painted discs on the chest, on the camera-facing side.
    let mut cream = MeshData::default();
    let mut red = MeshData::default();
    cream.merge(&ellipsoid(0.20, 0.20, 0.03, 10, 24).transformed(at(0.0, 0.58, 0.115)));
    red.merge(&ellipsoid(0.135, 0.135, 0.036, 10, 24).transformed(at(0.0, 0.58, 0.117)));
    cream.merge(&ellipsoid(0.07, 0.07, 0.04, 8, 16).transformed(at(0.0, 0.58, 0.12)));
    out.push(("dummy_target_cream", cream.uv_scaled(3.0, 3.0)));
    out.push(("dummy_target_red", red.uv_scaled(3.0, 3.0)));
}

fn shard_meshes(out: &mut MeshList) {
    // A splinter: a three-sided spike, used for every species' death burst.
    out.push(("shard", cone(0.10, 0.34, 3)));
}

/// Every mesh of every creature, by name (also what the tests validate).
pub fn enemy_meshes() -> MeshList {
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
