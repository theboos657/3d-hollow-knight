//! Physically-based material textures, generated in code.
//!
//! Every surface in the game gets a full set: a colour map, a tangent-space
//! **normal map** (so light rakes across pits, grain and seams), an **ORM** map
//! (occlusion / roughness / metallic, glTF layout), a **height** map (for
//! parallax) and, for glowing veins, an **emissive** mask. They are built from
//! tileable noise (value noise, cellular / Worley noise, ridged and warped
//! fractal sums), so they repeat seamlessly, and each comes with a full chain
//! of mipmaps (so far-away rock does not shimmer).
//!
//! Generation is pure maths returning bytes ([`PbrMaps`]), unit-tested; uploading
//! to the GPU is a separate step ([`upload`]). Sets are generated on several
//! threads at startup.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::ParallaxMappingMethod;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::rig::meshkit::hash3;

// ------------------------------------------------------------------- noise --

fn smooth(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Random value in `[0, 1)` at a lattice point.
fn lattice(seed: u32, x: i32, y: i32) -> f32 {
    hash3(seed, x, y, 0)
}

/// Value noise that repeats every `pu` cells in u and `pv` in v (`u`, `v` in
/// `[0, 1)` cover one repeat).
pub fn noise2(seed: u32, u: f32, v: f32, pu: i32, pv: i32) -> f32 {
    let (x, y) = (u * pu as f32, v * pv as f32);
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (smooth(x - ix as f32), smooth(y - iy as f32));
    let p = |dx: i32, dy: i32| lattice(seed, (ix + dx).rem_euclid(pu), (iy + dy).rem_euclid(pv));
    let top = lerp(p(0, 0), p(1, 0), fx);
    let bot = lerp(p(0, 1), p(1, 1), fx);
    lerp(top, bot, fy)
}

/// Fractal sum of value noise: `octaves` layers, each `lacunarity` times finer
/// and `gain` times weaker. Result in about `[0, 1]`.
pub fn fbm(seed: u32, u: f32, v: f32, base: i32, octaves: u32, gain: f32) -> f32 {
    let (mut sum, mut amp, mut norm, mut period) = (0.0, 1.0, 0.0, base);
    for o in 0..octaves {
        sum += amp * noise2(seed.wrapping_add(o * 977), u, v, period, period);
        norm += amp;
        amp *= gain;
        period *= 2;
    }
    sum / norm
}

/// Like [`fbm`] but stretched: `pu` x `pv` cells at the base octave (a grain).
pub fn fbm_aniso(seed: u32, u: f32, v: f32, pu: i32, pv: i32, octaves: u32, gain: f32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    let (mut a, mut b) = (pu, pv);
    for o in 0..octaves {
        sum += amp * noise2(seed.wrapping_add(o * 1013), u, v, a, b);
        norm += amp;
        amp *= gain;
        a *= 2;
        b *= 2;
    }
    sum / norm
}

/// Ridged noise: sharp creases where the fractal crosses 0.5, in `[0, 1]`
/// (1 on the crease).
pub fn ridged(seed: u32, u: f32, v: f32, base: i32, octaves: u32) -> f32 {
    let n = fbm(seed, u, v, base, octaves, 0.5);
    1.0 - (2.0 * n - 1.0).abs()
}

/// Cellular (Worley) noise repeating every `period` cells: the distance to the
/// nearest and second-nearest feature point (in cell units), and a random
/// value for the nearest cell.
pub fn worley(seed: u32, u: f32, v: f32, period: i32) -> (f32, f32, f32) {
    let (x, y) = (u * period as f32, v * period as f32);
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (8.0f32, 8.0f32, 0.0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (ix + dx, iy + dy);
            let (wx, wy) = (cx.rem_euclid(period), cy.rem_euclid(period));
            let jx = hash3(seed, wx, wy, 1);
            let jy = hash3(seed, wx, wy, 2);
            let (px, py) = (cx as f32 + jx, cy as f32 + jy);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = hash3(seed, wx, wy, 3);
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id)
}

/// Domain-warped coordinates: shifts `(u, v)` by a smooth noise, for organic
/// wobble.
pub fn warp(seed: u32, u: f32, v: f32, amount: f32, base: i32) -> (f32, f32) {
    let du = fbm(seed ^ 0xA5A5, u, v, base, 3, 0.5) - 0.5;
    let dv = fbm(seed ^ 0x5A5A, u, v, base, 3, 0.5) - 0.5;
    (
        (u + du * amount).rem_euclid(1.0),
        (v + dv * amount).rem_euclid(1.0),
    )
}

// ------------------------------------------------------------------- maps --

/// What a texel says about its surface.
#[derive(Clone, Copy, Debug)]
pub struct Texel {
    /// Surface height, 0 (deep) to 1 (high).
    pub height: f32,
    pub albedo: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    /// Extra occlusion on top of the height-derived cavity darkening, 0..1.
    pub occlusion: f32,
    /// Glow mask (veins, cracks, runes), 0..1.
    pub glow: f32,
}

impl Default for Texel {
    fn default() -> Self {
        Self {
            height: 0.5,
            albedo: [0.5; 3],
            roughness: 0.8,
            metallic: 0.0,
            occlusion: 1.0,
            glow: 0.0,
        }
    }
}

/// The finished byte maps of one material (RGBA8, `size` x `size`).
pub struct PbrMaps {
    pub size: usize,
    /// sRGB colour.
    pub albedo: Vec<u8>,
    /// Tangent-space normals, OpenGL convention (green = up).
    pub normal: Vec<u8>,
    /// R = occlusion, G = roughness, B = metallic.
    pub orm: Vec<u8>,
    /// Height in R (also G, B).
    pub height: Vec<u8>,
    /// Glow mask (white = glows).
    pub emissive: Vec<u8>,
}

fn srgb_encode(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn byte(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Runs `f(u, v)` for every texel, on all cores.
fn evaluate(size: usize, f: &(impl Fn(f32, f32) -> Texel + Sync)) -> Vec<Texel> {
    let mut out = vec![Texel::default(); size * size];
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    let rows_per = size.div_ceil(threads);
    std::thread::scope(|s| {
        for (chunk_i, chunk) in out.chunks_mut(rows_per * size).enumerate() {
            s.spawn(move || {
                for (k, t) in chunk.iter_mut().enumerate() {
                    let (x, y) = (k % size, chunk_i * rows_per + k / size);
                    *t = f(
                        (x as f32 + 0.5) / size as f32,
                        (y as f32 + 0.5) / size as f32,
                    );
                }
            });
        }
    });
    out
}

/// Box-blurs the height field (wrapping) to find cavities.
fn blur(h: &[f32], size: usize, radius: usize) -> Vec<f32> {
    let n = size as i32;
    let r = radius as i32;
    let mut tmp = vec![0.0; h.len()];
    let mut out = vec![0.0; h.len()];
    let inv = 1.0 / (2 * r + 1) as f32;
    for y in 0..n {
        for x in 0..n {
            let mut s = 0.0;
            for d in -r..=r {
                s += h[(y * n + (x + d).rem_euclid(n)) as usize];
            }
            tmp[(y * n + x) as usize] = s * inv;
        }
    }
    for y in 0..n {
        for x in 0..n {
            let mut s = 0.0;
            for d in -r..=r {
                s += tmp[(((y + d).rem_euclid(n)) * n + x) as usize];
            }
            out[(y * n + x) as usize] = s * inv;
        }
    }
    out
}

/// Turns a texel field into byte maps. `bump` scales the normal map's
/// steepness.
pub fn bake(size: usize, texels: &[Texel], bump: f32) -> PbrMaps {
    let n = size as i32;
    let h: Vec<f32> = texels.iter().map(|t| t.height).collect();
    let soft = blur(&h, size, (size / 48).max(2));
    let at = |x: i32, y: i32| h[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize];
    let mut albedo = Vec::with_capacity(size * size * 4);
    let mut normal = Vec::with_capacity(size * size * 4);
    let mut orm = Vec::with_capacity(size * size * 4);
    let mut height = Vec::with_capacity(size * size * 4);
    let mut emissive = Vec::with_capacity(size * size * 4);
    for y in 0..n {
        for x in 0..n {
            let t = &texels[(y * n + x) as usize];
            // Slope from central differences (wrapping); image y points down, the
            // normal map's green points up.
            let dx = (at(x + 1, y) - at(x - 1, y)) * 0.5;
            let dy = (at(x, y + 1) - at(x, y - 1)) * 0.5;
            let s = bump * size as f32 / 128.0;
            let nx = -dx * s;
            let ny = dy * s;
            let inv = 1.0 / (nx * nx + ny * ny + 1.0).sqrt();
            normal.extend([
                byte(nx * inv * 0.5 + 0.5),
                byte(ny * inv * 0.5 + 0.5),
                byte(inv * 0.5 + 0.5),
                255,
            ]);
            // Cavities (lower than their surroundings) are occluded.
            let cav = (soft[(y * n + x) as usize] - t.height).max(0.0);
            let ao = (1.0 - cav * 4.5).clamp(0.25, 1.0) * t.occlusion;
            albedo.extend([
                byte(srgb_encode(t.albedo[0])),
                byte(srgb_encode(t.albedo[1])),
                byte(srgb_encode(t.albedo[2])),
                255,
            ]);
            orm.extend([byte(ao), byte(t.roughness), byte(t.metallic), 255]);
            let hv = byte(t.height);
            height.extend([hv, hv, hv, 255]);
            let g = byte(t.glow);
            emissive.extend([g, g, g, 255]);
        }
    }
    PbrMaps {
        size,
        albedo,
        normal,
        orm,
        height,
        emissive,
    }
}

// ------------------------------------------------------------- mipmapping --

/// Appends the full mip chain of an RGBA8 image (`data` at `size` x `size`).
/// `srgb` averages in linear light; `normals` renormalises after averaging.
pub fn with_mips(data: &[u8], size: usize, srgb: bool, normals: bool) -> (Vec<u8>, u32) {
    let mut all = data.to_vec();
    let mut cur = data.to_vec();
    let mut s = size;
    let mut levels = 1;
    let to_lin = |b: u8| -> f32 {
        let c = b as f32 / 255.0;
        if srgb {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        } else {
            c
        }
    };
    while s > 1 {
        let ns = s / 2;
        let mut next = vec![0u8; ns * ns * 4];
        for y in 0..ns {
            for x in 0..ns {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let i = ((2 * y + dy) * s + 2 * x + dx) * 4;
                    for c in 0..3 {
                        acc[c] += if normals {
                            cur[i + c] as f32 / 255.0 * 2.0 - 1.0
                        } else {
                            to_lin(cur[i + c])
                        };
                    }
                    acc[3] += cur[i + 3] as f32 / 255.0;
                }
                let o = (y * ns + x) * 4;
                if normals {
                    let v = Vec3::new(acc[0], acc[1], acc[2]);
                    let v = if v.length() > 1e-4 {
                        v.normalize()
                    } else {
                        Vec3::Z
                    };
                    for c in 0..3 {
                        next[o + c] = byte([v.x, v.y, v.z][c] * 0.5 + 0.5);
                    }
                } else {
                    for c in 0..3 {
                        let lin = acc[c] * 0.25;
                        next[o + c] = byte(if srgb { srgb_encode(lin) } else { lin });
                    }
                }
                next[o + 3] = byte(acc[3] * 0.25);
            }
        }
        all.extend_from_slice(&next);
        cur = next;
        s = ns;
        levels += 1;
    }
    (all, levels)
}

fn image(data: Vec<u8>, size: usize, levels: u32, format: TextureFormat) -> Image {
    // `Image::new` insists on exactly one level of data, so build it empty and
    // attach the whole mip chain.
    let mut img = Image::new_uninit(
        Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    img
}

/// The GPU handles of one material's maps.
#[derive(Clone)]
pub struct PbrImages {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub orm: Handle<Image>,
    pub height: Handle<Image>,
    pub emissive: Handle<Image>,
}

pub fn upload(m: &PbrMaps, images: &mut Assets<Image>) -> PbrImages {
    let mut add = |data: &[u8], srgb: bool, normals: bool| {
        let (all, levels) = with_mips(data, m.size, srgb, normals);
        images.add(image(
            all,
            m.size,
            levels,
            if srgb {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            },
        ))
    };
    PbrImages {
        albedo: add(&m.albedo, true, false),
        normal: add(&m.normal, false, true),
        orm: add(&m.orm, false, false),
        height: add(&m.height, false, false),
        emissive: add(&m.emissive, false, false),
    }
}

impl PbrImages {
    /// A standard material using this set (the caller sets colours and the
    /// like on top).
    pub fn material(&self) -> StandardMaterial {
        StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(self.albedo.clone()),
            normal_map_texture: Some(self.normal.clone()),
            occlusion_texture: Some(self.orm.clone()),
            metallic_roughness_texture: Some(self.orm.clone()),
            // The maps carry the values; the scalars multiply them.
            perceptual_roughness: 1.0,
            metallic: 1.0,
            // Height gives parallax depth (needs tangents: build the mesh with
            // `to_mesh_pbr`); the glow mask is dark until the caller sets an
            // `emissive` colour to multiply it.
            depth_map: Some(self.height.clone()),
            parallax_depth_scale: 0.035,
            parallax_mapping_method: ParallaxMappingMethod::Occlusion,
            max_parallax_layer_count: 8.0,
            emissive_texture: Some(self.emissive.clone()),
            ..default()
        }
    }
}

// ----------------------------------------------------------------- kinds --

/// The materials the game is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Rock,
    Masonry,
    Moss,
    Wood,
    Bronze,
    Iron,
    Cloth,
    Burlap,
    Bone,
    Chitin,
    Flesh,
    Leather,
    Steel,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Rock,
        Kind::Masonry,
        Kind::Moss,
        Kind::Wood,
        Kind::Bronze,
        Kind::Iron,
        Kind::Cloth,
        Kind::Burlap,
        Kind::Bone,
        Kind::Chitin,
        Kind::Flesh,
        Kind::Leather,
        Kind::Steel,
    ];

    /// How steep this material's normal map should be.
    fn bump(self) -> f32 {
        match self {
            Kind::Rock | Kind::Masonry => 3.2,
            Kind::Moss => 2.6,
            Kind::Wood => 1.8,
            Kind::Bronze | Kind::Iron | Kind::Steel => 1.6,
            Kind::Cloth | Kind::Burlap => 2.4,
            Kind::Bone => 1.4,
            Kind::Chitin => 3.0,
            Kind::Flesh => 1.8,
            Kind::Leather => 2.2,
        }
    }

    fn seed(self) -> u32 {
        0x1000 + self as u32 * 7919
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// The texel of material `kind` at `(u, v)`.
pub fn texel(kind: Kind, u: f32, v: f32) -> Texel {
    let s = kind.seed();
    match kind {
        Kind::Rock => {
            // Layered stone: broad shapes, fine grain, sharp fractures, faint strata.
            let (wu, wv) = warp(s, u, v, 0.06, 3);
            let broad = fbm(s, wu, wv, 6, 5, 0.5);
            let grain = fbm(s + 1, u, v, 40, 4, 0.55);
            let crease = ridged(s + 2, wu, wv, 8, 4);
            let (f1, f2, id) = worley(s + 3, wu, wv, 11);
            let crack = 1.0 - smoothstep(0.0, 0.11, f2 - f1);
            let strata = (wv * 22.0 * std::f32::consts::TAU + broad * 9.0).sin() * 0.5 + 0.5;
            let pit = 1.0 - smoothstep(0.0, 0.18, worley(s + 4, u, v, 64).0);
            let h = 0.42 * broad + 0.16 * grain + 0.20 * crease.powf(3.0) + 0.05 * strata
                - 0.30 * crack
                - 0.10 * pit
                + 0.06 * id;
            let tone = 0.50 + 0.42 * broad + 0.10 * grain;
            let lichen = smoothstep(0.62, 0.78, fbm(s + 5, u, v, 9, 4, 0.55)) * 0.35;
            let warm = fbm(s + 6, u, v, 3, 3, 0.5);
            let mut c = mix3([0.62, 0.62, 0.64], [0.72, 0.66, 0.58], warm);
            c = scale3(c, tone * (0.85 + 0.3 * grain));
            c = mix3(c, [0.42, 0.50, 0.28], lichen);
            c = scale3(c, 1.0 - 0.55 * crack - 0.25 * pit);
            let wet = smoothstep(0.30, 0.0, h - 0.30).max(0.0);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: (0.90 - 0.28 * wet - 0.10 * grain).clamp(0.4, 1.0),
                metallic: 0.0,
                occlusion: 1.0 - 0.4 * crack,
                glow: 0.0,
            }
        }
        Kind::Masonry => {
            // Cut blocks in courses, with weathered faces and dark mortar joints.
            let rows = 6.0;
            let (cu, cv) = (u, v * rows);
            let row = cv.floor();
            let off = if (row as i32) % 2 == 0 { 0.0 } else { 0.5 };
            let bu = (cu * 3.0 + off).fract();
            let bv = cv.fract();
            let edge = bu.min(1.0 - bu).min(bv * 0.5).min((1.0 - bv) * 0.5) * 2.0;
            let mortar = 1.0 - smoothstep(0.02, 0.06, edge);
            let block_id = hash3(
                s,
                ((cu * 3.0 + off).floor() as i32).rem_euclid(3),
                row as i32,
                9,
            );
            let face = fbm(s, u, v, 10, 5, 0.55);
            let pits = 1.0 - smoothstep(0.0, 0.2, worley(s + 4, u, v, 36).0);
            let bevel = smoothstep(0.0, 0.10, edge);
            let h = (0.70 + 0.20 * face - 0.12 * pits) * bevel - 0.55 * mortar + 0.05 * block_id;
            let tone = 0.55 + 0.35 * face + 0.15 * block_id;
            let c = mix3(
                scale3([0.66, 0.65, 0.66], tone),
                [0.10, 0.10, 0.11],
                mortar * 0.85,
            );
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: (0.88 - 0.12 * face).clamp(0.5, 1.0),
                metallic: 0.0,
                occlusion: 1.0 - 0.5 * mortar,
                glow: 0.0,
            }
        }
        Kind::Moss => {
            // A dense pile of tiny domes and fibres.
            let (f1, _f2, id) = worley(s, u, v, 30);
            let (g1, _g2, gid) = worley(s + 1, u, v, 72);
            let dome = 1.0 - smoothstep(0.0, 0.7, f1);
            let tuft = 1.0 - smoothstep(0.0, 0.55, g1);
            let clump = fbm(s + 2, u, v, 5, 4, 0.5);
            let h = 0.35 * dome + 0.30 * tuft + 0.25 * clump + 0.10 * gid;
            let dry = smoothstep(0.55, 0.75, fbm(s + 3, u, v, 4, 3, 0.5));
            let base = mix3([0.20, 0.34, 0.10], [0.36, 0.44, 0.14], id);
            let c = mix3(scale3(base, 0.6 + 0.6 * h), [0.42, 0.36, 0.20], dry * 0.5);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: 0.85 - 0.1 * tuft,
                metallic: 0.0,
                occlusion: 0.75 + 0.25 * h,
                glow: 0.0,
            }
        }
        Kind::Wood => {
            // Long grain with rings, knots and fine checks; the grain runs along u.
            let (wu, wv) = warp(s, u, v, 0.05, 3);
            let ring = ((wv * 9.0 + fbm_aniso(s + 1, wu, wv, 2, 6, 3, 0.5) * 3.5)
                * std::f32::consts::TAU)
                .sin()
                * 0.5
                + 0.5;
            let fibre = fbm_aniso(s + 2, u, v, 3, 96, 3, 0.55);
            let (k1, _, _) = worley(s + 3, u, v, 2);
            let knot = (1.0 - smoothstep(0.0, 0.16, k1)) * 0.5;
            let check = 1.0
                - smoothstep(
                    0.0,
                    0.02,
                    (fbm_aniso(s + 4, u, v, 2, 40, 2, 0.5) - 0.5).abs(),
                );
            let h = 0.5 + 0.18 * ring + 0.22 * (fibre - 0.5) - 0.18 * check + knot * 0.2;
            let tone = 0.55 + 0.25 * ring + 0.2 * fibre;
            let c = mix3([0.30, 0.19, 0.10], [0.55, 0.38, 0.22], tone);
            let c = scale3(c, 1.0 - 0.5 * check - 0.3 * knot);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: 0.66 + 0.2 * fibre,
                metallic: 0.0,
                occlusion: 1.0 - 0.5 * check,
                glow: 0.0,
            }
        }
        Kind::Bronze | Kind::Iron | Kind::Steel => {
            // Brushed, pitted metal with corrosion. Scratches run along u.
            let scratch = fbm_aniso(s, u, v, 2, 160, 4, 0.6);
            let fine = fbm_aniso(s + 1, u, v, 3, 220, 2, 0.5);
            let (p1, _, pid) = worley(s + 2, u, v, 26);
            let pit = 1.0 - smoothstep(0.0, 0.16, p1);
            let blotch = fbm(s + 3, u, v, 4, 5, 0.55);
            let corr = smoothstep(0.60, 0.80, blotch + 0.20 * pid * pit);
            let (metal_c, corr_c, rough_m): ([f32; 3], [f32; 3], f32) = match kind {
                Kind::Bronze => ([0.72, 0.47, 0.22], [0.16, 0.42, 0.34], 0.36),
                Kind::Iron => ([0.30, 0.30, 0.33], [0.40, 0.19, 0.08], 0.52),
                _ => ([0.72, 0.74, 0.78], [0.42, 0.38, 0.34], 0.26),
            };
            // Steel barely corrodes, bronze goes green in patches, iron rusts.
            let corr = match kind {
                Kind::Steel => corr * 0.25,
                Kind::Bronze => corr * 0.7,
                _ => corr,
            };
            let h = 0.55 + 0.10 * (scratch - 0.5) + 0.05 * (fine - 0.5) - 0.28 * pit - 0.12 * corr;
            let c = mix3(scale3(metal_c, 0.85 + 0.3 * scratch), corr_c, corr);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: (lerp(rough_m + 0.10 * (fine - 0.5), 0.78, corr) + 0.18 * pit)
                    .clamp(0.12, 1.0),
                metallic: lerp(1.0, 0.15, corr) * (1.0 - 0.6 * pit),
                occlusion: 1.0 - 0.35 * pit,
                glow: 0.0,
            }
        }
        Kind::Cloth | Kind::Burlap => {
            // A plain weave: over-under threads with slubs.
            let n = if kind == Kind::Cloth { 60.0 } else { 26.0 };
            let (tu, tv) = (u * n, v * n);
            let (iu, iv) = (tu.floor() as i32, tv.floor() as i32);
            let over = (iu + iv).rem_euclid(2) == 0;
            let (fu, fv) = (tu.fract(), tv.fract());
            // The thread that is on top at this cell, as a rounded ridge.
            let along = if over { fv } else { fu };
            let across = if over { fu } else { fv };
            let round = (std::f32::consts::PI * along).sin().powf(0.6);
            let dip = (std::f32::consts::PI * across).sin().powf(0.4);
            let slub = fbm_aniso(
                s,
                u,
                v,
                if over { 4 } else { 60 },
                if over { 60 } else { 4 },
                3,
                0.55,
            );
            let fuzz = fbm(s + 1, u, v, 120, 2, 0.5);
            let h = 0.35 + 0.40 * round * dip + 0.15 * (slub - 0.5) + 0.05 * fuzz;
            let tone = 0.65 + 0.5 * (slub - 0.5);
            let dye = if kind == Kind::Cloth {
                [0.55, 0.55, 0.60]
            } else {
                [0.72, 0.60, 0.40]
            };
            let c = scale3(dye, tone * (0.8 + 0.3 * round));
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: 0.93,
                metallic: 0.0,
                occlusion: 0.6 + 0.4 * round * dip,
                glow: 0.0,
            }
        }
        Kind::Bone => {
            // Ivory: pores, hairline cracks, faint veining.
            let (wu, wv) = warp(s, u, v, 0.05, 4);
            let pore = 1.0 - smoothstep(0.0, 0.2, worley(s, u, v, 56).0);
            let (f1, f2, _) = worley(s + 1, wu, wv, 6);
            let hair = 1.0 - smoothstep(0.0, 0.045, f2 - f1);
            let vein = ridged(s + 2, wu, wv, 4, 4);
            let mottle = fbm(s + 3, u, v, 8, 4, 0.5);
            let h = 0.60 + 0.08 * mottle - 0.20 * pore - 0.30 * hair;
            let c = mix3(
                [0.90, 0.86, 0.74],
                [0.72, 0.62, 0.44],
                mottle * 0.7 + 0.3 * vein.powf(6.0),
            );
            let c = scale3(c, 1.0 - 0.45 * hair - 0.12 * pore);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: 0.42 + 0.25 * mottle + 0.2 * pore,
                metallic: 0.0,
                occlusion: 1.0 - 0.4 * hair,
                glow: 0.0,
            }
        }
        Kind::Chitin => {
            // Lacquered armour plates: domed cells, thin seams that glow from
            // within, faint growth lines across each plate.
            let (wu, wv) = warp(s, u, v, 0.03, 4);
            let (f1, f2, id) = worley(s, wu, wv, 6);
            let seam = 1.0 - smoothstep(0.0, 0.055, f2 - f1);
            let dome = 1.0 - smoothstep(0.0, 0.85, f1);
            let growth = (f1 * 30.0).sin() * 0.5 + 0.5;
            let micro = fbm(s + 1, u, v, 48, 3, 0.55);
            let h =
                0.30 + 0.55 * dome.powf(0.7) - 0.50 * seam + 0.05 * growth * dome + 0.04 * micro;
            let plate = mix3([0.040, 0.032, 0.034], [0.115, 0.070, 0.050], id);
            let sheen = 0.75 + 0.5 * micro + 0.25 * dome * growth;
            let c = mix3(scale3(plate, sheen), [0.60, 0.22, 0.04], seam * 0.75);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: (0.20 + 0.32 * micro + 0.35 * seam).clamp(0.15, 1.0),
                metallic: 0.10,
                occlusion: 1.0 - 0.65 * seam,
                glow: seam.powf(1.3),
            }
        }
        Kind::Flesh => {
            // Skin: pores, folds, and a network of veins.
            let (wu, wv) = warp(s, u, v, 0.08, 3);
            let pore = 1.0 - smoothstep(0.0, 0.22, worley(s + 1, u, v, 64).0);
            let fold = ridged(s + 2, wu, wv, 5, 4).powf(4.0);
            let (f1, f2, _) = worley(s + 3, wu, wv, 5);
            let vein = 1.0 - smoothstep(0.0, 0.05, f2 - f1);
            let mottle = fbm(s + 4, u, v, 7, 4, 0.5);
            let h = 0.55 + 0.20 * mottle - 0.12 * pore - 0.22 * fold - 0.08 * vein;
            let c = mix3([0.62, 0.40, 0.34], [0.78, 0.55, 0.46], mottle);
            let c = mix3(c, [0.32, 0.20, 0.36], vein * 0.6);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: c,
                roughness: (0.42 + 0.2 * pore + 0.15 * fold).clamp(0.2, 1.0),
                metallic: 0.0,
                occlusion: 1.0 - 0.3 * fold,
                glow: vein * 0.6,
            }
        }
        Kind::Leather => {
            let (f1, f2, id) = worley(s, u, v, 22);
            let pebble = smoothstep(0.0, 0.35, f2 - f1);
            let crease = ridged(s + 1, u, v, 5, 4).powf(5.0);
            let wear = fbm(s + 2, u, v, 6, 4, 0.5);
            let h = 0.35 + 0.35 * pebble + 0.1 * id - 0.15 * crease;
            let c = mix3([0.20, 0.11, 0.07], [0.36, 0.22, 0.13], wear);
            Texel {
                height: h.clamp(0.0, 1.0),
                albedo: scale3(c, 0.8 + 0.4 * pebble),
                roughness: 0.55 + 0.3 * wear,
                metallic: 0.0,
                occlusion: 0.7 + 0.3 * pebble,
                glow: 0.0,
            }
        }
    }
}

