//! Image-based light. Each area gets a small procedural cubemap of its own
//! hall: a dim gradient from the dark floor to the vaulted dark above, with a
//! few bright soft windows and a glow behind the camera. Bevy filters it into
//! diffuse and specular ambient light, so wet stone, bronze and steel reflect
//! something and every surface is lit from the right direction instead of by
//! one flat ambient colour.
//!
//! The maths is pure ([`radiance`], [`face_dir`], [`f16_bits`]) and unit-tested;
//! [`build_cubemap`] packs it into a half-float cube image.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use hk_sim::world::room::Theme;

use super::quality::CurrentPlan;
use super::style::style;
use super::LookState;
use crate::scene::MainCamera;

/// Texels along a cube face (a power of two: Bevy filters from it).
pub const FACE: usize = 64;

/// How much of the flat ambient light remains once the environment map lights
/// the scene (the map supplies the rest, with direction).
pub const AMBIENT_WITH_IBL: f32 = 0.35;

/// The world direction a texel at `(u, v)` (each `0..1`, `v` running down the
/// image) looks along on `face` (+X, -X, +Y, -Y, +Z, -Z, the standard cube
/// layout).
pub fn face_dir(face: usize, u: f32, v: f32) -> Vec3 {
    let (s, t) = (2.0 * u - 1.0, 2.0 * v - 1.0);
    let d = match face {
        0 => Vec3::new(1.0, -t, -s),
        1 => Vec3::new(-1.0, -t, s),
        2 => Vec3::new(s, 1.0, t),
        3 => Vec3::new(s, -1.0, -t),
        4 => Vec3::new(s, -t, 1.0),
        _ => Vec3::new(-s, -t, -1.0),
    };
    d.normalize()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn linear(c: Color) -> Vec3 {
    let l = c.to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

/// The light arriving from direction `d` in an area's hall, in units where an
/// average sky is about 1.
pub fn radiance(theme: Theme, d: Vec3) -> Vec3 {
    let st = style(theme);
    let ambient = linear(st.ambient);
    let key = linear(st.key);
    let flame = linear(st.flame_light);
    let up = d.y;
    // Cool-ish light from the vault above, a warm-dim horizon, a dark floor.
    let above = ambient * 1.5;
    let horizon = ambient * 0.85 + flame * 0.12;
    let floor = ambient * 0.18 + flame * 0.05;
    let mut c = if up >= 0.0 {
        horizon.lerp(above, smoothstep(0.0, 0.85, up))
    } else {
        horizon.lerp(floor, smoothstep(0.0, -0.6, up))
    };
    // Soft windows in the back wall and one glow behind the viewer: what the
    // wet floor and the metal actually reflect.
    let emitters = [
        (Vec3::new(-0.45, 0.55, -0.70), key * 8.0, 0.09),
        (Vec3::new(0.50, 0.35, -0.80), flame * 5.0, 0.11),
        (Vec3::new(-0.25, 0.60, 0.75), key * 3.0, 0.16),
    ];
    for (dir, colour, width) in emitters {
        let cos = d.dot(dir.normalize());
        c += colour * ((cos - 1.0) / width).exp();
    }
    c
}

/// A float as IEEE half-precision bits (round to nearest; clamps to the
/// largest finite half, flushes what is too small to zero).
pub fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let e = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let m = b & 0x7f_ffff;
    if e >= 31 {
        return sign | 0x7bff;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (m | 0x80_0000) >> (1 - e);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    sign | (((e as u32) << 10) + ((m + 0x1000) >> 13)) as u16
}

/// The environment cube for `theme`, as a half-float image ready for Bevy's
/// filtering.
pub fn build_cubemap(theme: Theme) -> Image {
    let mut data = Vec::with_capacity(6 * FACE * FACE * 8);
    for face in 0..6 {
        for y in 0..FACE {
            for x in 0..FACE {
                let d = face_dir(
                    face,
                    (x as f32 + 0.5) / FACE as f32,
                    (y as f32 + 0.5) / FACE as f32,
                );
                let c = radiance(theme, d);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend(f16_bits(ch).to_le_bytes());
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: FACE as u32,
            height: FACE as u32,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    img
}

/// Every area's environment cube.
#[derive(Resource)]
pub struct Environments(pub HashMap<Theme, Handle<Image>>);

const THEMES: [Theme; 6] = [
    Theme::Sandbox,
    Theme::Ashen,
    Theme::Warrens,
    Theme::Cistern,
    Theme::Spire,
    Theme::Throne,
];

fn build_environments(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(Environments(
        THEMES
            .into_iter()
            .map(|t| (t, images.add(build_cubemap(t))))
            .collect(),
    ));
}

/// Keeps the camera's environment light (and the flat ambient it replaces) in
/// step with the tier and the area.
fn sync_environment(
    plan: Res<CurrentPlan>,
    look: Res<LookState>,
    envs: Res<Environments>,
    mut commands: Commands,
    cam: Query<(Entity, Option<&GeneratedEnvironmentMapLight>), With<MainCamera>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut applied: Local<Option<Theme>>,
) {
    let want = plan.0.ibl.then_some(look.theme);
    let scale = if plan.0.ibl { AMBIENT_WITH_IBL } else { 1.0 };
    let brightness = look.ambient_brightness * scale;
    if (ambient.brightness - brightness).abs() > 1e-3 {
        ambient.brightness = brightness;
    }
    for (e, has) in &cam {
        match (want, has.is_some()) {
            (Some(theme), have) => {
                if !have || *applied != Some(theme) {
                    // Bevy builds the filtered maps only for a camera without one,
                    // so a change of area starts from a clean camera.
                    commands.entity(e).remove::<EnvironmentMapLight>().insert(
                        GeneratedEnvironmentMapLight {
                            environment_map: envs.0[&theme].clone(),
                            intensity: look.ambient_brightness * 1.4,
                            rotation: Quat::IDENTITY,
                            affects_lightmapped_mesh_diffuse: false,
                        },
                    );
                    *applied = Some(theme);
                }
            }
            (None, true) => {
                commands
                    .entity(e)
                    .remove::<(GeneratedEnvironmentMapLight, EnvironmentMapLight)>();
                *applied = None;
            }
            (None, false) => {}
        }
    }
}

pub struct IblPlugin;

impl Plugin for IblPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build_environments)
            .add_systems(Update, sync_environment.after(super::quality::apply_plan));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_face_centres_look_along_their_axes_and_edges_meet() {
        let axes = [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::NEG_Z,
        ];
        for (face, axis) in axes.into_iter().enumerate() {
            let d = face_dir(face, 0.5, 0.5);
            assert!((d - axis).length() < 1e-5, "face {face}: {d:?}");
        }
        // The top edge of the +Z face is the bottom edge of the +Y face.
        let a = face_dir(4, 0.5, 0.0);
        let b = face_dir(2, 0.5, 1.0);
        assert!((a - b).length() < 1e-5, "{a:?} vs {b:?}");
        // +Z's right edge meets +X's left edge.
        let a = face_dir(4, 1.0, 0.5);
        let b = face_dir(0, 0.0, 0.5);
        assert!((a - b).length() < 1e-5, "{a:?} vs {b:?}");
    }

    #[test]
    fn half_floats_round_trip_the_common_values() {
        assert_eq!(f16_bits(0.0), 0x0000);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(2.0), 0x4000);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        assert_eq!(f16_bits(1.0e9), 0x7bff, "clamps to the largest half");
        assert_eq!(f16_bits(1.0e-9), 0x0000, "flushes the negligible");
        // 1.5 is exact; 0.1 lands within half precision.
        assert_eq!(f16_bits(1.5), 0x3e00);
        assert_eq!(f16_bits(0.1), 0x2e66);
    }

    #[test]
    fn every_hall_is_lit_from_above_and_has_bright_windows() {
        for t in THEMES {
            let luma = |d: Vec3| {
                let c = radiance(t, d);
                0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
            };
            let (up, down) = (luma(Vec3::Y), luma(Vec3::NEG_Y));
            assert!(
                up > down * 2.0,
                "{t:?}: the vault is brighter than the floor"
            );
            let window = luma(Vec3::new(-0.45, 0.55, -0.70));
            assert!(window > up * 2.0, "{t:?}: the windows outshine the vault");
            for d in [Vec3::X, Vec3::NEG_Z, Vec3::Z, Vec3::NEG_Y] {
                let c = radiance(t, d);
                assert!(c.min_element() >= 0.0 && c.is_finite());
            }
        }
    }

    #[test]
    fn the_cube_has_six_full_faces_of_half_float_texels() {
        let img = build_cubemap(Theme::Cistern);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers, 6);
        assert_eq!(img.texture_descriptor.size.width as usize, FACE);
        assert!(FACE.is_power_of_two());
        assert_eq!(img.data.as_ref().map(Vec::len), Some(6 * FACE * FACE * 8));
    }
}
