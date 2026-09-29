//! The things placed in the world that the simulation cares about: doors (and
//! floor hatches), benches, ability pickups and spikes. Each is built from
//! `meshkit` pieces and dressed in the current area's stone.
//!
//! * A **door** is a pointed stone arch around a veil of pale light; a
//!   **hatch** (a wide, short exit) is a glowing opening with a column of
//!   light rising from it. Exits are all pale blue-white, in every area, so
//!   "the way onward" always looks the same.
//! * A **bench** is a stone seat with a wrought-iron back and a lantern on a
//!   post, warm in every area.
//! * A **pickup** is a floating orb with two turning rings and a light.
//! * **Spikes** are clusters of violet crystals, the same colour as every
//!   other hazard.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hk_sim::combat::{HitKind, Hitbox, Hurtbox};
use hk_sim::components::SimPos;
use hk_sim::world::room::{Ability, Bench, CurrentRoom, Pickup, RoomExit, RoomLibrary};

use super::kits::pointed_arch;
use super::pbr::{Kind, Materials};
use super::props::Flame;
use super::room::surface;
use super::style::style;
use crate::rig::meshkit::{cone, ellipsoid, extrude, hash3, ring, tube, MeshData};

/// Every exit glows this colour.
const EXIT_LIGHT: [f32; 3] = [0.55, 0.82, 1.0];

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}

fn rect(x0: f32, x1: f32, y0: f32, y1: f32) -> Vec<Vec2> {
    vec![
        Vec2::new(x0, y0),
        Vec2::new(x1, y0),
        Vec2::new(x1, y1),
        Vec2::new(x0, y1),
    ]
}

/// A door is a tall exit; a hatch is a wide, low one.
pub fn is_hatch(half: Vec2) -> bool {
    half.x > half.y * 1.4
}

/// The stone frame of a door of half size `half`, centred on the exit: two
/// jambs, a pointed arch over them, a threshold. (A hatch gets a low rim.)
pub fn door_frame(half: Vec2) -> MeshData {
    let mut m = MeshData::default();
    if is_hatch(half) {
        // A raised lip around a hole.
        for side in [-1.0f32, 1.0] {
            m.merge(
                &extrude(&rect(-0.16, 0.16, -half.y, half.y + 0.1), 1.4).transformed(at(
                    side * (half.x + 0.16),
                    0.0,
                    -0.2,
                )),
            );
        }
        return m;
    }
    let (hw, hh) = (half.x, half.y);
    for side in [-1.0f32, 1.0] {
        m.merge(
            &extrude(&rect(-0.16, 0.16, -hh - 0.1, hh + 0.2), 1.5).transformed(at(
                side * (hw + 0.16),
                0.0,
                -0.3,
            )),
        );
    }
    // The pointed arch over the top, and a thinner one behind it.
    m.merge(&pointed_arch(-hw - 0.16, hw + 0.16, hh - 0.3, -0.3, 0.2));
    m.merge(&pointed_arch(-hw - 0.5, hw + 0.5, hh - 0.5, -0.9, 0.16));
    // The threshold.
    m.merge(
        &extrude(&rect(-hw - 0.5, hw + 0.5, -hh - 0.14, -hh + 0.05), 1.5)
            .transformed(at(0.0, 0.0, -0.3)),
    );
    m
}

