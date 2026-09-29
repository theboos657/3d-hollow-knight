//! The architecture behind the play lane: for each area a "kit" of large
//! silhouettes built from `meshkit` pieces (pillars and pointed arches,
//! mushrooms and roots, hanging bells, stained-glass windows, giant ribs),
//! set in front of the chamber wall at a few depths so the perspective camera
//! gives real parallax, plus glowing panes and slanting shafts of light.
//!
//! Everything is placed from a hash of the room's id, so a room always looks
//! the same, and it is all merged into three meshes: dark stone, glowing
//! parts (HDR vertex colours, drawn unlit) and additive light shafts.

use std::f32::consts::{PI, TAU};

use bevy::math::{Mat4, Vec2, Vec3};
use hk_sim::world::room::Theme;

use super::style::{style, LookStyle};
use crate::rig::meshkit::{cone, extrude, hash3, lathe, ribbon, ring, tube, MeshData};

/// Depths of the kit's layers (the chamber wall is behind them all).
pub const WALL_Z: f32 = -7.0;
pub const NEAR_Z: f32 = -4.4;
pub const MID_Z: f32 = -5.6;
/// Things that hang in front of the kit (chains, banners), still behind the level.
pub const HANG_Z: f32 = -2.7;

#[derive(Default)]
pub struct Kit {
    /// Stone, iron, wood: lit like the level.
    pub dark: MeshData,
    /// Windows, gills, bells' glints: HDR vertex colours, drawn unlit.
    pub glow: MeshData,
    /// Slanting light shafts: additive, alpha in the vertex colours.
    pub beams: MeshData,
}

struct Dice(u32);

impl Dice {
    fn f(&self, a: i32, b: i32) -> f32 {
        hash3(self.0, a, b, 77)
    }
    /// A value in `[lo, hi)`.
    fn range(&self, a: i32, b: i32, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f(a, b)
    }
}

fn shade(m: MeshData, k: f32) -> MeshData {
    m.tinted([k, k, k, 1.0])
}

/// Darkens toward the bottom (`lo` at `y0`, `hi` at `y1`), which reads as depth
/// and as mist gathering low.
fn vgrad(m: MeshData, y0: f32, y1: f32, lo: f32, hi: f32) -> MeshData {
    m.recolor(move |p| {
        let k = ((p.y - y0) / (y1 - y0)).clamp(0.0, 1.0);
        let v = lo + (hi - lo) * k;
        [v, v, v, 1.0]
    })
}

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

fn glow_colour(m: MeshData, c: [f32; 3]) -> MeshData {
    m.recolor(move |_| [c[0], c[1], c[2], 1.0])
}

// ------------------------------------------------------------------ pieces --

/// A fluted column with a flared foot and a capital. A `broken` one ends in a
/// jagged stump.
pub fn pillar(x: f32, y0: f32, y1: f32, z: f32, r: f32, broken: bool) -> MeshData {
    let mut profile = vec![
        (0.0, y0),
        (1.5 * r, y0),
        (1.5 * r, y0 + 0.5),
        (1.1 * r, y0 + 0.9),
        (r, y0 + 1.4),
    ];
    if broken {
        profile.extend([
            (0.95 * r, y1 - 0.6),
            (r, y1),
            (0.7 * r, y1 + 0.4),
            (0.6 * r, y1 + 0.2),
            (0.3 * r, y1 + 0.75),
            (0.0, y1 + 0.5),
        ]);
    } else {
        profile.extend([
            (0.92 * r, y1 - 2.0),
            (1.05 * r, y1 - 1.5),
            (1.3 * r, y1 - 1.0),
            (1.3 * r, y1 - 0.7),
            (1.6 * r, y1 - 0.5),
            (1.6 * r, y1),
            (0.0, y1),
        ]);
    }
    lathe(&profile, 14).transformed(at(x, 0.0, z))
}

