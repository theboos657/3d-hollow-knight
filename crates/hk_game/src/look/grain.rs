//! Film grain: a fine, fast-changing speckle laid over the picture (under the
//! HUD), so flat dark areas do not band and the whole frame has the slight
//! restlessness of film. Only on tiers that ask for it.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::widget::NodeImageMode;

use super::quality::CurrentPlan;
use crate::rig::meshkit::hash3;

/// Side of a grain tile, in pixels (drawn at one texel per pixel).
pub const SIDE: usize = 256;
/// Distinct frames of grain, cycled.
pub const FRAMES: usize = 6;
/// Grain frames per second.
pub const RATE: f32 = 20.0;
/// The peak opacity of a grain speck (0..1).
pub const STRENGTH: f32 = 0.016;

/// One speck: a shade (0 or 255) and an opacity, from position and frame.
pub fn speck(frame: usize, x: usize, y: usize) -> (u8, u8) {
    let n = hash3(0x61A1 + frame as u32 * 7919, x as i32, y as i32, 3);
    let m = hash3(0x2F0D + frame as u32 * 104_729, x as i32, y as i32, 5);
    let shade = if n < 0.5 { 0 } else { 255 };
    // Most specks are faint; a few are stronger, like film's clumps.
    let alpha = STRENGTH * m * m * 255.0 * 1.6;
    (shade, alpha.min(255.0 * STRENGTH * 1.6) as u8)
}

#[derive(Component)]
struct Grain {
    frames: Vec<Handle<Image>>,
}

fn spawn_grain(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let frames = (0..FRAMES)
        .map(|f| {
            let mut data = Vec::with_capacity(SIDE * SIDE * 4);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let (shade, alpha) = speck(f, x, y);
                    data.extend([shade, shade, shade, alpha]);
                }
            }
            images.add(Image::new(
                Extent3d {
                    width: SIDE as u32,
                    height: SIDE as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ))
        })
        .collect::<Vec<_>>();
    let mut node = ImageNode::new(frames[0].clone());
    node.image_mode = NodeImageMode::Tiled {
        tile_x: true,
        tile_y: true,
        stretch_value: 1.0,
    };
    commands.spawn((
        Grain { frames },
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        node,
        // Over the vignette, under the HUD.
        GlobalZIndex(-40),
        Visibility::Hidden,
    ));
}

fn animate_grain(
    time: Res<Time>,
    plan: Res<CurrentPlan>,
    mut q: Query<(&Grain, &mut ImageNode, &mut Visibility)>,
) {
    for (g, mut img, mut vis) in &mut q {
        let want = if plan.0.grain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        if plan.0.grain {
            let k = (time.elapsed_secs() * RATE) as usize % g.frames.len();
            if img.image != g.frames[k] {
                img.image = g.frames[k].clone();
            }
        }
    }
}

pub struct GrainPlugin;

impl Plugin for GrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_grain)
            .add_systems(Update, animate_grain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_is_faint_balanced_and_changes_every_frame() {
        let (mut dark, mut light, mut sum) = (0u32, 0u32, 0u64);
        for y in 0..64 {
            for x in 0..64 {
                let (shade, alpha) = speck(0, x, y);
                if shade == 0 {
                    dark += 1;
                } else {
                    light += 1;
                }
                sum += alpha as u64;
                assert!(
                    alpha as f32 <= 255.0 * STRENGTH * 1.6 + 1.0,
                    "no speck is strong"
                );
            }
        }
        let total = 64 * 64;
        assert!(dark > total / 3 && light > total / 3, "both dark and light");
        let mean = sum as f32 / total as f32 / 255.0;
        assert!(mean > 0.005 && mean < STRENGTH, "faint on average: {mean}");
        assert_ne!(speck(0, 5, 5), speck(1, 5, 5), "frames differ");
    }
}