/// Every glow map has at least this much glow everywhere, so a creature whose
/// body takes a tell colour (the emissive colour multiplies the map) is washed
/// evenly and its veins and seams glow brighter than the rest.
pub const EMISSIVE_FLOOR: f32 = 0.2;

/// Generates the maps of `kind` at `size` x `size`.
pub fn generate(kind: Kind, size: usize) -> PbrMaps {
    let texels = evaluate(size, &|u, v| {
        let mut t = texel(kind, u, v);
        t.glow = EMISSIVE_FLOOR + (1.0 - EMISSIVE_FLOOR) * t.glow;
        t
    });
    bake(size, &texels, kind.bump())
}

/// The texture size for a material (smaller in debug builds, which are slow).
pub fn size_for(kind: Kind) -> usize {
    let big = matches!(kind, Kind::Rock | Kind::Masonry | Kind::Moss);
    match (cfg!(debug_assertions), big) {
        (true, true) => 256,
        (true, false) => 128,
        (false, true) => 1024,
        (false, false) => 512,
    }
}

/// Every material's GPU maps.
#[derive(Resource, Clone)]
pub struct Materials(pub std::collections::HashMap<Kind, PbrImages>);

impl Materials {
    pub fn get(&self, k: Kind) -> &PbrImages {
        &self.0[&k]
    }
}

