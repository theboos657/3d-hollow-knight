//! Water in the air and on the floor: glossy puddles that catch the light of
//! the hall, and drips that form on the ceilings, fall and splash, more or
//! fewer of them according to how wet the area is (`LookStyle::wet`).
//!
//! Where they go is pure ([`puddle_specs`], [`drip_specs`]) and unit-tested;
//! [`spawn_wet`] turns the specs into entities and [`animate_drips`] runs the
//! falling.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hk_sim::world::grid::{Tile, TileGrid};

use super::RoomVisual;
use crate::rig::meshkit::{ellipsoid, hash3, ring};

/// Top of a floor cap above its tile's top edge (see `level::cap_piece`).
const CAP_TOP: f32 = 0.18;
/// How hard drips fall, in tiles per second squared.
const GRAVITY: f32 = 16.0;
/// Seconds a bead swells on the ceiling before it lets go.
const FORMING: f32 = 0.9;
/// Seconds a splash lasts.
const SPLASH: f32 = 0.28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PuddleSpec {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Full width along x and depth along z.
    pub width: f32,
    pub depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DripSpec {
    pub x: f32,
    pub z: f32,
    /// Where the bead hangs and where it lands.
    pub top: f32,
    pub floor: f32,
    /// Seconds between drips, and where in that cycle this one starts.
    pub period: f32,
    pub offset: f32,
}

fn floor_tile(grid: &TileGrid, i: i32, j: i32) -> bool {
    // Open air above, inside the map (not the outer face of the roof).
    j + 2 < grid.height()
        && grid.get(i, j) == Tile::Solid
        && grid.get(i, j + 1) == Tile::Empty
        && grid.get(i, j + 2) == Tile::Empty
        && (-1..=1).all(|d| grid.get(i + d, j + 1) != Tile::Spike)
}

/// Where puddles lie: on open floors, about `wet * 24` of them at most.
pub fn puddle_specs(grid: &TileGrid, seed: u32, wet: f32) -> Vec<PuddleSpec> {
    let budget = (wet * 24.0).round() as usize;
    if budget == 0 {
        return Vec::new();
    }
    let mut out: Vec<PuddleSpec> = Vec::new();
    for j in 0..grid.height() {
        for i in 1..grid.width() - 1 {
            if !floor_tile(grid, i, j) || hash3(seed, i, j, 61) > wet * 0.45 {
                continue;
            }
            let h = |k: i32| hash3(seed, i, j, 62 + k);
            let spec = PuddleSpec {
                x: i as f32 + 0.25 + 0.5 * h(0),
                y: j as f32 + 1.0 + CAP_TOP + 0.004,
                z: -0.55 + 0.9 * h(1),
                width: 0.9 + 1.5 * h(2),
                depth: 0.35 + 0.5 * h(3),
            };
            // Never two puddles on top of one another.
            if out
                .iter()
                .all(|p| (p.x - spec.x).abs() > 1.6 || (p.y - spec.y).abs() > 0.5)
            {
                out.push(spec);
            }
            if out.len() >= budget {
                return out;
            }
        }
    }
    out
}

/// Where drips form: ceilings with open air below them, up to about
/// `2 + wet * 24` of them, each landing on the first floor beneath.
pub fn drip_specs(grid: &TileGrid, seed: u32, wet: f32) -> Vec<DripSpec> {
    if wet <= 0.0 {
        return Vec::new();
    }
    let budget = 1 + (wet * 24.0).round() as usize;
    let mut out: Vec<DripSpec> = Vec::new();
    for j in 2..grid.height() {
        for i in 1..grid.width() - 1 {
            let ceiling = grid.get(i, j) == Tile::Solid && grid.get(i, j - 1) == Tile::Empty;
            if !ceiling || hash3(seed, i, j, 71) > wet * 0.5 {
                continue;
            }
            // The first floor below.
            let mut fy = None;
            for k in (0..j).rev() {
                match grid.get(i, k) {
                    Tile::Solid => {
                        fy = Some(k as f32 + 1.0 + CAP_TOP);
                        break;
                    }
                    Tile::OneWay => {
                        fy = Some(k as f32 + 1.0);
                        break;
                    }
                    Tile::Spike => {
                        fy = Some(k as f32 + 0.5);
                        break;
                    }
                    _ => {}
                }
            }
            let Some(floor) = fy else { continue };
            let top = j as f32;
            if top - floor < 2.0 {
                continue;
            }
            let h = |k: i32| hash3(seed, i, j, 72 + k);
            out.push(DripSpec {
                x: i as f32 + 0.2 + 0.6 * h(0),
                z: -0.4 + 0.8 * h(1),
                top,
                floor,
                period: 2.4 + 3.6 * h(2),
                offset: 8.0 * h(3),
            });
            if out.len() >= budget {
                return out;
            }
        }
    }
    out
}

/// What a drip is doing `s` seconds into its cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DripState {
    /// A bead swelling on the ceiling (`0..1` of its full size).
    Forming(f32),
    /// Falling: how far it has dropped.
    Falling(f32),
    /// Splashing on the floor (`0..1` through the splash).
    Splashing(f32),
    Waiting,
}

