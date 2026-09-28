//! Render interpolation: visuals lerp `PrevPos` -> `SimPos` by how far we are
//! into the next fixed step, so motion is smooth at any refresh rate while the
//! simulation stays locked to 120 Hz.

use bevy::prelude::*;
use hk_sim::components::{PrevPos, SimPos};

/// Marks an entity whose `Transform` follows its sim position.
#[derive(Component)]
pub struct Interpolated {
    /// Depth on the z axis (the sim lane is z = 0).
    pub z: f32,
    /// Offset from the sim position to the mesh origin.
    pub offset: Vec2,
}

pub struct InterpPlugin;

impl Plugin for InterpPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, interpolate.in_set(RenderPrepSet));
    }
}

/// Everything that positions things for the frame (interpolation, then camera).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderPrepSet;

fn interpolate(
    fixed: Res<Time<Fixed>>,
    mut q: Query<(&SimPos, &PrevPos, &Interpolated, &mut Transform)>,
) {
    let alpha = fixed.overstep_fraction();
    for (pos, prev, interp, mut t) in &mut q {
        let p = prev.0.lerp(pos.0, alpha) + interp.offset;
        t.translation = Vec3::new(p.x, p.y, interp.z);
    }
}