/// The veil of light in a doorway (or the glow of a hatch): additive, brightest
/// low down and fading up, with a soft second layer.
pub fn door_veil(half: Vec2) -> MeshData {
    let mut m = MeshData::default();
    let c = |a: f32| [EXIT_LIGHT[0], EXIT_LIGHT[1], EXIT_LIGHT[2], a];
    let mut layer = |w: f32, h: f32, lo: f32, hi: f32, z: f32| {
        m.add_quad(
            [
                Vec3::new(-w, -h, z),
                Vec3::new(w, -h, z),
                Vec3::new(w, h, z),
                Vec3::new(-w, h, z),
            ],
            Vec3::Z,
            [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
            [c(lo), c(lo), c(hi), c(hi)],
        );
    };
    if is_hatch(half) {
        // A pool of light on the opening and a column rising from it.
        layer(half.x, half.y, 0.5, 0.5, -0.3);
        let mut col = MeshData::default();
        col.add_quad(
            [
                Vec3::new(-half.x * 0.8, half.y, -0.25),
                Vec3::new(half.x * 0.8, half.y, -0.25),
                Vec3::new(half.x * 0.5, half.y + 3.5, -0.25),
                Vec3::new(-half.x * 0.5, half.y + 3.5, -0.25),
            ],
            Vec3::Z,
            [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
            [c(0.4), c(0.4), c(0.0), c(0.0)],
        );
        m.merge(&col);
    } else {
        layer(half.x, half.y, 0.55, 0.14, -0.2);
        layer(half.x * 0.6, half.y * 0.9, 0.5, 0.0, -0.1);
    }
    m
}

/// A bench (feet at the origin): stone seat on two legs, an iron back and a
/// lantern on a post. Returns `(stone, iron, lantern)`.
pub fn bench_meshes() -> (MeshData, MeshData, MeshData) {
    let mut stone = extrude(
        &[
            Vec2::new(-0.80, 0.24),
            Vec2::new(0.80, 0.24),
            Vec2::new(0.84, 0.30),
            Vec2::new(0.80, 0.38),
            Vec2::new(-0.80, 0.38),
            Vec2::new(-0.84, 0.30),
        ],
        0.7,
    );
    for x in [-0.55f32, 0.55] {
        stone.merge(&extrude(&rect(x - 0.08, x + 0.08, 0.0, 0.25), 0.6));
    }
    // A wrought-iron back: posts joined by a scrolling rail.
    let mut iron = tube(
        &[
            Vec3::new(-0.78, 0.36, -0.22),
            Vec3::new(-0.78, 0.9, -0.22),
            Vec3::new(-0.62, 1.12, -0.22),
            Vec3::new(-0.3, 1.16, -0.22),
            Vec3::new(0.0, 1.06, -0.22),
            Vec3::new(0.3, 1.16, -0.22),
            Vec3::new(0.62, 1.12, -0.22),
            Vec3::new(0.78, 0.9, -0.22),
            Vec3::new(0.78, 0.36, -0.22),
        ],
        |_| 0.04,
        6,
    );
    iron.merge(&tube(
        &[
            Vec3::new(-0.4, 0.4, -0.22),
            Vec3::new(-0.42, 0.75, -0.22),
            Vec3::new(-0.1, 0.9, -0.22),
        ],
        |_| 0.03,
        5,
    ));
    // The lamp post with a hook, at the bench's right end.
    iron.merge(&tube(
        &[
            Vec3::new(1.05, 0.0, -0.1),
            Vec3::new(1.05, 1.7, -0.1),
            Vec3::new(0.95, 1.95, -0.1),
            Vec3::new(0.7, 2.0, -0.1),
        ],
        |t| 0.06 - 0.02 * t,
        7,
    ));
    let mut lantern = ellipsoid(0.12, 0.16, 0.12, 6, 10).transformed(at(0.7, 1.8, -0.1));
    lantern.merge(&cone(0.1, 0.1, 6).transformed(at(0.7, 1.94, -0.1)));
    (stone, iron, lantern)
}

/// A cluster of crystals across `tiles` tiles (spanning `[0, tiles]` on x, the
/// floor at y = 0), a little taller than the hazard's own box (0.5), so the
/// danger is never smaller than it looks.
pub fn spike_crystals(tiles: i32, seed: u32) -> MeshData {
    let mut m = MeshData::default();
    for i in 0..tiles {
        for k in 0..4 {
            let h = |salt: i32| hash3(seed, i, k, salt);
            let x = i as f32 + 0.12 + 0.25 * k as f32 + (h(1) - 0.5) * 0.12;
            let height = 0.42 + 0.26 * h(2);
            let tilt = (h(3) - 0.5) * 0.5;
            let r = 0.14 + 0.06 * h(4);
            let crystal = cone(r, height, 5).recolor(move |p| {
                // Dark violet at the root, pale at the tip.
                let t = (p.y / height).clamp(0.0, 1.0);
                [0.22 + 0.9 * t, 0.07 + 0.7 * t, 0.40 + 1.1 * t, 1.0]
            });
            m.merge(
                &crystal
                    .transformed(at(x, -0.02, (h(5) - 0.5) * 0.6) * Mat4::from_rotation_z(tilt)),
            );
        }
    }
    m
}

/// A pickup's core and its two rings (centred on the origin).
pub fn pickup_meshes() -> (MeshData, MeshData, MeshData) {
    (
        ellipsoid(0.2, 0.2, 0.2, 10, 14),
        ring(0.42, 0.028, 28, 6),
        ring(0.32, 0.024, 24, 6),
    )
}

// ----------------------------------------------------------------- systems --

/// The glowing veil of an exit (tinted red while a boss seals the arena).
#[derive(Component)]
pub struct ExitVeil;

/// An exit's light.
#[derive(Component)]
pub struct ExitLight;

#[derive(Component)]
pub struct Spin {
    pub axis: Vec3,
    pub rate: f32,
}

#[derive(Component)]
pub struct Bob {
    pub base: f32,
    pub amp: f32,
}

/// Carved stone from the generated rock maps, tinted `c`.
fn stone_material(
    mats: &mut Assets<StandardMaterial>,
    pbr: &Materials,
    c: Color,
) -> Handle<StandardMaterial> {
    let l = c.to_linear();
    let tint = Color::linear_rgb(l.red * 0.9, l.green * 0.9, l.blue * 0.9);
    mats.add(surface(pbr, Kind::Rock, tint, 0.9, 0.0, false))
}

/// A crystal: glass that glows, in the colour `c` with emission `e`.
fn crystal_material(
    mats: &mut Assets<StandardMaterial>,
    c: Color,
    e: LinearRgba,
    thickness: f32,
) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: c,
        emissive: e,
        perceptual_roughness: 0.12,
        reflectance: 0.6,
        specular_transmission: 0.65,
        ior: 1.55,
        thickness,
        attenuation_color: c,
        attenuation_distance: 0.25,
        ..default()
    })
}