/// Generates and uploads every material (blocking; a few seconds of startup).
pub fn build_materials(images: &mut Assets<Image>) -> Materials {
    let maps: Vec<(Kind, PbrMaps)> = std::thread::scope(|s| {
        let handles: Vec<_> = Kind::ALL
            .into_iter()
            .map(|k| s.spawn(move || (k, generate(k, size_for(k)))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("texture thread"))
            .collect()
    });
    Materials(
        maps.into_iter()
            .map(|(k, m)| (k, upload(&m, images)))
            .collect(),
    )
}

/// Bakes every material set once, before anything that needs one is built.
pub struct PbrPlugin;

impl Plugin for PbrPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, init_materials);
    }
}

fn init_materials(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let started = std::time::Instant::now();
    let materials = build_materials(&mut images);
    info!(
        "baked {} material sets in {:.2?}",
        materials.0.len(),
        started.elapsed()
    );
    commands.insert_resource(materials);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_repeats_seamlessly() {
        // Whatever the period, u = 0 and u = 1 are the same lattice line.
        for k in 0..16 {
            let t = k as f32 / 16.0;
            let (a, b) = (noise2(3, 0.0, t, 8, 8), noise2(3, 1.0 - 1e-5, t, 8, 8));
            assert!((a - b).abs() < 1e-2, "value noise seam at {t}: {a} vs {b}");
            let (a, b) = (fbm(3, t, 0.0, 4, 5, 0.5), fbm(3, t, 1.0 - 1e-5, 4, 5, 0.5));
            assert!((a - b).abs() < 2e-2, "fbm seam at {t}: {a} vs {b}");
            let (w, x) = (worley(3, 0.0, t, 7), worley(3, 1.0 - 1e-5, t, 7));
            assert!((w.0 - x.0).abs() < 2e-2, "worley seam at {t}");
        }
    }

    #[test]
    fn noise_stays_in_range_and_varies() {
        let vals: Vec<f32> = (0..4096)
            .map(|k| fbm(1, (k % 64) as f32 / 64.0, (k / 64) as f32 / 64.0, 4, 5, 0.5))
            .collect();
        assert!(vals.iter().all(|v| (0.0..=1.0).contains(v)));
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        let sd = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / vals.len() as f32).sqrt();
        assert!((0.35..0.65).contains(&mean), "mean {mean}");
        assert!(sd > 0.05, "flat noise: {sd}");
        // Worley distances are non-negative and ordered.
        for k in 0..200 {
            let (f1, f2, id) = worley(2, k as f32 / 200.0, 0.37, 9);
            assert!(f1 >= 0.0 && f2 >= f1 && (0.0..1.0).contains(&id));
        }
    }

    #[test]
    fn every_material_bakes_valid_maps() {
        for kind in Kind::ALL {
            let m = generate(kind, 64);
            let n = 64 * 64 * 4;
            for (name, data) in [
                ("albedo", &m.albedo),
                ("normal", &m.normal),
                ("orm", &m.orm),
                ("height", &m.height),
                ("emissive", &m.emissive),
            ] {
                assert_eq!(data.len(), n, "{kind:?} {name}");
                assert!(data.chunks(4).all(|p| p[3] == 255), "{kind:?} {name} alpha");
            }
            // Normals are unit length (within byte quantisation) and face outward.
            for p in m.normal.chunks(4) {
                let v = Vec3::new(
                    p[0] as f32 / 127.5 - 1.0,
                    p[1] as f32 / 127.5 - 1.0,
                    p[2] as f32 / 127.5 - 1.0,
                );
                assert!(
                    (v.length() - 1.0).abs() < 0.05,
                    "{kind:?}: normal length {}",
                    v.length()
                );
                assert!(v.z > 0.0, "{kind:?}: normal faces into the surface");
            }
            // The material has real relief and real variation, and is not degenerate.
            let hs: Vec<f32> = m.height.chunks(4).map(|p| p[0] as f32 / 255.0).collect();
            let mean = hs.iter().sum::<f32>() / hs.len() as f32;
            let sd = (hs.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / hs.len() as f32).sqrt();
            assert!(sd > 0.03, "{kind:?}: height is flat (sd {sd})");
            let lum: Vec<f32> = m.albedo.chunks(4).map(|p| p[1] as f32 / 255.0).collect();
            let lmean = lum.iter().sum::<f32>() / lum.len() as f32;
            assert!(
                (0.08..0.95).contains(&lmean),
                "{kind:?}: albedo mean {lmean}"
            );
        }
    }

    #[test]
    fn metals_are_metallic_and_stone_is_not() {
        let mean_metal = |k: Kind| {
            let m = generate(k, 64);
            m.orm.chunks(4).map(|p| p[2] as f32 / 255.0).sum::<f32>() / (64.0 * 64.0)
        };
        for k in [Kind::Bronze, Kind::Iron, Kind::Steel] {
            assert!(mean_metal(k) > 0.5, "{k:?} should be metal");
        }
        for k in [Kind::Rock, Kind::Cloth, Kind::Bone, Kind::Flesh, Kind::Wood] {
            assert!(mean_metal(k) < 0.05, "{k:?} should not be metal");
        }
    }

    #[test]
    fn a_normal_map_tilts_toward_rising_ground() {
        // A ramp rising to the right: the normal leans left (-x), as it should.
        let size = 32;
        let texels: Vec<Texel> = (0..size * size)
            .map(|k| Texel {
                height: (k % size) as f32 / size as f32,
                ..Default::default()
            })
            .collect();
        let m = bake(size, &texels, 16.0);
        let mid = (size / 2 * size + size / 2) * 4;
        assert!(m.normal[mid] < 118, "x leans left: {}", m.normal[mid]);
        // A ramp rising down the image (increasing y): the map's green points up
        // the image, so the normal leans that way and green climbs above 128.
        let texels: Vec<Texel> = (0..size * size)
            .map(|k| Texel {
                height: (k / size) as f32 / size as f32,
                ..Default::default()
            })
            .collect();
        let m = bake(size, &texels, 16.0);
        assert!(
            m.normal[mid + 1] > 137,
            "y leans up the image: {}",
            m.normal[mid + 1]
        );
    }

    #[test]
    fn mip_chains_are_complete_and_normals_stay_unit() {
        let m = generate(Kind::Rock, 64);
        let (all, levels) = with_mips(&m.normal, 64, false, true);
        assert_eq!(levels, 7, "64, 32, 16, 8, 4, 2, 1");
        let expected: usize = (0..7).map(|l| (64usize >> l).pow(2) * 4).sum();
        assert_eq!(all.len(), expected);
        // The 1x1 normal is still a unit vector.
        let last = &all[all.len() - 4..];
        let v = Vec3::new(
            last[0] as f32 / 127.5 - 1.0,
            last[1] as f32 / 127.5 - 1.0,
            last[2] as f32 / 127.5 - 1.0,
        );
        assert!((v.length() - 1.0).abs() < 0.05);
        // Colour mips keep the average brightness roughly (linear-light averaging).
        let (calls, _) = with_mips(&m.albedo, 64, true, false);
        let top: f32 = m.albedo.chunks(4).map(|p| p[1] as f32).sum::<f32>() / (64.0 * 64.0);
        let tail = calls[calls.len() - 4 + 1] as f32;
        assert!((top - tail).abs() < 45.0, "mean {top} vs 1x1 {tail}");
    }

    #[test]
    fn stone_and_masonry_are_mid_grey_so_palettes_tint_them_predictably() {
        let lin = |b: u8| {
            let c = b as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let luma = |kind: Kind| {
            let m = generate(kind, 64);
            let sum: f32 = m
                .albedo
                .chunks(4)
                .map(|p| 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2]))
                .sum();
            sum / (64.0 * 64.0)
        };
        for k in [Kind::Rock, Kind::Masonry] {
            let l = luma(k);
            assert!((0.3..0.6).contains(&l), "{k:?} luma {l}");
        }
        assert!(luma(Kind::Chitin) < 0.2, "husk shells are dark");
        assert!(
            luma(Kind::Bone) > luma(Kind::Rock),
            "bone is paler than rock"
        );
    }

    /// `cargo test --release -p hk_game bake_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "a timing probe, not a check"]
    fn bake_cost() {
        let t = std::time::Instant::now();
        let mut images = Assets::<Image>::default();
        let m = build_materials(&mut images);
        println!(
            "{} sets at release sizes in {:.2?} (cores: {})",
            m.0.len(),
            t.elapsed(),
            std::thread::available_parallelism().map_or(0, |n| n.get())
        );
    }
}
