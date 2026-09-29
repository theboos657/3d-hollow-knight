//! The title screen's stage: Nym on a broken cliff with a lantern and a fire,
//! looking across mist at the great bell in the moonlit distance while embers
//! rise, the camera drifting slowly. Built from the same pieces as the game
//! (level blocks, kits, the knight rig), and torn down when the game starts.

use bevy::light::NotShadowCaster;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use hk_sim::world::grid::{Tile, TileGrid};

use crate::look::kits::{bell, chain, pillar, pointed_arch};
use crate::look::pbr::Materials;
use crate::look::props::{brazier_meshes, Flame};
use crate::look::room::{level_mats, spawn_level};
use crate::look::style::style;
use crate::look::{KeyLight, Mote, RimLight};
use crate::menu::{Back, Screen};
use crate::models::knight::{spawn_knight, KnightAnim, KnightAssets};
use crate::rig::meshkit::{extrude, hash3, ring, MeshData};
use crate::scene::MainCamera;
use crate::viewer::{animate_viewer, pose, ViewerKnight};

pub struct TitleScenePlugin;

impl Plugin for TitleScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                sync_stage,
                drift_camera,
                drift_mist,
                // The viewer animates its own knights; on the title screen this does.
                animate_viewer.run_if(not(resource_exists::<crate::viewer::ViewerSet>)),
            ),
        );
    }
}

/// Everything on the stage.
#[derive(Component, Clone, Copy)]
struct TitleStage;

#[derive(Component)]
struct Mist {
    base_x: f32,
    speed: f32,
    phase: f32,
}

/// Where the knight stands.
const KNIGHT_AT: Vec3 = Vec3::new(12.0, 6.0, 0.0);

fn on_title(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Title | Screen::Options(Back::Title) | Screen::Controls(Back::Title)
    )
}

/// The cliff, as ASCII rows (top first), 40 wide: a stepped, broken edge and a
/// couple of floating stones.
pub fn cliff_grid() -> TileGrid {
    let (w, h) = (40, 8);
    let mut g = TileGrid::new(w, h);
    for j in 0..6 {
        // The face steps outward as it goes down.
        let edge = 14 + (5 - j) * 2 - (j % 2);
        for i in 0..edge {
            g.set(i, j, Tile::Solid);
        }
    }
    for (i, j) in [
        (19, 3),
        (20, 3),
        (21, 3),
        (20, 2),
        (24, 1),
        (25, 1),
        (25, 0),
    ] {
        g.set(i, j, Tile::Solid);
    }
    g
}

/// A soft glowing disc (the moon): HDR at the centre, fading to nothing.
fn moon_mesh(radius: f32) -> MeshData {
    let n = 48;
    let poly: Vec<Vec2> = (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            Vec2::new(a.cos() * radius, a.sin() * radius)
        })
        .collect();
    extrude(&poly, 0.1).recolor(move |p| {
        let r = (p.x * p.x + p.y * p.y).sqrt() / radius;
        let v = 1.6 - 0.9 * r * r;
        [v * 0.85, v * 0.95, v * 1.2, 1.0]
    })
}