fn glow_material(mats: &mut Assets<StandardMaterial>, e: LinearRgba) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.15, 0.1),
        emissive: e,
        ..default()
    })
}

fn additive(mats: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    })
}

/// Doors, benches and pickups get their models when the room is entered.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn attach_fixtures(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pbr: Res<Materials>,
    library: Res<RoomLibrary>,
    current: Res<CurrentRoom>,
    exits: Query<(Entity, &RoomExit, &SimPos), Added<RoomExit>>,
    benches: Query<(Entity, &Bench, &SimPos), Added<Bench>>,
    pickups: Query<(Entity, &Pickup, &SimPos), Added<Pickup>>,
) {
    let theme = library
        .get(&current.id)
        .map(|d| d.theme)
        .unwrap_or_default();
    let st = style(theme);
    let prep = |commands: &mut Commands, e: Entity, pos: &SimPos| {
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Visibility::default(),
        ));
    };

    for (e, x, pos) in &exits {
        prep(&mut commands, e, pos);
        let frame = meshes.add(
            door_frame(x.half)
                .box_mapped(0.35, Vec2::ZERO)
                .to_mesh_pbr(),
        );
        let veil = meshes.add(door_veil(x.half).to_mesh());
        let stone = stone_material(
            &mut mats,
            &pbr,
            st.stone.mix(&Color::srgb(0.3, 0.3, 0.32), 0.3),
        );
        let veil_mat = additive(&mut mats);
        commands.entity(e).with_children(|p| {
            p.spawn((Mesh3d(frame), MeshMaterial3d(stone), Transform::IDENTITY));
            p.spawn((
                ExitVeil,
                NotShadowCaster,
                Mesh3d(veil),
                MeshMaterial3d(veil_mat),
                Transform::IDENTITY,
            ));
            p.spawn((
                ExitLight,
                Flame {
                    base: 260_000.0,
                    phase: x.half.x,
                },
                PointLight {
                    intensity: 260_000.0,
                    range: 9.0,
                    color: Color::srgb(EXIT_LIGHT[0], EXIT_LIGHT[1], EXIT_LIGHT[2]),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 1.6),
            ));
        });
    }

    for (e, b, pos) in &benches {
        // The entity sits at the bench's centre; the model's feet at its base.
        prep(&mut commands, e, pos);
        let (stone, iron, lantern) = bench_meshes();
        let (stone, iron, lantern) = (
            meshes.add(stone.box_mapped(0.4, Vec2::ZERO).to_mesh_pbr()),
            meshes.add(iron.box_mapped(0.6, Vec2::ZERO).to_mesh_pbr()),
            meshes.add(lantern.to_mesh()),
        );
        let stone_m = stone_material(&mut mats, &pbr, st.cap);
        let iron_m = mats.add(surface(
            &pbr,
            Kind::Iron,
            Color::srgb(0.55, 0.55, 0.6),
            0.9,
            0.0,
            false,
        ));
        let lamp_m = glow_material(&mut mats, LinearRgba::rgb(3.4, 1.9, 0.6));
        let feet = -b.half.y;
        commands.entity(e).with_children(|p| {
            p.spawn((
                Mesh3d(stone),
                MeshMaterial3d(stone_m),
                Transform::from_xyz(0.0, feet, 0.0),
            ));
            p.spawn((
                Mesh3d(iron),
                MeshMaterial3d(iron_m),
                Transform::from_xyz(0.0, feet, 0.0),
            ));
            p.spawn((
                NotShadowCaster,
                Mesh3d(lantern),
                MeshMaterial3d(lamp_m),
                Transform::from_xyz(0.0, feet, 0.0),
            ));
            p.spawn((
                Flame {
                    base: 520_000.0,
                    phase: pos.0.x,
                },
                PointLight {
                    intensity: 520_000.0,
                    range: 12.0,
                    color: Color::srgb(1.0, 0.72, 0.4),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.7, feet + 1.7, 0.8),
            ));
        });
    }

    for (e, pk, pos) in &pickups {
        prep(&mut commands, e, pos);
        let (core, r1, r2) = pickup_meshes();
        let (core, r1, r2) = (
            meshes.add(core.to_mesh_pbr()),
            meshes.add(r1.to_mesh_pbr()),
            meshes.add(r2.to_mesh_pbr()),
        );
        let (colour, emissive) = match pk.ability {
            Ability::Dash => (Color::srgb(0.5, 0.9, 1.0), LinearRgba::rgb(0.8, 2.8, 4.0)),
            Ability::WallGrip => (Color::srgb(1.0, 0.8, 0.4), LinearRgba::rgb(4.0, 2.4, 0.7)),
        };
        let core_m = crystal_material(&mut mats, colour, emissive, 0.35);
        let ring_m = crystal_material(&mut mats, colour, emissive * 0.6, 0.1);
        commands.entity(e).with_children(|p| {
            p.spawn((
                Bob {
                    base: 0.0,
                    amp: 0.1,
                },
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|b| {
                b.spawn((Mesh3d(core), MeshMaterial3d(core_m), Transform::IDENTITY));
                b.spawn((
                    Spin {
                        axis: Vec3::new(0.3, 1.0, 0.2).normalize(),
                        rate: 1.4,
                    },
                    Mesh3d(r1),
                    MeshMaterial3d(ring_m.clone()),
                    Transform::from_rotation(Quat::from_rotation_x(1.1)),
                ));
                b.spawn((
                    Spin {
                        axis: Vec3::new(1.0, 0.2, 0.4).normalize(),
                        rate: -1.9,
                    },
                    Mesh3d(r2),
                    MeshMaterial3d(ring_m),
                    Transform::from_rotation(Quat::from_rotation_z(0.9)),
                ));
                b.spawn((
                    Flame {
                        base: 380_000.0,
                        phase: pos.0.y,
                    },
                    PointLight {
                        intensity: 380_000.0,
                        range: 10.0,
                        color: colour,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.0, 0.9),
                ));
            });
        });
    }
}