/// A pointed (gothic) arch rib springing from `(xl, y)` to `(xr, y)`.
pub fn pointed_arch(xl: f32, xr: f32, y: f32, z: f32, r: f32) -> MeshData {
    let w = xr - xl;
    let big_r = 0.9 * w;
    let theta_apex = (w / (2.0 * big_r) - 1.0).clamp(-1.0, 1.0).acos();
    let n = 9;
    let mut path = Vec::new();
    for k in 0..=n {
        let th = PI + (theta_apex - PI) * k as f32 / n as f32;
        path.push(Vec3::new(
            xl + big_r + big_r * th.cos(),
            y + big_r * th.sin(),
            z,
        ));
    }
    for k in (0..n).rev() {
        let th = PI + (theta_apex - PI) * k as f32 / n as f32;
        path.push(Vec3::new(
            xr - big_r - big_r * th.cos(),
            y + big_r * th.sin(),
            z,
        ));
    }
    tube(&path, |t| r * (1.0 - 0.25 * (t * 2.0 - 1.0).abs()), 8)
}

/// The outline of a pointed window of width `w` and height `h`, base at 0.
fn window_outline(w: f32, h: f32) -> Vec<Vec2> {
    let shoulder = (h - w * 0.85).max(h * 0.5);
    vec![
        Vec2::new(-w / 2.0, 0.0),
        Vec2::new(w / 2.0, 0.0),
        Vec2::new(w / 2.0, shoulder),
        Vec2::new(w * 0.28, shoulder + (h - shoulder) * 0.62),
        Vec2::new(0.0, h),
        Vec2::new(-w * 0.28, shoulder + (h - shoulder) * 0.62),
        Vec2::new(-w / 2.0, shoulder),
    ]
}

/// A glowing pointed window on the wall, with dark mullions across it.
pub fn window(cx: f32, y0: f32, w: f32, h: f32, z: f32) -> (MeshData, MeshData) {
    let pane = extrude(&window_outline(w, h), 0.12).transformed(at(cx, y0, z));
    let bar = |x: f32, y: f32, bw: f32, bh: f32| {
        extrude(
            &[
                Vec2::new(x - bw / 2.0, y - bh / 2.0),
                Vec2::new(x + bw / 2.0, y - bh / 2.0),
                Vec2::new(x + bw / 2.0, y + bh / 2.0),
                Vec2::new(x - bw / 2.0, y + bh / 2.0),
            ],
            0.16,
        )
        .transformed(at(cx, y0, z + 0.06))
    };
    let mut frame = bar(0.0, h * 0.45, 0.14, h * 0.9);
    frame.merge(&bar(0.0, h / 3.0, w * 0.95, 0.12));
    frame.merge(&bar(0.0, h * 2.0 / 3.0, w * 0.95, 0.12));
    (pane, frame)
}

/// A slanting shaft of light from a window: a quad fading out downward, wider
/// at the bottom.
pub fn beam(
    cx: f32,
    y_top: f32,
    w: f32,
    drop: f32,
    lean: f32,
    z: f32,
    c: [f32; 3],
    a: f32,
) -> MeshData {
    let mut m = MeshData::default();
    let col = |alpha: f32| [c[0], c[1], c[2], alpha];
    m.add_quad(
        [
            Vec3::new(cx - w / 2.0 + lean - w * 0.5, y_top - drop, z),
            Vec3::new(cx + w / 2.0 + lean + w * 0.5, y_top - drop, z),
            Vec3::new(cx + w / 2.0, y_top, z),
            Vec3::new(cx - w / 2.0, y_top, z),
        ],
        Vec3::Z,
        [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
        [col(0.0), col(0.0), col(a), col(a)],
    );
    m
}

/// A hanging cloth banner with a swallow-tail.
pub fn banner(x: f32, y_top: f32, w: f32, len: f32, z: f32, phase: f32) -> MeshData {
    let n = 8;
    let path: Vec<Vec3> = (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            Vec3::new(x + 0.18 * (t * 5.0 + phase).sin() * t, y_top - len * t, z)
        })
        .collect();
    ribbon(&path, |t| w * (1.0 - 0.55 * t * t * t), Vec3::Z)
}

/// A chain hanging from `(x, y_top)`: a thin tube with a link every 0.4.
pub fn chain(x: f32, y_top: f32, len: f32, z: f32) -> MeshData {
    let mut m = tube(
        &[Vec3::new(x, y_top, z), Vec3::new(x, y_top - len, z)],
        |_| 0.035,
        5,
    );
    let links = (len / 0.42) as i32;
    for k in 0..links {
        let y = y_top - 0.2 - k as f32 * 0.42;
        let turn = if k % 2 == 0 {
            0.0
        } else {
            std::f32::consts::FRAC_PI_2
        };
        m.merge(
            &ring(0.07, 0.022, 8, 4)
                .transformed(Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2))
                .transformed(Mat4::from_rotation_y(turn))
                .transformed(at(x, y, z)),
        );
    }
    m
}