/// Seconds a drip needs to fall `height`.
pub fn fall_time(height: f32) -> f32 {
    (2.0 * height / GRAVITY).sqrt()
}

pub fn drip_state(spec: &DripSpec, t: f32) -> DripState {
    let s = (t + spec.offset).rem_euclid(spec.period);
    let height = spec.top - spec.floor;
    let fall = fall_time(height);
    if s < FORMING {
        DripState::Forming(s / FORMING)
    } else if s < FORMING + fall {
        let u = s - FORMING;
        DripState::Falling((0.5 * GRAVITY * u * u).min(height))
    } else if s < FORMING + fall + SPLASH {
        DripState::Splashing((s - FORMING - fall) / SPLASH)
    } else {
        DripState::Waiting
    }
}

#[derive(Component)]
pub struct Drip {
    spec: DripSpec,
    splash: Entity,
}

#[derive(Component)]
pub struct Splash;

/// Puddles and drips for a room.
pub fn spawn_wet(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    grid: &TileGrid,
    seed: u32,
    wet: f32,
) {
    let puddles = puddle_specs(grid, seed, wet);
    if !puddles.is_empty() {
        let disc = meshes.add(ellipsoid(0.5, 1.0, 0.5, 4, 24).to_mesh());
        // A dark mirror: it shows the hall's light (the environment map, where
        // the tier has one), not a colour of its own.
        let water = mats.add(StandardMaterial {
            base_color: Color::srgb(0.30, 0.40, 0.50),
            perceptual_roughness: 0.04,
            metallic: 0.85,
            reflectance: 0.9,
            ..default()
        });
        for p in puddles {
            commands.spawn((
                RoomVisual,
                NotShadowCaster,
                Mesh3d(disc.clone()),
                MeshMaterial3d(water.clone()),
                Transform::from_xyz(p.x, p.y, p.z).with_scale(Vec3::new(p.width, 0.011, p.depth)),
            ));
        }
    }
    let drips = drip_specs(grid, seed, wet);
    if drips.is_empty() {
        return;
    }
    let bead = meshes.add(ellipsoid(0.032, 0.055, 0.032, 6, 10).to_mesh());
    let splash_mesh = meshes.add(ring(0.16, 0.012, 16, 4).to_mesh());
    // A drop catching the light: a faint additive gleam.
    let gleam = mats.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.75, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    for spec in drips {
        let splash = commands
            .spawn((
                RoomVisual,
                Splash,
                NotShadowCaster,
                Mesh3d(splash_mesh.clone()),
                MeshMaterial3d(gleam.clone()),
                Transform::from_xyz(spec.x, spec.floor + 0.01, spec.z).with_scale(Vec3::ZERO),
            ))
            .id();
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Drip { spec, splash },
            Mesh3d(bead.clone()),
            MeshMaterial3d(gleam.clone()),
            Transform::from_xyz(spec.x, spec.top, spec.z).with_scale(Vec3::ZERO),
        ));
    }
}

