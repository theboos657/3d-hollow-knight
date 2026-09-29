//! Procedural character rigs: geometry (`meshkit`), animation maths (`pose`)
//! and the small pieces of glue shared by every model.
//!
//! A model is a hierarchy of entities under the simulation "anchor" (the
//! entity `interp` moves to the creature's sim position):
//!
//! ```text
//! anchor            translation only (Interpolated); never scaled or rotated
//! |- lights         siblings of the model, so squash and flicker never touch them
//! `- ModelRoot      feet pivot; Visibility here flickers with i-frames
//!    `- Squash     scale about the feet (landing squash, stretch)
//!       `- Facing  yaw toward the way the creature faces (never a negative scale)
//!          `- Lean rotation about the feet and vertical drop (kneel, collapse)
//!             `- joints ... parts
//! ```
//!
//! Joints carry a [`Rest`] transform; the pose functions produce deltas that
//! [`posed`] applies on top, so a pose can be swapped without rebuilding.

pub mod creature;
pub mod meshkit;
pub mod pose;

use bevy::prelude::*;
use pose::JointXf;

/// A joint's rest transform (its pose is `rest` plus a [`JointXf`] delta).
#[derive(Component, Clone, Copy, Debug)]
pub struct Rest(pub Transform);

/// The flicker target of a model: the node whose `Visibility` is toggled.
#[derive(Component)]
pub struct ModelRoot;

/// `rest` with the delta `d` applied.
pub fn posed(rest: &Transform, d: &JointXf) -> Transform {
    Transform {
        translation: rest.translation + Vec3::from(d.pos),
        rotation: rest.rotation * Quat::from_rotation_z(d.rot),
        scale: rest.scale * Vec3::from(d.scale),
    }
}

/// Spawns a mesh part as a child of `parent`.
pub fn part(
    commands: &mut Commands,
    parent: Entity,
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    at: Transform,
) -> Entity {
    let e = commands
        .spawn((Mesh3d(mesh), MeshMaterial3d(mat), at, Visibility::default()))
        .id();
    commands.entity(parent).add_child(e);
    e
}

/// Spawns an empty joint (with its rest transform) under `parent`.
pub fn joint(commands: &mut Commands, parent: Entity, rest: Transform) -> Entity {
    let e = commands
        .spawn((rest, Rest(rest), Visibility::default()))
        .id();
    commands.entity(parent).add_child(e);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posing_adds_to_the_rest_transform() {
        let rest = Transform::from_xyz(1.0, 2.0, 3.0).with_scale(Vec3::splat(2.0));
        let d = JointXf {
            pos: [0.5, 0.0, 0.0],
            rot: std::f32::consts::FRAC_PI_2,
            scale: [1.0, 0.5, 1.0],
        };
        let t = posed(&rest, &d);
        assert_eq!(t.translation, Vec3::new(1.5, 2.0, 3.0));
        assert_eq!(t.scale, Vec3::new(2.0, 1.0, 2.0));
        let up = t.rotation * Vec3::X;
        assert!(
            (up - Vec3::Y).length() < 1e-5,
            "rotated a quarter turn about Z"
        );
        // The identity delta leaves the rest transform alone.
        let same = posed(&rest, &JointXf::REST);
        assert_eq!(same.translation, rest.translation);
        assert_eq!(same.scale, rest.scale);
    }
}
