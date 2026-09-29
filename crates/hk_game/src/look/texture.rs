//! Procedural textures for the level: a mottled stone grain that every block
//! samples through its own random window, so no two blocks look the same.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::rig::meshkit::hash3;

pub const N: usize = 128;

/// Smooth value noise on a wrapping lattice of `cells` x `cells`.
fn value_noise(seed: u32, x: f32, y: f32, cells: i32) -> f32 {
    let (fx, fy) = (x * cells as f32, y * cells as f32);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let h = |dx: i32, dy: i32| {
        hash3(
            seed,
            (ix + dx).rem_euclid(cells),
            (iy + dy).rem_euclid(cells),
            0,
        )
    };
    let top = h(0, 0) + (h(1, 0) - h(0, 0)) * s(tx);
    let bot = h(0, 1) + (h(1, 1) - h(0, 1)) * s(tx);
    top + (bot - top) * s(ty)
}

/// The grey level (0..1) of the stone grain at `(x, y)` in `[0, 1)`; tiles
/// seamlessly. Blotches at three scales, fine speckle and a few dark cracks.
pub fn stone_value(x: f32, y: f32) -> f32 {
    let blotch = 0.5 * value_noise(1, x, y, 4)
        + 0.3 * value_noise(2, x, y, 8)
        + 0.2 * value_noise(3, x, y, 16);
    let speck = value_noise(4, x, y, 64) - 0.5;
    // Cracks: thin dark lines where a noise field crosses a level.
    let c = (value_noise(5, x, y, 6) - 0.5).abs();
    let crack = if c < 0.012 { 0.35 } else { 1.0 };
    ((0.62 + 0.55 * (blotch - 0.5) + 0.10 * speck) * crack).clamp(0.05, 1.0)
}

pub fn stone_grain(images: &mut Assets<Image>) -> Handle<Image> {
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let v = stone_value(x as f32 / N as f32, y as f32 / N as f32);
            let b = (v * 255.0) as u8;
            data.extend([b, b, b, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: N as u32,
            height: N as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    images.add(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grain_is_varied_but_never_black_or_white() {
        let vals: Vec<f32> = (0..N * N)
            .map(|k| stone_value((k % N) as f32 / N as f32, (k / N) as f32 / N as f32))
            .collect();
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / vals.len() as f32;
        assert!((0.45..0.75).contains(&mean), "mean {mean}");
        assert!(var.sqrt() > 0.04, "flat texture: sd {}", var.sqrt());
        assert!(vals.iter().all(|v| (0.04..=1.0).contains(v)));
    }

    #[test]
    fn the_grain_tiles_seamlessly() {
        // Noise lattice wraps, so the texture repeats without a visible seam.
        for k in 0..16 {
            let t = k as f32 / 16.0;
            let (a, b) = (stone_value(0.0, t), stone_value(1.0 - 1e-4, t));
            assert!((a - b).abs() < 0.08, "seam in x at {t}: {a} vs {b}");
            let (a, b) = (stone_value(t, 0.0), stone_value(t, 1.0 - 1e-4));
            assert!((a - b).abs() < 0.08, "seam in y at {t}: {a} vs {b}");
        }
    }
}
