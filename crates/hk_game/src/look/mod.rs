//! How the world looks: the level's stone, the chamber behind it, the light
//! that falls on it and the fires in it. Everything here reads the simulation's
//! room and never feeds back into it.

pub mod decor;
pub mod fixtures;
pub mod grain;
pub mod ibl;
pub mod kits;
pub mod level;
pub mod pbr;
pub mod props;
pub mod quality;
pub mod room;
pub mod style;
pub mod vignette;
pub mod wet;

use bevy::prelude::*;

use crate::interp::RenderPrepSet;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motes_climb_and_fade_at_both_ends() {
        let m = Mote {
            base: Vec3::new(5.0, 2.0, -1.0),
            phase: 0.3,
            rise: 0.4,
            span: 6.0,
        };
        let mut last_y = f32::MIN;
        let (mut min_size, mut max_size) = (f32::MAX, f32::MIN);
        for k in 0..600 {
            let (p, s) = mote_state(&m, k as f32 * 0.05);
            assert!(
                p.y >= 2.0 - 1e-3 && p.y <= 8.0 + 1e-3,
                "stays within its climb"
            );
            assert!((0.0..=1.0 + 1e-6).contains(&s));
            min_size = min_size.min(s);
            max_size = max_size.max(s);
            let _ = last_y;
            last_y = p.y;
        }
        assert!(min_size < 0.05, "it fades out where it restarts");
        assert!(max_size > 0.95, "and is fully visible mid-climb");
    }
}

/// Everything spawned for the current room (torn down when the room changes).
#[derive(Component, Clone, Copy)]
pub struct RoomVisual;

/// A speck of light drifting in the air: rises slowly, sways, and fades in and
/// out so it never pops.
#[derive(Component)]
pub struct Mote {
    pub base: Vec3,
    pub phase: f32,
    /// Units per second upward.
    pub rise: f32,
    /// How far it climbs before starting over.
    pub span: f32,
}

/// Where a mote is at time `t`, and how big (0..1) so it fades at the ends of
/// its climb.
pub fn mote_state(m: &Mote, t: f32) -> (Vec3, f32) {
    let u = ((t * m.rise + m.phase * m.span) / m.span).rem_euclid(1.0);
    let pos = m.base
        + Vec3::new(
            0.9 * (t * 0.23 + m.phase * 5.0).sin(),
            u * m.span,
            0.4 * (t * 0.17 + m.phase * 3.0).sin(),
        );
    (pos, (std::f32::consts::PI * u).sin())
}

fn drift_motes(time: Res<Time>, mut q: Query<(&Mote, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (m, mut tr) in &mut q {
        let (pos, size) = mote_state(m, t);
        tr.translation = pos;
        tr.scale = Vec3::splat(size.max(0.001));
    }
}

/// The main directional light (the one that casts shadows).
#[derive(Component)]
pub struct KeyLight;

/// A weak opposite light that lifts silhouettes off the wall.
#[derive(Component)]
pub struct RimLight;

/// A point light that must not glow in the volumetric haze: one that sits in
/// the middle of the play lane, where a view ray passes so close to it that the
/// haze's scattering blows up into a black speck.
#[derive(Component)]
pub struct NoHalo;

/// What the current area asks of the shared lighting (set when a room or the
/// title stage is built), so the tier systems can apply it.
#[derive(Resource)]
pub struct LookState {
    pub theme: hk_sim::world::room::Theme,
    /// The flat ambient light brightness the area is tuned for (image-based
    /// light replaces most of it where the tier has it).
    pub ambient_brightness: f32,
}

impl Default for LookState {
    fn default() -> Self {
        LookState {
            theme: hk_sim::world::room::Theme::Ashen,
            ambient_brightness: 250.0,
        }
    }
}

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            pbr::PbrPlugin,
            quality::QualityPlugin,
            ibl::IblPlugin,
            grain::GrainPlugin,
        ))
        .init_resource::<LookState>()
        .add_systems(
            Update,
            (
                room::rebuild_room,
                props::flicker,
                drift_motes,
                fixtures::attach_fixtures,
                fixtures::attach_spikes,
                fixtures::animate_fixtures,
                wet::animate_drips,
            )
                .after(RenderPrepSet),
        );
    }
}
