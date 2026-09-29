//! A soft dark vignette over the 3D view (under the HUD), so the eye rests in
//! the middle of the screen where the knight is.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub struct VignettePlugin;

impl Plugin for VignettePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_vignette);
    }
}

const W: usize = 128;
const H: usize = 72;

/// The vignette's opacity at `(u, v)` in `[0, 1]^2`: clear through the middle,
/// darkening toward the corners (never fully black).
pub fn vignette_alpha(u: f32, v: f32) -> f32 {
    let (x, y) = ((u - 0.5) * 2.0, (v - 0.5) * 2.0);
    // A rounded-rectangle distance, so the top and bottom edges darken too.
    let d = (x.abs().powf(2.6) + y.abs().powf(2.6)).powf(1.0 / 2.6);
    let t = ((d - 0.55) / 0.85).clamp(0.0, 1.0);
    0.62 * t * t * (3.0 - 2.0 * t)
}

fn spawn_vignette(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut data = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        for x in 0..W {
            let a = vignette_alpha((x as f32 + 0.5) / W as f32, (y as f32 + 0.5) / H as f32);
            data.extend([4, 3, 8, (a * 255.0) as u8]);
        }
    }
    let image = Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(images.add(image)),
        GlobalZIndex(-50),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_is_clear_and_the_corners_are_dark_but_not_black() {
        assert_eq!(vignette_alpha(0.5, 0.5), 0.0);
        assert!(vignette_alpha(0.0, 0.0) > 0.5);
        assert!(
            vignette_alpha(0.0, 0.0) < 0.7,
            "the corners still show the world"
        );
        assert!(
            vignette_alpha(1.0, 1.0) == vignette_alpha(0.0, 0.0),
            "symmetric"
        );
    }

    #[test]
    fn the_vignette_only_grows_outward() {
        let mut last = 0.0;
        for k in 0..=10 {
            let a = vignette_alpha(0.5 + 0.05 * k as f32, 0.5 + 0.05 * k as f32);
            assert!(a >= last - 1e-6, "monotone along the diagonal");
            last = a;
        }
        // The lanes the knight uses (the vertical middle band) stay clear.
        assert!(vignette_alpha(0.25, 0.5) < 0.05);
        assert!(vignette_alpha(0.75, 0.5) < 0.05);
    }
}