/// A wide band of mist: brightest through the middle, fading to nothing at the
/// top and the bottom (so it has no visible edge).
fn mist_mesh(w: f32, h: f32) -> MeshData {
    let mut m = MeshData::default();
    let c = |a: f32| [0.45, 0.6, 0.9, a];
    for (y0, y1, a0, a1) in [(0.0, h * 0.4, 0.0, 0.09), (h * 0.4, h, 0.09, 0.0)] {
        m.add_quad(
            [
                Vec3::new(-w / 2.0, y0, 0.0),
                Vec3::new(w / 2.0, y0, 0.0),
                Vec3::new(w / 2.0, y1, 0.0),
                Vec3::new(-w / 2.0, y1, 0.0),
            ],
            Vec3::Z,
            [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y],
            [c(a0), c(a0), c(a1), c(a1)],
        );
    }
    m
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_stage(
    mut commands: Commands,
    screen: Res<Screen>,
    existing: Query<Entity, With<TitleStage>>,
    knight: Option<Res<KnightAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pbr: Res<Materials>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut cam: Query<(&mut DistanceFog, &mut ColorGrading), With<MainCamera>>,
    mut key: Query<&mut DirectionalLight, With<KeyLight>>,
    mut rim: Query<&mut DirectionalLight, (With<RimLight>, Without<KeyLight>)>,
) {
    if !screen.is_changed() {
        return;
    }
    let want = on_title(&screen);
    let have = !existing.is_empty();
    if !want {
        if have {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    if have {
        return;
    }
    let Some(knight) = knight else {
        return;
    };

    // Moonlight: cool key, a warm rim from the fire, deep indigo fog.
    let st = style(hk_sim::world::room::Theme::Cistern);
    ambient.color = Color::srgb(0.30, 0.38, 0.72);
    ambient.brightness = 190.0;
    let fog_colour = Color::srgb(0.03, 0.045, 0.10);
    clear.0 = fog_colour;
    for (mut fog, mut grading) in &mut cam {
        fog.color = fog_colour;
        // A long reach, so the moon and the bell are seen across the gulf.
        fog.falloff = bevy::pbr::FogFalloff::Linear {
            start: 40.0,
            end: 210.0,
        };
        grading.global.temperature = -0.05;
        grading.global.post_saturation = 1.12;
        grading.midtones.contrast = 1.08;
    }
    for mut l in &mut key {
        l.color = Color::srgb(0.62, 0.78, 1.0);
        l.illuminance = 4200.0;
    }
    for mut l in &mut rim {
        l.color = Color::srgb(1.0, 0.62, 0.32);
        l.illuminance = 1500.0;
    }

    let stage = TitleStage;
    // The cliff.
    let grid = cliff_grid();
    let lm = level_mats(&mut mats, &pbr, &st, true);
    spawn_level(&mut commands, &mut meshes, &lm, &grid, 7, stage);

    // The moon and its halo.
    let moon = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    commands.spawn((
        stage,
        NotShadowCaster,
        Mesh3d(meshes.add(moon_mesh(15.0).to_mesh())),
        MeshMaterial3d(moon),
        Transform::from_xyz(54.0, 17.0, -60.0),
    ));

    // The great bell, hanging in the mist on a chain that leaves the frame.
    let bronze = mats.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.30, 0.16),
        perceptual_roughness: 0.5,
        metallic: 0.7,
        emissive: LinearRgba::rgb(0.05, 0.03, 0.015),
        ..default()
    });
    let (bx, by, bz) = (41.0, 19.5, -28.0);
    commands.spawn((
        stage,
        Mesh3d(meshes.add(bell(bx, by, 8.5, bz).to_mesh())),
        MeshMaterial3d(bronze.clone()),
        Transform::IDENTITY,
    ));
    commands.spawn((
        stage,
        Mesh3d(meshes.add(chain(bx, 90.0, 90.0 - by, bz).to_mesh())),
        MeshMaterial3d(bronze.clone()),
        Transform::IDENTITY,
    ));
    let rune = mats.add(StandardMaterial {
        base_color: Color::linear_rgb(1.9, 0.9, 0.3),
        unlit: true,
        ..default()
    });
    commands.spawn((
        stage,
        NotShadowCaster,
        Mesh3d(
            meshes.add(
                ring(7.6, 0.12, 32, 6)
                    .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.5)))
                    .transformed(Mat4::from_translation(Vec3::new(bx, by - 6.5, bz)))
                    .to_mesh(),
            ),
        ),
        MeshMaterial3d(rune),
        Transform::IDENTITY,
    ));

    // Ruined pillars and arches across the distance.
    let dark = mats.add(StandardMaterial {
        base_color: Color::srgb(0.20, 0.24, 0.36),
        perceptual_roughness: 1.0,
        ..default()
    });
    let mut ruins = MeshData::default();
    let xs: Vec<f32> = (0..10).map(|k| -6.0 + k as f32 * 8.5).collect();
    for (k, &x) in xs.iter().enumerate() {
        // Far ones fill the distance; the near ones stay at the edges of the frame.
        let near = k % 3 == 0;
        let z = if near {
            -13.0
        } else {
            -34.0 - 6.0 * (k % 3) as f32
        };
        if near && (8.0..40.0).contains(&x) {
            continue;
        }
        let broken = hash3(3, k as i32, 0, 1) < 0.35;
        let top = if broken {
            12.0 + hash3(3, k as i32, 1, 1) * 8.0
        } else {
            44.0
        };
        let tint = if near { 0.7 } else { 0.5 };
        ruins.merge(&pillar(x, -6.0, top, z, 1.2, broken).tinted([
            tint,
            tint * 1.05,
            tint * 1.4,
            1.0,
        ]));
        if !broken && k + 1 < xs.len() && !near {
            ruins.merge(&pointed_arch(x, xs[k + 1], 17.0, z, 0.8).tinted([
                tint,
                tint * 1.05,
                tint * 1.4,
                1.0,
            ]));
        }
    }
    commands.spawn((
        stage,
        NotShadowCaster,
        Mesh3d(meshes.add(ruins.to_mesh())),
        MeshMaterial3d(dark),
        Transform::IDENTITY,
    ));

    // Mist drifting between us and the bell.
    let mist_mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    let mist = meshes.add(mist_mesh(70.0, 7.0).to_mesh());
    for (k, (z, y, speed)) in [(-4.0, 3.2, 0.05), (-9.0, 4.4, 0.08), (-16.0, 5.5, 0.04)]
        .into_iter()
        .enumerate()
    {
        commands.spawn((
            stage,
            NotShadowCaster,
            Mist {
                base_x: 22.0,
                speed,
                phase: k as f32 * 2.0,
            },
            Mesh3d(mist.clone()),
            MeshMaterial3d(mist_mat.clone()),
            Transform::from_xyz(22.0, y, z),
        ));
    }

    // A brazier by the knight, and embers rising from below.
    let (stand, flame) = brazier_meshes();
    let iron = mats.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.11, 0.12),
        perceptual_roughness: 0.6,
        metallic: 0.6,
        ..default()
    });
    let fire = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.12, 0.05),
        emissive: LinearRgba::rgb(3.6, 1.7, 0.45),
        ..default()
    });
    let (stand, flame) = (meshes.add(stand.to_mesh()), meshes.add(flame.to_mesh()));
    commands
        .spawn((
            stage,
            Transform::from_xyz(9.4, 6.0, 0.4),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((Mesh3d(stand), MeshMaterial3d(iron), Transform::IDENTITY));
            p.spawn((
                NotShadowCaster,
                Mesh3d(flame),
                MeshMaterial3d(fire),
                Transform::IDENTITY,
            ));
            p.spawn((
                Flame {
                    base: 700_000.0,
                    phase: 1.3,
                },
                PointLight {
                    intensity: 700_000.0,
                    range: 16.0,
                    color: Color::srgb(1.0, 0.62, 0.3),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.6, 1.2, 1.4),
            ));
        });
    let ember_mesh = meshes.add(Sphere::new(0.07));
    let ember = mats.add(StandardMaterial {
        base_color: st.mote,
        emissive: LinearRgba::rgb(3.4, 1.6, 0.5),
        unlit: true,
        ..default()
    });
    for k in 0..70 {
        let x = -2.0 + hash3(9, k, 0, 5) * 44.0;
        let y = 1.0 + hash3(9, k, 1, 5) * 6.0;
        let z = -0.5 - hash3(9, k, 2, 5) * 14.0;
        commands.spawn((
            stage,
            NotShadowCaster,
            Mote {
                base: Vec3::new(x, y, z),
                phase: hash3(9, k, 3, 5),
                rise: 0.5 + 0.7 * hash3(9, k, 4, 5),
                span: 8.0 + 8.0 * hash3(9, k, 6, 5),
            },
            Mesh3d(ember_mesh.clone()),
            MeshMaterial3d(ember.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }

    // The knight, breathing, with his lantern.
    let anchor = commands
        .spawn((
            stage,
            Transform::from_xyz(KNIGHT_AT.x, KNIGHT_AT.y + 0.75, 0.0),
            Visibility::default(),
        ))
        .id();
    commands.entity(anchor).with_children(|p| {
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
    let rig = spawn_knight(&mut commands, &knight, anchor, 0.75);
    commands.entity(anchor).insert((
        rig,
        KnightAnim::default(),
        ViewerKnight {
            input: pose(|k| {
                k.soul = 0.7;
                k.clock = 0.4;
            }),
            swing: None,
            facing: 1,
            tint: LinearRgba::rgb(0.05, 0.05, 0.05),
        },
    ));
}

/// The camera drifts slowly across the scene while the title is up.
fn drift_camera(
    time: Res<Time>,
    stage: Query<(), With<TitleStage>>,
    mut cam: Query<&mut Transform, With<MainCamera>>,
) {
    if stage.is_empty() {
        return;
    }
    let t = time.elapsed_secs();
    for mut c in &mut cam {
        *c = Transform::from_xyz(
            15.5 + 2.2 * (t * 0.11).sin(),
            8.4 + 0.5 * (t * 0.16).sin(),
            21.0 + 0.8 * (t * 0.07).sin(),
        )
        .looking_at(Vec3::new(20.0 + 1.5 * (t * 0.09).sin(), 8.9, 0.0), Vec3::Y);
    }
}

fn drift_mist(time: Res<Time>, mut q: Query<(&Mist, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (m, mut tr) in &mut q {
        tr.translation.x = m.base_x + 5.0 * (t * m.speed + m.phase).sin();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cliff_has_a_floor_at_the_knights_feet_and_a_drop_beyond() {
        let g = cliff_grid();
        let (i, j) = (KNIGHT_AT.x as i32, KNIGHT_AT.y as i32 - 1);
        assert_eq!(g.get(i, j), Tile::Solid, "the knight stands on rock");
        assert_eq!(g.get(i, j + 1), Tile::Empty, "with open air above");
        // The edge: the top row is rock up to x = 12, air beyond.
        assert_eq!(g.get(12, 5), Tile::Solid);
        assert_eq!(g.get(16, 5), Tile::Empty, "the drop");
        // Broken stones float past the edge.
        assert_eq!(g.get(20, 3), Tile::Solid);
    }

    #[test]
    fn only_the_title_screens_show_the_stage() {
        assert!(on_title(&Screen::Title));
        assert!(on_title(&Screen::Options(Back::Title)));
        assert!(on_title(&Screen::Controls(Back::Title)));
        assert!(!on_title(&Screen::Options(Back::Pause)));
        assert!(!on_title(&Screen::Playing));
        assert!(!on_title(&Screen::Paused));
        assert!(!on_title(&Screen::Ended));
    }

    #[test]
    fn the_stage_meshes_are_well_formed() {
        moon_mesh(13.0).validate().expect("moon");
        mist_mesh(70.0, 7.0).validate().expect("mist");
        // The moon is brighter in the middle than at the rim.
        let m = moon_mesh(10.0);
        let centre = m
            .pos
            .iter()
            .zip(&m.col)
            .filter(|(p, _)| (p[0] * p[0] + p[1] * p[1]).sqrt() < 1.0)
            .map(|(_, c)| c[0])
            .fold(0.0f32, f32::max);
        let rim = m
            .pos
            .iter()
            .zip(&m.col)
            .filter(|(p, _)| (p[0] * p[0] + p[1] * p[1]).sqrt() > 9.5)
            .map(|(_, c)| c[0])
            .fold(f32::MAX, f32::min);
        assert!(centre > rim + 0.3, "{centre} vs {rim}");
    }
}