/// A great cast bell (hanging: its crown at `y_top`).
pub fn bell(x: f32, y_top: f32, r: f32, z: f32) -> MeshData {
    let s = r;
    lathe(
        &[
            (1.02 * s, 0.0),
            (1.06 * s, 0.10 * s),
            (0.92 * s, 0.16 * s),
            (0.84 * s, 0.40 * s),
            (0.66 * s, 0.85 * s),
            (0.42 * s, 1.20 * s),
            (0.26 * s, 1.40 * s),
            (0.30 * s, 1.52 * s),
            (0.0, 1.55 * s),
        ],
        20,
    )
    .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.5)))
    .transformed(at(x, y_top - 1.55 * s, z))
}

/// A giant mushroom: stalk, domed cap and a glowing ring of gills under it.
/// Returns `(stone, glow)`.
pub fn mushroom(x: f32, y0: f32, h: f32, cap_r: f32, z: f32) -> (MeshData, MeshData) {
    let stalk = lathe(
        &[
            (0.0, y0),
            (0.62, y0),
            (0.46, y0 + 0.4 * h),
            (0.38, y0 + 0.85 * h),
            (0.55, y0 + h),
        ],
        12,
    );
    let cap = lathe(
        &[
            (0.5, y0 + h - 0.1),
            (cap_r * 0.9, y0 + h + 0.02),
            (cap_r, y0 + h + 0.32),
            (cap_r * 0.78, y0 + h + 0.9),
            (cap_r * 0.42, y0 + h + 1.28),
            (0.0, y0 + h + 1.38),
        ],
        18,
    );
    let mut body = stalk;
    body.merge(&cap);
    let gills = ring(cap_r * 0.78, 0.10, 20, 5).transformed(at(0.0, y0 + h + 0.06, 0.0));
    // Squashed in depth: a round cap this wide would otherwise poke into the play lane.
    let flat = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.3));
    (
        body.transformed(flat).transformed(at(x, 0.0, z)),
        gills.transformed(flat).transformed(at(x, 0.0, z)),
    )
}

/// A hanging root or vine.
pub fn root(x: f32, y_top: f32, len: f32, z: f32, phase: f32) -> MeshData {
    let n = 8;
    let path: Vec<Vec3> = (0..=n)
        .map(|k| {
            let t = k as f32 / n as f32;
            Vec3::new(x + 0.35 * (t * 4.0 + phase).sin() * t, y_top - len * t, z)
        })
        .collect();
    tube(&path, |t| 0.16 * (1.0 - t * 0.9), 6)
}

/// A colossal rib arching out of the ground toward `dir` (+1 / -1).
pub fn rib(x: f32, y_base: f32, radius: f32, dir: f32, z: f32) -> MeshData {
    let n = 14;
    let path: Vec<Vec3> = (0..=n)
        .map(|k| {
            let a = k as f32 / n as f32 * PI * 0.78;
            Vec3::new(
                x + dir * (radius - radius * a.cos()),
                y_base + radius * a.sin(),
                z,
            )
        })
        .collect();
    tube(&path, |t| 0.95 * (1.0 - 0.72 * t), 9)
}

/// A jagged glowing crack in the wall, wandering from `(x, y)` in a rough
/// direction.
fn crack(x: f32, y: f32, len: f32, dice: &Dice, salt: i32, z: f32) -> MeshData {
    let n = 9;
    let (mut px, mut py) = (x, y);
    let mut path = Vec::new();
    for k in 0..=n {
        path.push(Vec3::new(px, py, z));
        let a = -PI / 2.0 + (dice.f(salt, k) - 0.5) * 1.9;
        px += a.cos() * len / n as f32;
        py += a.sin() * len / n as f32;
    }
    tube(&path, |t| 0.07 * (1.0 - 0.6 * t), 5)
}

