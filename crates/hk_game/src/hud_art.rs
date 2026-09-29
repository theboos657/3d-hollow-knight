//! The HUD's pictures, drawn in code from signed-distance shapes: a bone-white
//! bell-shaped mask for each point of health (full, empty and cracked), the
//! soul vessel (a glass flask, its liquid and a glow), and the red hurt
//! vignette. Nothing is loaded from disk.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const MASK_W: usize = 64;
pub const MASK_H: usize = 72;
pub const FLASK: usize = 96;

/// Handles to every picture the HUD uses.
#[derive(Resource, Clone)]
pub struct HudArt {
    pub mask_full: Handle<Image>,
    pub mask_empty: Handle<Image>,
    pub mask_cracked: Handle<Image>,
    pub flask: Handle<Image>,
    pub liquid: Handle<Image>,
    pub glow: Handle<Image>,
    pub hurt: Handle<Image>,
}

pub struct HudArtPlugin;

impl Plugin for HudArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, build_art);
    }
}

// ------------------------------------------------------------------- maths --

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Coverage (0..1) of a shape from its signed distance (negative inside),
/// with a one-pixel soft edge.
fn cover(d: f32) -> f32 {
    1.0 - smoothstep(-0.5, 0.5, d)
}

/// Distance to the segment `a`-`b` with a radius that tapers from `ra` to `rb`.
fn taper(p: Vec2, a: Vec2, b: Vec2, ra: f32, rb: f32) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    (p - (a + ab * t)).length() - (ra + (rb - ra) * t)
}

/// The mask's outline: a round dome that tapers to a chin, like a bell.
pub fn mask_sdf(x: f32, y: f32) -> f32 {
    let p = Vec2::new(x, y);
    let dome = (p - Vec2::new(32.0, 30.0)).length() - 27.0;
    let chin = taper(p, Vec2::new(32.0, 30.0), Vec2::new(32.0, 66.0), 26.0, 3.5);
    dome.min(chin)
}

/// An eye hole: a slanted ellipse.
fn eye_sdf(x: f32, y: f32, cx: f32, cy: f32, tilt: f32) -> f32 {
    let (dx, dy) = (x - cx, y - cy);
    let (s, c) = tilt.sin_cos();
    let (u, v) = (dx * c + dy * s, -dx * s + dy * c);
    // Approximate ellipse distance (semi-axes 7.5 x 4.5).
    (((u / 7.5).powi(2) + (v / 4.5).powi(2)).sqrt() - 1.0) * 4.5
}

fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Rasterises `f(x, y)` (pixel-centre coordinates, straight-alpha RGBA 0..1)
/// into an image.
fn raster(w: usize, h: usize, f: impl Fn(f32, f32) -> [f32; 4]) -> Image {
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = f(x as f32 + 0.5, y as f32 + 0.5);
            data.extend([to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), to_u8(c[3])]);
        }
    }
    Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

// ------------------------------------------------------------------- masks --

/// The full mask: bone with a soft gradient, a dark rim, two slanted eyes.
pub fn mask_pixel(x: f32, y: f32) -> [f32; 4] {
    let d = mask_sdf(x, y);
    let a = cover(d);
    if a <= 0.0 {
        return [0.0; 4];
    }
    // Shading: lighter top-left, darker toward the chin, a dark rim.
    let light = 1.0 - 0.22 * (y / MASK_H as f32) + 0.10 * (1.0 - x / MASK_W as f32);
    let rim = smoothstep(-3.5, -0.5, d); // 1 at the very edge
    let mut c = [0.96 * light, 0.94 * light, 0.86 * light];
    for k in c.iter_mut() {
        *k = *k * (1.0 - 0.45 * rim) + 0.10 * rim;
    }
    // The eye holes.
    let eye = cover(eye_sdf(x, y, 22.0, 32.0, 0.38)).max(cover(eye_sdf(x, y, 42.0, 32.0, -0.38)));
    for k in c.iter_mut() {
        *k *= 1.0 - eye;
    }
    // A small highlight on the brow.
    let hi = (1.0 - ((x - 22.0).powi(2) / 60.0 + (y - 13.0).powi(2) / 18.0).sqrt()).clamp(0.0, 1.0);
    for k in c.iter_mut() {
        *k = (*k + 0.10 * hi).min(1.0);
    }
    [c[0], c[1], c[2], a]
}