pub fn animate_drips(
    time: Res<Time>,
    drips: Query<(&Drip, &mut Transform), Without<Splash>>,
    mut splashes: Query<&mut Transform, With<Splash>>,
) {
    let t = time.elapsed_secs();
    for (d, mut tr) in drips {
        let spec = d.spec;
        let (pos, scale, splash) = match drip_state(&spec, t) {
            DripState::Forming(f) => (spec.top - 0.06 * f, Vec3::splat(0.25 + 0.75 * f), 0.0),
            DripState::Falling(dy) => (spec.top - 0.06 - dy, Vec3::new(0.8, 1.4, 0.8), 0.0),
            DripState::Splashing(f) => (spec.top, Vec3::ZERO, f),
            DripState::Waiting => (spec.top, Vec3::ZERO, 0.0),
        };
        tr.translation.y = pos;
        tr.scale = scale;
        if let Ok(mut s) = splashes.get_mut(d.splash) {
            // The ring opens and thins away.
            let k = if splash > 0.0 {
                (splash * (2.0 - splash)).min(1.0)
            } else {
                0.0
            };
            s.scale = if splash > 0.0 {
                Vec3::new(k, 1.0, k) * (1.0 - splash * 0.5)
            } else {
                Vec3::ZERO
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hall() -> TileGrid {
        TileGrid::from_ascii(&[
            "####################",
            "####################",
            "#..................#",
            "#..................#",
            "#..................#",
            "#..................#",
            "#..................#",
            "####################",
        ])
    }

    #[test]
    fn dry_areas_get_nothing_and_wet_ones_get_more() {
        let g = hall();
        assert!(puddle_specs(&g, 3, 0.0).is_empty());
        assert!(drip_specs(&g, 3, 0.0).is_empty());
        let (a, b) = (puddle_specs(&g, 3, 0.12), puddle_specs(&g, 3, 0.35));
        assert!(b.len() > a.len(), "{} vs {}", b.len(), a.len());
        let (c, d) = (drip_specs(&g, 3, 0.12), drip_specs(&g, 3, 0.35));
        assert!(d.len() > c.len());
    }

    #[test]
    fn puddles_lie_on_floors_and_never_overlap() {
        let g = hall();
        let specs = puddle_specs(&g, 9, 0.35);
        assert!(!specs.is_empty());
        for p in &specs {
            // The floor's top is y = 1 (the bottom row is solid) plus the cap.
            assert!((p.y - (1.0 + CAP_TOP)).abs() < 0.01, "y = {}", p.y);
            assert!(p.x > 1.0 && p.x < 19.0);
            assert!(p.width > 0.5 && p.depth > 0.2);
            assert!(p.z.abs() < 1.0, "inside the play lane's depth");
        }
        for (a, b) in specs.iter().zip(specs.iter().skip(1)) {
            assert!((a.x - b.x).abs() > 1.6 || (a.y - b.y).abs() > 0.5);
        }
        assert_eq!(specs, puddle_specs(&g, 9, 0.35), "deterministic");
    }

    #[test]
    fn drips_hang_from_ceilings_and_land_on_the_floor_below() {
        let g = hall();
        let specs = drip_specs(&g, 5, 0.35);
        assert!(!specs.is_empty());
        for d in &specs {
            assert!(d.top > d.floor + 2.0, "a real fall: {d:?}");
            assert!((d.top - 6.0).abs() < 1e-3, "hangs from the ceiling: {d:?}");
            assert!(
                (d.floor - (1.0 + CAP_TOP)).abs() < 0.01,
                "lands on the floor"
            );
            assert!(d.period >= 2.4 && d.period <= 6.0);
        }
    }

    #[test]
    fn a_drip_swells_falls_accelerating_splashes_and_waits() {
        let spec = DripSpec {
            x: 5.0,
            z: 0.0,
            top: 8.0,
            floor: 1.0,
            period: 6.0,
            offset: 0.0,
        };
        assert!(matches!(drip_state(&spec, 0.1), DripState::Forming(f) if f < 0.2));
        assert!(matches!(drip_state(&spec, FORMING - 0.01), DripState::Forming(f) if f > 0.95));
        // Falling: farther each moment, and never past the floor.
        let fall = fall_time(7.0);
        let mut last = -1.0;
        for k in 1..10 {
            let s = FORMING + fall * k as f32 / 10.0;
            let DripState::Falling(dy) = drip_state(&spec, s) else {
                panic!("should be falling at {s}");
            };
            assert!(dy > last && dy <= 7.0 + 1e-4);
            last = dy;
        }
        assert!(matches!(
            drip_state(&spec, FORMING + fall + 0.05),
            DripState::Splashing(_)
        ));
        assert_eq!(
            drip_state(&spec, FORMING + fall + SPLASH + 0.1),
            DripState::Waiting
        );
        // The cycle repeats.
        let (DripState::Forming(a), DripState::Forming(b)) =
            (drip_state(&spec, 0.3), drip_state(&spec, 6.3))
        else {
            panic!("forming both times");
        };
        assert!((a - b).abs() < 1e-3);
    }
}
