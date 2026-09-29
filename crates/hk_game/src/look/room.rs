//! Building the current room's scenery when it is entered.

use bevy::light::{FogVolume, NotShadowCaster};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use hk_sim::world::room::{RoomEntered, RoomLibrary};

use super::decor::build_decor;
use super::kits::{build_kit, WALL_Z};
use super::level::{build_level, wall_mesh};
use super::pbr::{Kind, Materials};
use super::props::{brazier_meshes, pick_spots, Flame};
use super::quality::CurrentPlan;
use super::style::{style, LookStyle};
use super::{KeyLight, LookState, Mote, RimLight, RoomVisual};
use crate::rig::meshkit::hash3;
use crate::scene::MainCamera;

/// A stable seed from a room's id, so a room always looks the same.
pub fn room_seed(id: &str) -> u32 {
    id.bytes().fold(0x811C_9DC5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

/// Texture repeats per world unit on the pillars, arches and near wall behind
/// the play lane.
const KIT_UV: f32 = 0.2;

/// The materials a room's architecture is made of: real relief, roughness and
/// occlusion maps tinted by the area's palette.
pub struct LevelMats {
    pub stone: Handle<StandardMaterial>,
    pub cap: Handle<StandardMaterial>,
    pub plank: Handle<StandardMaterial>,
    pub wall: Handle<StandardMaterial>,
    /// Pillars and arches behind the play lane (no parallax: it is far and
    /// curved).
    pub kit: Handle<StandardMaterial>,
    pub fungus: Handle<StandardMaterial>,
    pub roots: Handle<StandardMaterial>,
    pub bone: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub bronze: Handle<StandardMaterial>,
    pub cloth: Handle<StandardMaterial>,
    /// Loose rubble and stalactites.
    pub rubble: Handle<StandardMaterial>,
}

/// One PBR surface: `kind`'s maps tinted, roughened and coated as asked.
pub fn surface(
    pbr: &Materials,
    kind: Kind,
    tint: Color,
    rough: f32,
    coat: f32,
    parallax: bool,
) -> StandardMaterial {
    let mut m = pbr.get(kind).material();
    m.base_color = tint;
    m.perceptual_roughness = rough;
    m.clearcoat = coat;
    m.clearcoat_perceptual_roughness = 0.3;
    if !parallax {
        m.depth_map = None;
    }
    m
}

/// Builds the room materials for `st`. `parallax` adds depth-shifted relief to
/// the stone and the wall (a tier feature).
pub fn level_mats(
    mats: &mut Assets<StandardMaterial>,
    pbr: &Materials,
    st: &LookStyle,
    parallax: bool,
) -> LevelMats {
    // The maps are mid-grey; these gains bring each surface to the value the
    // palette was tuned for (the wall stays dark so the knight stands out).
    let tone = |c: Color, k: f32| {
        let l = c.to_linear();
        Color::linear_rgb(l.red * k, l.green * k, l.blue * k)
    };
    let cap_kind = if st.moss { Kind::Moss } else { Kind::Rock };
    // The moss map is already green: wash the palette's tint out toward
    // neutral so the two do not stack into neon.
    let cap_tint = if st.moss {
        tone(st.cap.mix(&Color::srgb(0.6, 0.6, 0.6), 0.55), 0.85)
    } else {
        tone(st.cap, 0.85)
    };
    let kit_tint = tone(Color::srgb(0.5, 0.5, 0.5).mix(&st.stone, 0.6), 0.6);
    let mut fungus = surface(
        pbr,
        Kind::Flesh,
        tone(Color::srgb(0.6, 0.9, 0.75).mix(&st.cap, 0.3), 0.7),
        0.6,
        0.5,
        false,
    );
    // Waxy flesh that light seeps a little way into.
    fungus.diffuse_transmission = 0.3;
    let mut cloth = surface(
        pbr,
        Kind::Cloth,
        Color::srgb(0.9, 0.9, 0.9),
        1.0,
        0.0,
        false,
    );
    cloth.cull_mode = None;
    cloth.double_sided = true;
    LevelMats {
        stone: mats.add(surface(
            pbr,
            Kind::Rock,
            tone(st.stone, 0.8),
            st.roughness,
            st.wet,
            parallax,
        )),
        cap: mats.add(surface(
            pbr,
            cap_kind,
            cap_tint,
            st.roughness * 0.9,
            st.wet,
            parallax,
        )),
        plank: mats.add(surface(
            pbr,
            Kind::Wood,
            tone(st.one_way, 0.9),
            0.9,
            0.0,
            parallax,
        )),
        wall: mats.add(surface(
            pbr,
            Kind::Masonry,
            tone(st.wall, 0.7),
            1.0,
            st.wet * 0.5,
            parallax,
        )),
        kit: mats.add(surface(pbr, Kind::Masonry, kit_tint, 1.0, 0.0, false)),
        fungus: mats.add(fungus),
        roots: mats.add(surface(
            pbr,
            Kind::Wood,
            tone(Color::srgb(0.55, 0.65, 0.42), 0.6),
            1.0,
            0.0,
            false,
        )),
        bone: mats.add(surface(
            pbr,
            Kind::Bone,
            tone(Color::srgb(0.85, 0.66, 0.60), 0.6),
            0.85,
            0.2,
            false,
        )),
        metal: mats.add(surface(
            pbr,
            Kind::Iron,
            tone(Color::srgb(0.65, 0.68, 0.75), 0.75),
            1.0,
            0.0,
            false,
        )),
        bronze: mats.add(surface(
            pbr,
            Kind::Bronze,
            tone(Color::srgb(0.8, 0.72, 0.6), 0.8),
            0.9,
            0.3,
            false,
        )),
        cloth: mats.add(cloth),
        rubble: mats.add(surface(
            pbr,
            Kind::Rock,
            tone(st.stone, 0.9),
            st.roughness,
            st.wet,
            false,
        )),
    }
}

/// Spawns a level's stone blocks, floor lips and one-way planks, tagged with
/// `marker`.
pub fn spawn_level<M: Bundle + Clone>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    lm: &LevelMats,
    grid: &hk_sim::world::grid::TileGrid,
    seed: u32,
    marker: M,
) {
    let geo = build_level(grid, seed);
    for (first, m) in geo.chunks {
        commands.spawn((
            marker.clone(),
            Name::new(format!("stone {first}")),
            // The rock does not cast shadows: a whole ceiling's shadow lands on the
            // wall as a heavy black bar. Actors and props still ground themselves.
            NotShadowCaster,
            Mesh3d(meshes.add(m.to_mesh_pbr())),
            MeshMaterial3d(lm.stone.clone()),
            Transform::IDENTITY,
        ));
    }
    commands.spawn((
        marker.clone(),
        NotShadowCaster,
        Mesh3d(meshes.add(geo.caps.to_mesh_pbr())),
        MeshMaterial3d(lm.cap.clone()),
        Transform::IDENTITY,
    ));
    commands.spawn((
        marker,
        // Planks would throw floating bars of shadow on the far wall.
        NotShadowCaster,
        Mesh3d(meshes.add(geo.planks.to_mesh_pbr())),
        MeshMaterial3d(lm.plank.clone()),
        Transform::IDENTITY,
    ));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn rebuild_room(
    mut commands: Commands,
    mut entered: MessageReader<RoomEntered>,
    library: Res<RoomLibrary>,
    old: Query<Entity, With<RoomVisual>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pbr: Res<Materials>,
    plan: Res<CurrentPlan>,
    mut look: ResMut<LookState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut cam: Query<(&mut DistanceFog, &mut ColorGrading), With<MainCamera>>,
    mut key: Query<&mut DirectionalLight, With<KeyLight>>,
    mut rim: Query<&mut DirectionalLight, (With<RimLight>, Without<KeyLight>)>,
) {
    let Some(ev) = entered.read().last().cloned() else {
        return;
    };
    let Some(def) = library.get(&ev.id) else {
        return;
    };
    for e in &old {
        commands.entity(e).despawn();
    }
    let st = style(def.theme);
    let seed = room_seed(&def.id);

    ambient.color = st.ambient;
    look.theme = def.theme;
    look.ambient_brightness = st.ambient_brightness;
    clear.0 = st.fog;
    for (mut fog, mut grading) in &mut cam {
        fog.color = st.fog;
        fog.falloff = bevy::pbr::FogFalloff::Linear {
            start: 22.0,
            end: 80.0,
        };
        grading.global.temperature = st.temperature;
        grading.global.post_saturation = st.saturation;
        grading.midtones.contrast = st.contrast;
    }
    for mut l in &mut key {
        l.color = st.key;
        l.illuminance = st.key_lux;
    }
    for mut l in &mut rim {
        l.color = st.rim;
    }

    let grid = def.grid();
    let lm = level_mats(&mut mats, &pbr, &st, plan.0.parallax);
    spawn_level(&mut commands, &mut meshes, &lm, &grid, seed, RoomVisual);
    commands.spawn((
        RoomVisual,
        NotShadowCaster,
        Mesh3d(
            meshes.add(
                wall_mesh(grid.width() as f32, grid.height() as f32, WALL_Z, seed).to_mesh_pbr(),
            ),
        ),
        MeshMaterial3d(lm.wall.clone()),
        Transform::IDENTITY,
    ));

    // The architecture in front of the wall: stone, glowing panes and light shafts.
    let kit = build_kit(def.theme, grid.width() as f32, grid.height() as f32, seed);
    let kit_glow = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let kit_beam = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    // Each family of solid parts in its own material, mapped at its own scale.
    for (family, mesh) in kit.solids() {
        let (material, scale) = match family {
            "dark" => (&lm.kit, KIT_UV),
            "fungus" => (&lm.fungus, 0.30),
            "roots" => (&lm.roots, 0.45),
            "bone" => (&lm.bone, 0.22),
            "metal" => (&lm.metal, 0.40),
            "bronze" => (&lm.bronze, 0.22),
            _ => (&lm.cloth, 0.55),
        };
        if mesh.vertex_count() == 0 {
            continue;
        }
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(mesh.box_mapped(scale, Vec2::ZERO).to_mesh_pbr())),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
        ));
    }
    if kit.glow.vertex_count() > 0 {
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(kit.glow.to_mesh())),
            MeshMaterial3d(kit_glow),
            Transform::IDENTITY,
        ));
    }
    if kit.beams.vertex_count() > 0 {
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(kit.beams.to_mesh())),
            MeshMaterial3d(kit_beam),
            Transform::IDENTITY,
        ));
    }

    // The air: a volume of haze over the whole hall. Only tiers with
    // volumetrics see it (the camera has to ask for it).
    let (w, h) = (grid.width() as f32, grid.height() as f32);
    commands.spawn((
        RoomVisual,
        FogVolume {
            fog_color: st.rim.mix(&st.key, 0.5),
            density_factor: st.haze,
            absorption: 0.2,
            scattering: 0.4,
            scattering_asymmetry: 0.3,
            light_tint: Color::WHITE,
            light_intensity: 1.0,
            ..default()
        },
        Transform::from_xyz(w / 2.0, h / 2.0, -2.0).with_scale(Vec3::new(w + 60.0, h + 40.0, 24.0)),
    ));

    // Braziers: iron stands with a flame and a flickering pool of light.
    let (stand, flame) = brazier_meshes();
    let stand = meshes.add(stand.box_mapped(0.5, Vec2::ZERO).to_mesh_pbr());
    let flame = meshes.add(flame.to_mesh());
    let iron = lm.metal.clone();
    let fire = mats.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.12, 0.05),
        emissive: st.flame,
        ..default()
    });
    for (i, j) in pick_spots(&grid, seed, 5) {
        let at = Vec3::new(i as f32 + 0.5, j as f32 + 1.0, 0.6);
        let phase = hash3(seed, i, j, 31) * std::f32::consts::TAU;
        commands
            .spawn((
                RoomVisual,
                Transform::from_translation(at),
                Visibility::default(),
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(stand.clone()),
                    MeshMaterial3d(iron.clone()),
                    Transform::IDENTITY,
                ));
                p.spawn((
                    NotShadowCaster,
                    Mesh3d(flame.clone()),
                    MeshMaterial3d(fire.clone()),
                    Transform::IDENTITY,
                ));
                p.spawn((
                    Flame {
                        base: 420_000.0,
                        phase,
                    },
                    PointLight {
                        intensity: 420_000.0,
                        range: 13.0,
                        color: st.flame_light,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 1.1, 0.5),
                ));
            });
    }

    // Growth (blades of grass, moss, fronds), rubble, stalactites and chains.
    let decor = build_decor(&grid, def.theme, seed);
    if decor.growth.vertex_count() > 0 {
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(decor.growth.to_mesh())),
            MeshMaterial3d(mats.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.85,
                // Thin blades are lit from both sides.
                cull_mode: None,
                double_sided: true,
                ..default()
            })),
            Transform::IDENTITY,
        ));
    }
    for (m, scale) in [(decor.rubble, 0.5), (decor.hangers, 0.4)] {
        if m.vertex_count() == 0 {
            continue;
        }
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mesh3d(meshes.add(m.box_mapped(scale, Vec2::ZERO).to_mesh_pbr())),
            MeshMaterial3d(lm.rubble.clone()),
            Transform::IDENTITY,
        ));
    }

    // Puddles and drips, in proportion to how wet the area is.
    super::wet::spawn_wet(&mut commands, &mut meshes, &mut mats, &grid, seed, st.wet);

    // Motes drifting through the air.
    let mote_mesh = meshes.add(Sphere::new(0.07));
    let mote = mats.add(StandardMaterial {
        base_color: st.mote,
        emissive: st.mote_emissive,
        unlit: true,
        ..default()
    });
    let count = ((grid.width() as f32 * grid.height() as f32) / 40.0).clamp(24.0, 70.0) as i32;
    for k in 0..count {
        let x = hash3(seed, k, 0, 41) * grid.width() as f32;
        let span = 4.0 + hash3(seed, k, 1, 41) * 6.0;
        let y = 1.5 + hash3(seed, k, 3, 41) * (grid.height() as f32 - span - 1.0).max(1.0);
        let z = -0.5 - hash3(seed, k, 2, 41) * 2.6;
        commands.spawn((
            RoomVisual,
            NotShadowCaster,
            Mote {
                base: Vec3::new(x, y, z),
                phase: hash3(seed, k, 4, 41),
                rise: 0.18 + 0.3 * hash3(seed, k, 5, 41),
                span,
            },
            Mesh3d(mote_mesh.clone()),
            MeshMaterial3d(mote.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_seeds_are_stable_and_differ() {
        assert_eq!(room_seed("A1"), room_seed("A1"));
        assert_ne!(room_seed("A1"), room_seed("A2"));
    }
}