/// Spikes are clusters of violet crystals.
#[allow(clippy::type_complexity)]
pub fn attach_spikes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &Hitbox, &Hurtbox, &SimPos), Added<Hurtbox>>,
    mut material: Local<Option<Handle<StandardMaterial>>>,
) {
    for (e, hb, hu, pos) in &q {
        if hb.kind != HitKind::Hazard || hu.team != hk_sim::combat::Team::Hazard {
            continue;
        }
        let mat = material
            .get_or_insert_with(|| {
                // Violet glass with a glow inside: the hazard colour, unchanged.
                crystal_material(
                    &mut mats,
                    Color::srgb(0.75, 0.55, 1.0),
                    LinearRgba::rgb(0.55, 0.10, 0.95),
                    0.4,
                )
            })
            .clone();
        let tiles = (hu.half.x * 2.0).round().max(1.0) as i32;
        let seed = ((pos.0.x * 7.0) as i32 as u32).wrapping_mul(2_654_435_761)
            ^ ((pos.0.y * 13.0) as i32 as u32);
        // The hazard's box is centred; its floor is `half.y` below the centre.
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y - hu.half.y, 0.0),
            Visibility::default(),
            crate::interp::Interpolated {
                z: 0.0,
                offset: Vec2::new(0.0, -hu.half.y),
            },
            Mesh3d(
                meshes.add(
                    spike_crystals(tiles, seed)
                        .transformed(at(-(tiles as f32) / 2.0, 0.0, 0.0))
                        .to_mesh_pbr(),
                ),
            ),
            MeshMaterial3d(mat),
        ));
    }
}