/// A string of small lanterns hanging between two points.
pub fn lantern_string(x0: f32, x1: f32, y: f32, sag: f32, z: f32) -> (MeshData, MeshData) {
    let n = 12;
    let point = |t: f32| Vec3::new(x0 + (x1 - x0) * t, y - sag * 4.0 * t * (1.0 - t), z);
    let path: Vec<Vec3> = (0..=n).map(|k| point(k as f32 / n as f32)).collect();
    let wire = tube(&path, |_| 0.03, 4);
    let mut lamps = MeshData::default();
    let count = (((x1 - x0).abs() / 1.6) as i32).max(2);
    for k in 1..count {
        let p = point(k as f32 / count as f32);
        lamps.merge(
            &crate::rig::meshkit::ellipsoid(0.13, 0.17, 0.13, 6, 8).transformed(at(
                p.x,
                p.y - 0.22,
                p.z,
            )),
        );
    }
    (wire, lamps)
}

// ------------------------------------------------------------------- kits --

fn spread(w: f32, spacing: f32) -> (i32, impl Fn(f32, i32, &Dice) -> f32) {
    let n = ((w + 28.0) / spacing).ceil() as i32 + 1;
    (n, move |jitter: f32, i: i32, d: &Dice| {
        -14.0 + i as f32 * spacing + (d.f(i, 0) - 0.5) * jitter
    })
}

/// The glowing parts' colour: the area's flame colour, held below the level where
/// the tonemapper bleaches it to white.
fn tone(st: &LookStyle) -> [f32; 3] {
    [
        st.flame.red * 0.55,
        st.flame.green * 0.55,
        st.flame.blue * 0.55,
    ]
}

fn ashen(w: f32, h: f32, d: &Dice, st: &LookStyle, k: &mut Kit) {
    let glow = tone(st);
    let beam_c = [1.0, 0.78, 0.5];
    let (n, xf) = spread(w, 11.0);
    for i in 0..n {
        let x = xf(3.0, i, d);
        let x2 = xf(3.0, i + 1, d);
        let broken = d.f(i, 1) < 0.28;
        let y1 = if broken {
            h * 0.5 + d.f(i, 2) * h * 0.25
        } else {
            h + 12.0
        };
        k.dark.merge(&vgrad(
            pillar(x, -10.0, y1, NEAR_Z, 0.9, broken),
            -2.0,
            h * 0.7,
            0.55,
            1.0,
        ));
        if !broken && d.f(i, 3) > 0.25 {
            let y = h * 0.55 + d.range(i, 4, -2.5, 3.0);
            k.dark
                .merge(&shade(pointed_arch(x, x2, y, NEAR_Z, 0.55), 0.95));
            if d.f(i, 9) > 0.5 {
                k.dark.merge(&shade(
                    pointed_arch(x + 0.3, x2 - 0.3, y - 1.4, MID_Z, 0.4),
                    0.7,
                ));
            }
        }
        // A tall window between this pillar and the next, with a shaft of light.
        let wx = (x + x2) / 2.0;
        if d.f(i, 5) > 0.2 {
            let (wh, ww) = (d.range(i, 6, 6.5, 9.0), 2.5);
            let wy = d.range(i, 7, 1.5, 4.0);
            let (pane, frame) = window(wx, wy, ww, wh, WALL_Z + 0.15);
            k.glow.merge(&glow_colour(pane, glow));
            k.dark.merge(&shade(frame, 0.8));
            k.beams.merge(&beam(
                wx,
                wy + wh * 0.5,
                ww * 0.9,
                wh * 0.95,
                3.8,
                MID_Z - 0.5,
                beam_c,
                0.15,
            ));
        }
        if d.f(i, 8) > 0.55 {
            k.dark.merge(
                &shade(
                    banner(
                        wx + d.range(i, 10, -2.0, 2.0),
                        h + 8.0,
                        1.5,
                        d.range(i, 11, 5.0, 9.0),
                        HANG_Z,
                        i as f32,
                    ),
                    0.8,
                )
                .tinted([0.9, 0.35, 0.3, 1.0]),
            );
            k.dark
                .merge(&shade(chain(wx - 1.6, h + 8.0, 3.0, HANG_Z), 0.6));
        }
    }
}