pub fn mask_empty_pixel(x: f32, y: f32) -> [f32; 4] {
    let d = mask_sdf(x, y);
    let a = cover(d);
    if a <= 0.0 {
        return [0.0; 4];
    }
    let ring = smoothstep(-2.6, -0.8, d);
    // A faint dark fill with a pale outline.
    [
        0.08 + 0.55 * ring,
        0.09 + 0.55 * ring,
        0.12 + 0.55 * ring,
        a * (0.55 + 0.30 * ring),
    ]
}

/// A full mask with a jagged crack across it and a red cast: the moment after
/// a hit.
pub fn mask_cracked_pixel(x: f32, y: f32) -> [f32; 4] {
    let mut c = mask_pixel(x, y);
    if c[3] <= 0.0 {
        return c;
    }
    // The crack: distance to a zigzag polyline from the brow to the chin.
    let pts = [
        Vec2::new(30.0, 6.0),
        Vec2::new(36.0, 20.0),
        Vec2::new(28.0, 32.0),
        Vec2::new(37.0, 44.0),
        Vec2::new(31.0, 58.0),
    ];
    let p = Vec2::new(x, y);
    let mut best = f32::MAX;
    for w in pts.windows(2) {
        let ab = w[1] - w[0];
        let t = ((p - w[0]).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        best = best.min((p - (w[0] + ab * t)).length());
    }
    let crack = 1.0 - smoothstep(0.8, 2.0, best);
    for v in c.iter_mut().take(3) {
        *v *= 1.0 - 0.9 * crack;
    }
    // Red tint.
    c[0] = (c[0] + 0.25).min(1.0);
    c[1] *= 0.6;
    c[2] *= 0.6;
    c
}

// ------------------------------------------------------------------- flask --

const FC: f32 = FLASK as f32 / 2.0;

/// The vessel: an iron rim, dark glass and a highlight.
pub fn flask_pixel(x: f32, y: f32) -> [f32; 4] {
    let r = ((x - FC).powi(2) + (y - FC).powi(2)).sqrt();
    let outer = cover(r - 46.0);
    if outer <= 0.0 {
        return [0.0; 4];
    }
    let glass = cover(r - 41.0);
    // Rim: iron, lighter at the top-left.
    let lit = 0.5 + 0.5 * (1.0 - ((x + y) / (2.0 * FLASK as f32)));
    let rim = [0.20 * lit + 0.10, 0.19 * lit + 0.09, 0.22 * lit + 0.11];
    let inner = [0.04, 0.07, 0.14];
    let mix = |a: f32, b: f32| a * (1.0 - glass) + b * glass;
    let mut c = [
        mix(rim[0], inner[0]),
        mix(rim[1], inner[1]),
        mix(rim[2], inner[2]),
    ];
    let alpha = outer * (1.0 * (1.0 - glass) + 0.86 * glass);
    // A curved highlight near the top-left of the glass.
    let hl = (1.0 - ((r - 32.0).abs() / 2.2)).clamp(0.0, 1.0)
        * smoothstep(-0.2, 0.5, -((x - FC) + (y - FC)) / 40.0)
        * smoothstep(0.3, -0.1, (y - FC) / 30.0);
    for k in c.iter_mut() {
        *k = (*k + 0.55 * hl).min(1.0);
    }
    [c[0], c[1], c[2], alpha]
}

/// The liquid: a full disc with a vertical gradient (cropped by the HUD to
/// show how much soul there is).
pub fn liquid_pixel(x: f32, y: f32) -> [f32; 4] {
    let r = ((x - FC).powi(2) + (y - FC).powi(2)).sqrt();
    let a = cover(r - 39.0);
    if a <= 0.0 {
        return [0.0; 4];
    }
    let t = ((y - (FC - 39.0)) / 78.0).clamp(0.0, 1.0);
    [0.78 - 0.50 * t, 0.95 - 0.42 * t, 1.0 - 0.10 * t, a]
}

/// A soft halo to pulse when there is enough soul to use.
pub fn glow_pixel(x: f32, y: f32) -> [f32; 4] {
    let r = ((x - FC).powi(2) + (y - FC).powi(2)).sqrt();
    let a = smoothstep(FC, 40.0, r) * (1.0 - smoothstep(46.0, FC, r)).max(0.0);
    let halo = (1.0 - ((r - 46.0).abs() / 12.0)).clamp(0.0, 1.0);
    [0.6, 0.9, 1.0, (halo * halo * 0.9).max(a * 0.0)]
}

/// The red hurt vignette: transparent in the middle, red at the edges.
pub fn hurt_pixel(x: f32, y: f32, w: f32, h: f32) -> [f32; 4] {
    let (u, v) = ((x / w - 0.5) * 2.0, (y / h - 0.5) * 2.0);
    let d = (u.abs().powf(2.4) + v.abs().powf(2.4)).powf(1.0 / 2.4);
    let a = smoothstep(0.45, 1.2, d);
    [0.85, 0.05, 0.05, a * a]
}

fn build_art(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut add = |img: Image| images.add(img);
    let art = HudArt {
        mask_full: add(raster(MASK_W, MASK_H, mask_pixel)),
        mask_empty: add(raster(MASK_W, MASK_H, mask_empty_pixel)),
        mask_cracked: add(raster(MASK_W, MASK_H, mask_cracked_pixel)),
        flask: add(raster(FLASK, FLASK, flask_pixel)),
        liquid: add(raster(FLASK, FLASK, liquid_pixel)),
        glow: add(raster(FLASK, FLASK, glow_pixel)),
        hurt: add(raster(128, 72, |x, y| hurt_pixel(x, y, 128.0, 72.0))),
    };
    commands.insert_resource(art);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(f: fn(f32, f32) -> [f32; 4], x: f32, y: f32) -> f32 {
        f(x, y)[3]
    }

    #[test]
    fn the_mask_is_a_bell_wide_at_the_top_and_pointed_at_the_chin() {
        // Solid in the dome, empty in the corners, narrow near the bottom.
        assert!(alpha_at(mask_pixel, 32.0, 20.0) > 0.99);
        assert!(alpha_at(mask_pixel, 2.0, 2.0) < 0.01);
        assert!(alpha_at(mask_pixel, 32.0, 60.0) > 0.9, "the chin");
        assert!(
            alpha_at(mask_pixel, 12.0, 60.0) < 0.01,
            "narrow at the bottom"
        );
        assert!(alpha_at(mask_pixel, 10.0, 30.0) > 0.9, "wide at the brow");
    }

    #[test]
    fn the_eye_holes_are_dark_and_the_bone_is_light() {
        let bone = mask_pixel(32.0, 22.0);
        let eye = mask_pixel(22.0, 32.0);
        assert!(bone[0] > 0.6, "bone {bone:?}");
        assert!(eye[0] < 0.2, "eye {eye:?}");
    }

    #[test]
    fn empty_and_full_masks_are_told_apart_by_brightness() {
        let full = mask_pixel(32.0, 22.0);
        let empty = mask_empty_pixel(32.0, 22.0);
        assert!(full[0] > empty[0] + 0.5);
    }

    #[test]
    fn the_cracked_mask_has_a_crack_and_a_red_cast() {
        // On the crack (a kink at (36, 20)) it is darker than the plain mask.
        let plain = mask_pixel(36.0, 20.0);
        let cracked = mask_cracked_pixel(36.0, 20.0);
        assert!(cracked[0] < plain[0] - 0.2, "the crack is dark");
        let off = mask_cracked_pixel(20.0, 20.0);
        assert!(off[0] > off[1] + 0.2, "and red elsewhere: {off:?}");
    }

    #[test]
    fn the_flask_is_a_disc_with_a_glass_centre_and_the_liquid_fills_it() {
        assert!(
            flask_pixel(FC, FC)[3] > 0.8 && flask_pixel(FC, FC)[3] < 0.95,
            "glass is translucent"
        );
        assert!(flask_pixel(1.0, 1.0)[3] < 0.01, "corner empty");
        assert!(flask_pixel(FC, 3.0)[3] > 0.99, "the rim is solid");
        assert!(liquid_pixel(FC, FC)[3] > 0.99);
        assert!(
            liquid_pixel(FC, 1.0)[3] < 0.01,
            "the liquid stays inside the glass"
        );
        // Lighter at the top than at the bottom.
        assert!(liquid_pixel(FC, FC - 30.0)[0] > liquid_pixel(FC, FC + 30.0)[0]);
    }

    #[test]
    fn the_hurt_vignette_is_clear_in_the_middle_and_red_at_the_corners() {
        assert!(hurt_pixel(64.0, 36.0, 128.0, 72.0)[3] < 0.01);
        let corner = hurt_pixel(1.0, 1.0, 128.0, 72.0);
        assert!(corner[3] > 0.5 && corner[0] > corner[1] * 4.0);
    }
}