/// Pickups bob and their rings turn.
pub fn animate_fixtures(
    time: Res<Time>,
    mut spins: Query<(&Spin, &mut Transform), Without<Bob>>,
    mut bobs: Query<(&Bob, &mut Transform), Without<Spin>>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs().min(0.05);
    for (s, mut tr) in &mut spins {
        tr.rotate(Quat::from_axis_angle(s.axis, s.rate * dt));
    }
    for (b, mut tr) in &mut bobs {
        tr.translation.y = b.base + b.amp * (t * 2.2).sin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hatches_and_doors_are_told_apart_by_shape() {
        assert!(is_hatch(Vec2::new(1.5, 0.5)));
        assert!(!is_hatch(Vec2::new(0.5, 1.5)));
        assert!(!is_hatch(Vec2::new(1.0, 1.0)));
    }

    #[test]
    fn door_and_hatch_meshes_are_well_formed() {
        for half in [
            Vec2::new(0.5, 2.0),
            Vec2::new(1.0, 1.5),
            Vec2::new(1.5, 0.5),
        ] {
            let f = door_frame(half);
            f.validate()
                .unwrap_or_else(|e| panic!("frame {half:?}: {e}"));
            let v = door_veil(half);
            v.validate()
                .unwrap_or_else(|e| panic!("veil {half:?}: {e}"));
        }
    }

    #[test]
    fn a_door_frame_stands_around_its_opening_not_in_it() {
        let half = Vec2::new(0.5, 2.0);
        let (lo, hi) = door_frame(half).bounds();
        assert!(lo.x < -half.x && hi.x > half.x, "wider than the opening");
        assert!(hi.y > half.y, "the arch rises above the top");
        // Nothing crosses the middle of the doorway at body height in front of the veil.
        let m = door_frame(half);
        for p in &m.pos {
            let inside = p[0].abs() < half.x - 0.05 && p[1].abs() < half.y - 0.6 && p[2] > -0.15;
            assert!(!inside, "frame vertex in the doorway: {p:?}");
        }
    }

    #[test]
    fn a_bench_is_seat_height_with_a_lantern_above() {
        let (stone, iron, lantern) = bench_meshes();
        for (n, m) in [("stone", &stone), ("iron", &iron), ("lantern", &lantern)] {
            m.validate().unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        let (lo, hi) = stone.bounds();
        assert!(lo.y >= -1e-3 && hi.y < 0.5, "a low seat: {lo:?} {hi:?}");
        assert!(lantern.bounds().0.y > 1.4, "the lantern hangs high");
        // The whole bench fits the 1.2 x 1.2 box the simulation gives it, plus
        // a lamp post beside it.
        assert!(stone.bounds().1.x < 0.9 && iron.bounds().1.y < 2.1);
    }

    #[test]
    fn spike_crystals_stay_within_the_hazard_box() {
        let m = spike_crystals(5, 9);
        m.validate().expect("crystals");
        let (lo, hi) = m.bounds();
        assert!(lo.x > -0.2 && hi.x < 5.2, "spans its tiles: {lo:?} {hi:?}");
        assert!(hi.y <= 0.75, "barely taller than the hurtbox: {}", hi.y);
        assert!(hi.y > 0.45, "and tall enough to read");
        assert_eq!(spike_crystals(5, 9).pos, spike_crystals(5, 9).pos);
    }

    #[test]
    fn pickup_meshes_are_well_formed_and_the_rings_clear_the_core() {
        let (core, r1, r2) = pickup_meshes();
        core.validate().expect("core");
        r1.validate().expect("ring 1");
        r2.validate().expect("ring 2");
        assert!(core.bounds().1.x < r2.bounds().0.x.abs() - 0.05);
    }
}