fn warrens(w: f32, h: f32, d: &Dice, st: &LookStyle, k: &mut Kit) {
    let glow = tone(st);
    let (n, xf) = spread(w, 9.0);
    for i in 0..n {
        let x = xf(6.0, i, d);
        let near = d.f(i, 1) > 0.4;
        let cap = d.range(i, 2, 2.4, 4.2);
        let stalk_h = d.range(i, 3, h * 0.25, h * 0.6);
        let (z, tint) = if near { (NEAR_Z, 1.0) } else { (MID_Z, 0.7) };
        let (body, gills) = mushroom(x, -8.0, stalk_h + 8.0, cap, z);
        k.dark.merge(
            &vgrad(body, -2.0, h * 0.8, 0.4 * tint, 1.05 * tint).tinted([0.85, 1.0, 0.85, 1.0]),
        );
        k.glow.merge(&glow_colour(gills, glow));
        // A shaft of spore-light from the canopy.
        if d.f(i, 4) > 0.5 {
            k.beams.merge(&beam(
                x,
                h + 14.0,
                2.4,
                h + 14.0,
                -2.0,
                MID_Z - 0.4,
                [0.5, 1.0, 0.6],
                0.09,
            ));
        }
        // Roots and vines hang from the ceiling.
        for r in 0..2 {
            let rx = x + d.range(i, 20 + r, -4.5, 4.5);
            k.dark.merge(
                &shade(
                    root(
                        rx,
                        h + 10.0,
                        d.range(i, 30 + r, 8.0, h + 8.0),
                        HANG_Z - 0.6 * r as f32,
                        i as f32 + r as f32,
                    ),
                    0.75,
                )
                .tinted([0.7, 0.9, 0.6, 1.0]),
            );
        }
    }
}

fn cistern(w: f32, h: f32, d: &Dice, st: &LookStyle, k: &mut Kit) {
    let glow = tone(st);
    let (n, xf) = spread(w, 12.0);
    for i in 0..n {
        let x = xf(4.0, i, d);
        // A great bell on a chain, in fog.
        let r = d.range(i, 1, 1.6, 2.6);
        let ytop = h + d.range(i, 2, -2.0, 4.0);
        let z = if d.f(i, 3) > 0.5 { NEAR_Z } else { MID_Z };
        k.dark.merge(&shade(
            bell(x, ytop, r, z),
            if z == NEAR_Z { 1.0 } else { 0.72 },
        ));
        k.dark
            .merge(&shade(chain(x, h + 14.0, h + 14.0 - ytop, z), 0.7));
        k.glow.merge(&glow_colour(
            ring(r * 1.0, 0.04, 20, 4)
                .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.5)))
                .transformed(at(x, ytop - 1.55 * r + 0.12, z)),
            glow,
        ));
        // A round window and its shaft.
        if d.f(i, 4) > 0.4 {
            let wx = x + 6.0;
            let wy = h * 0.6 + d.range(i, 5, -3.0, 3.0);
            let disc = extrude(
                &(0..20)
                    .map(|a| {
                        let t = a as f32 / 20.0 * TAU;
                        Vec2::new(t.cos() * 1.1, t.sin() * 1.1)
                    })
                    .collect::<Vec<_>>(),
                0.1,
            )
            .transformed(at(wx, wy, WALL_Z + 0.15))
            .recolor(move |p| {
                // Bright in the middle, dimmer at the rim.
                let r = ((p.x - wx).powi(2) + (p.y - wy).powi(2)).sqrt() / 1.1;
                let v = 1.0 - 0.6 * r.min(1.0);
                [glow[0] * v, glow[1] * v, glow[2] * v, 1.0]
            });
            k.glow.merge(&disc);
            // A dark iron rim around it.
            k.dark.merge(&shade(
                ring(1.14, 0.09, 24, 5)
                    .transformed(Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .transformed(at(wx, wy, WALL_Z + 0.2)),
                0.6,
            ));
            k.beams.merge(&beam(
                wx,
                h * 0.6,
                2.6,
                h * 0.9,
                -3.0,
                MID_Z - 0.4,
                [0.6, 0.9, 1.0],
                0.09,
            ));
        }
    }
    // Long pipes run along the wall.
    let mut y = 3.0;
    while y < h + 6.0 {
        k.dark.merge(&shade(
            tube(
                &[
                    Vec3::new(-14.0, y, NEAR_Z - 0.3),
                    Vec3::new(w + 14.0, y, NEAR_Z - 0.3),
                ],
                |_| 0.32,
                10,
            ),
            0.6,
        ));
        y += d.range(y as i32, 6, 6.0, 9.0);
    }
}

fn spire(w: f32, h: f32, d: &Dice, st: &LookStyle, k: &mut Kit) {
    let (n, xf) = spread(w, 10.0);
    let panes = [
        [3.4, 0.5, 0.6],
        [0.6, 1.6, 3.6],
        [3.4, 2.5, 0.6],
        [0.7, 3.0, 1.4],
    ];
    for i in 0..n {
        let x = xf(3.0, i, d);
        let x2 = xf(3.0, i + 1, d);
        k.dark.merge(&vgrad(
            pillar(x, -10.0, h + 12.0, NEAR_Z, 0.75, false),
            -2.0,
            h * 0.7,
            0.55,
            1.0,
        ));
        if d.f(i, 3) > 0.3 {
            k.dark.merge(&shade(
                pointed_arch(x, x2, h * 0.7 + d.range(i, 4, -2.0, 2.0), NEAR_Z, 0.5),
                0.95,
            ));
        }
        // A stained-glass window: three bands of colour behind dark leading.
        let wx = (x + x2) / 2.0;
        let (wh, ww) = (d.range(i, 6, 7.0, 10.0), 2.6);
        let wy = d.range(i, 7, 1.0, 3.5);
        let bands = [
            panes[(d.f(i, 8) * 4.0) as usize % 4],
            panes[(d.f(i, 9) * 4.0) as usize % 4],
            panes[(d.f(i, 10) * 4.0) as usize % 4],
        ];
        let (pane, frame) = window(wx, wy, ww, wh, WALL_Z + 0.15);
        k.glow.merge(&pane.recolor(move |p| {
            let t = ((p.y - wy) / wh).clamp(0.0, 0.999);
            let c = bands[(t * 3.0) as usize];
            [c[0] * 0.55, c[1] * 0.55, c[2] * 0.55, 1.0]
        }));
        k.dark.merge(&shade(frame, 0.8));
        k.beams.merge(&beam(
            wx,
            wy + wh * 0.5,
            ww,
            wh,
            4.0,
            MID_Z - 0.4,
            [1.0, 0.85, 0.6],
            0.13,
        ));
        // Strings of lanterns between the pillars.
        if d.f(i, 12) > 0.4 {
            let (wire, lamps) = lantern_string(
                x + 0.5,
                x2 - 0.5,
                h * 0.4 + d.range(i, 13, 0.0, 4.0),
                1.2,
                HANG_Z,
            );
            k.dark.merge(&shade(wire, 0.6));
            k.glow.merge(&glow_colour(lamps, tone(st)));
        }
    }
}

fn throne(w: f32, h: f32, d: &Dice, st: &LookStyle, k: &mut Kit) {
    let glow = tone(st);
    let (n, xf) = spread(w, 13.0);
    for i in 0..n {
        let x = xf(5.0, i, d);
        let dir = if d.f(i, 1) > 0.5 { 1.0 } else { -1.0 };
        let radius = d.range(i, 2, 7.0, 12.0);
        let z = if d.f(i, 3) > 0.5 { NEAR_Z } else { MID_Z };
        k.dark.merge(&shade(
            rib(x, -6.0, radius, dir, z),
            if z == NEAR_Z { 1.0 } else { 0.7 },
        ));
        // Glowing fissures in the wall, and ember light from below.
        k.glow.merge(&glow_colour(
            crack(
                x + d.range(i, 4, -4.0, 4.0),
                d.range(i, 5, h * 0.4, h + 4.0),
                d.range(i, 6, 5.0, 10.0),
                d,
                i,
                WALL_Z + 0.12,
            ),
            glow,
        ));
        // A rising column of ember light.
        let base = x + d.range(i, 7, -2.0, 2.0);
        k.beams.merge(&{
            let mut m = MeshData::default();
            let c = |a: f32| [1.0, 0.4, 0.25, a];
            m.add_quad(
                [
                    Vec3::new(base - 1.6, -3.0, MID_Z - 0.4),
                    Vec3::new(base + 1.6, -3.0, MID_Z - 0.4),
                    Vec3::new(base + 0.6, h + 6.0, MID_Z - 0.4),
                    Vec3::new(base - 0.6, h + 6.0, MID_Z - 0.4),
                ],
                Vec3::Z,
                [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
                [c(0.07), c(0.07), c(0.0), c(0.0)],
            );
            m
        });
    }
}

/// The kit for an area, for a room `w` x `h` tiles, seeded by `seed`.
pub fn build_kit(theme: Theme, w: f32, h: f32, seed: u32) -> Kit {
    let st = style(theme);
    let dice = Dice(seed);
    let mut kit = Kit::default();
    match theme {
        Theme::Ashen | Theme::Sandbox => ashen(w, h, &dice, &st, &mut kit),
        Theme::Warrens => warrens(w, h, &dice, &st, &mut kit),
        Theme::Cistern => cistern(w, h, &dice, &st, &mut kit),
        Theme::Spire => spire(w, h, &dice, &st, &mut kit),
        Theme::Throne => throne(w, h, &dice, &st, &mut kit),
    }
    kit
}

/// A stalactite (a downward cone), for ceilings.
pub fn stalactite(x: f32, y_top: f32, len: f32, r: f32, z: f32) -> MeshData {
    cone(r, len, 6).transformed(at(x, y_top, z) * Mat4::from_rotation_x(PI))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEMES: [Theme; 6] = [
        Theme::Sandbox,
        Theme::Ashen,
        Theme::Warrens,
        Theme::Cistern,
        Theme::Spire,
        Theme::Throne,
    ];

    #[test]
    fn every_kit_is_well_formed_and_covers_the_room() {
        for t in THEMES {
            let kit = build_kit(t, 60.0, 20.0, 12345);
            for (name, m) in [
                ("dark", &kit.dark),
                ("glow", &kit.glow),
                ("beams", &kit.beams),
            ] {
                if m.vertex_count() == 0 {
                    assert_ne!(name, "dark", "{t:?} has no stone");
                    continue;
                }
                m.validate().unwrap_or_else(|e| panic!("{t:?} {name}: {e}"));
            }
            let (lo, hi) = kit.dark.bounds();
            assert!(
                lo.x < 0.0 && hi.x > 60.0,
                "{t:?} dark spans the room: {lo:?} {hi:?}"
            );
        }
    }

    #[test]
    fn nothing_stands_in_front_of_the_level_or_behind_the_wall() {
        for t in THEMES {
            let kit = build_kit(t, 50.0, 18.0, 7);
            for (name, m) in [
                ("dark", &kit.dark),
                ("glow", &kit.glow),
                ("beams", &kit.beams),
            ] {
                for p in &m.pos {
                    assert!(
                        p[2] < -1.8 && p[2] > WALL_Z - 0.5,
                        "{t:?} {name} has a vertex at z = {}",
                        p[2]
                    );
                }
            }
        }
    }

    #[test]
    fn kits_are_deterministic_and_vary_with_the_room() {
        let a = build_kit(Theme::Ashen, 50.0, 18.0, 1);
        let b = build_kit(Theme::Ashen, 50.0, 18.0, 1);
        let c = build_kit(Theme::Ashen, 50.0, 18.0, 2);
        assert_eq!(a.dark.pos, b.dark.pos);
        assert_ne!(a.dark.pos, c.dark.pos);
    }

    #[test]
    fn kits_stay_within_a_sensible_size() {
        // Merged meshes are uploaded once per room: keep them modest.
        for t in THEMES {
            let kit = build_kit(t, 90.0, 40.0, 3);
            let verts =
                kit.dark.vertex_count() + kit.glow.vertex_count() + kit.beams.vertex_count();
            assert!(verts < 120_000, "{t:?}: {verts} vertices");
        }
    }

    #[test]
    fn the_arch_is_pointed_and_spans_its_width() {
        let m = pointed_arch(0.0, 8.0, 5.0, -4.0, 0.5);
        let (lo, hi) = m.bounds();
        assert!(lo.x < 0.2 && hi.x > 7.8, "spans the gap");
        assert!(
            lo.y >= 4.4 && hi.y > 5.0 + 6.0,
            "rises like a gothic arch: {hi:?}"
        );
    }

    #[test]
    fn a_window_is_a_pane_and_mullions_in_front_of_it() {
        let (pane, frame) = window(0.0, 1.0, 2.5, 8.0, WALL_Z);
        pane.validate().expect("pane");
        frame.validate().expect("frame");
        let (_, phi) = pane.bounds();
        let (_, fhi) = frame.bounds();
        assert!((phi.y - 9.0).abs() < 1e-3, "pane top");
        assert!(fhi.z > phi.z, "the frame is in front of the glass");
    }
}
