//! Everything that flies or sweeps: the Ember Bolt, the Spitter's acid, the
//! Matron's and the Bellwarden's ground shockwaves, falling bells and the
//! bosses' melee arcs. Each is drawn from the hitbox the simulation created,
//! sized to it, so what looks dangerous is exactly what is.
//!
//! The player's nail has no box here: the sword and its trail are the visual
//! (`models::knight`); `--show-hitboxes` draws the box for debugging.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hk_sim::boss::Boss;
use hk_sim::combat::{HitKind, Hitbox, HitboxFollow, Hurtbox, Team};
use hk_sim::components::{SimPos, Velocity};
use hk_sim::enemy::Brain;

use crate::interp::{Interpolated, RenderPrepSet};
use crate::look::kits::bell;
use crate::look::props::Flame;
use crate::rig::meshkit::{cone, crescent, ellipsoid, extrude, MeshData};
use crate::scene::Palette;
use crate::visuals::ShowHitboxes;

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_assets)
            .add_systems(Update, (attach_hit_visuals, fade_arcs).after(RenderPrepSet));
    }
}

// ---------------------------------------------------------------- geometry --

/// The ground shockwave, as a side profile of a cresting wave, unit sized
/// (spans x in [-0.5, 0.5], y in [0, 1.2]); vertex alpha fades toward the top.
pub fn shockwave_mesh() -> MeshData {
    let profile = [
        Vec2::new(-0.5, 0.0),
        Vec2::new(-0.35, 0.35),
        Vec2::new(-0.05, 0.85),
        Vec2::new(0.45, 1.2),
        Vec2::new(0.30, 0.65),
        Vec2::new(0.5, 0.0),
    ];
    extrude(&profile, 0.7).recolor(|p| {
        let a = (1.0 - p.y / 1.4).clamp(0.15, 1.0);
        [1.0, 0.55 + 0.3 * (1.0 - a), 0.18, a]
    })
}

/// A slash crescent for melee arcs, unit sized (spans x in [0, 1], y in [-1, 1]).
pub fn arc_mesh() -> MeshData {
    crescent(1.0, 0.04, 0.55, -1.25, 1.25, 16)
}

/// A comet tail for bolts and bells: a cone pointing in -X, alpha fading out.
pub fn tail_mesh(len: f32, r: f32) -> MeshData {
    cone(r, len, 8)
        .transformed(Mat4::from_rotation_z(FRAC_PI_2))
        .recolor(move |p| {
            let a = (1.0 - (-p.x) / len).clamp(0.0, 1.0);
            [1.0, 1.0, 1.0, a * 0.7]
        })
}

#[derive(Resource)]
struct ProjAssets {
    orb: Handle<Mesh>,
    tail: Handle<Mesh>,
    wave: Handle<Mesh>,
    arc: Handle<Mesh>,
    bell: Handle<Mesh>,
    bolt_core: Handle<StandardMaterial>,
    acid_core: Handle<StandardMaterial>,
    add_ember: Handle<StandardMaterial>,
    add_acid: Handle<StandardMaterial>,
    wave_mat: Handle<StandardMaterial>,
    arc_mat: Handle<StandardMaterial>,
    bell_mat: Handle<StandardMaterial>,
    streak_mat: Handle<StandardMaterial>,
}

fn additive(colour: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: colour,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    }
}

fn build_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |c: Color, e: LinearRgba| StandardMaterial {
        base_color: c,
        emissive: e,
        ..default()
    };
    commands.insert_resource(ProjAssets {
        orb: meshes.add(ellipsoid(1.0, 1.0, 1.0, 10, 14).to_mesh()),
        tail: meshes.add(tail_mesh(1.0, 0.5).to_mesh()),
        wave: meshes.add(shockwave_mesh().to_mesh()),
        arc: meshes.add(arc_mesh().to_mesh()),
        // Unit bell (r = 1); scaled at spawn.
        bell: meshes.add(bell(0.0, 1.55, 1.0, 0.0).to_mesh()),
        bolt_core: mats.add(glow(
            Color::srgb(1.0, 0.75, 0.3),
            LinearRgba::rgb(4.5, 2.0, 0.4),
        )),
        acid_core: mats.add(glow(
            Color::srgb(0.6, 1.0, 0.3),
            LinearRgba::rgb(1.0, 3.6, 0.4),
        )),
        add_ember: mats.add(additive(Color::srgb(1.0, 0.55, 0.15))),
        add_acid: mats.add(additive(Color::srgb(0.45, 1.0, 0.25))),
        wave_mat: mats.add(additive(Color::WHITE)),
        arc_mat: mats.add(additive(Color::srgb(1.0, 0.35, 0.25))),
        bell_mat: mats.add(StandardMaterial {
            base_color: Color::srgb(0.72, 0.5, 0.22),
            metallic: 0.8,
            perceptual_roughness: 0.35,
            emissive: LinearRgba::rgb(0.5, 0.22, 0.05),
            ..default()
        }),
        streak_mat: mats.add(additive(Color::srgb(1.0, 0.7, 0.35))),
    });
}

/// A trailing arc's age, for fading it out.
#[derive(Component)]
pub struct ArcFade {
    pub life: f32,
    pub max: f32,
    /// The crescent's full height scale (the hitbox's half height).
    pub base_y: f32,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn attach_hit_visuals(
    mut commands: Commands,
    assets: Option<Res<ProjAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    pal: Res<Palette>,
    show: Res<ShowHitboxes>,
    owners: Query<(Option<&Brain>, Option<&Boss>)>,
    q: Query<
        (
            Entity,
            &Hitbox,
            &SimPos,
            Option<&Velocity>,
            Option<&HitboxFollow>,
        ),
        (Added<Hitbox>, Without<Hurtbox>),
    >,
) {
    let Some(a) = assets else {
        return;
    };
    for (e, hb, pos, vel, follow) in &q {
        let v = vel.map_or(Vec2::ZERO, |v| v.0);
        let dir = if v.x < 0.0 { -1.0f32 } else { 1.0 };
        let base = Transform::from_xyz(pos.0.x, pos.0.y, 0.4);
        let interp = Interpolated {
            z: 0.4,
            offset: Vec2::ZERO,
        };
        match (hb.team, hb.kind) {
            // The sword is the visual for the nail.
            (Team::Player, HitKind::Nail) => {
                if show.0 {
                    debug_box(&mut commands, &mut meshes, &pal, e, hb, base, interp);
                }
            }
            // The Ember Bolt: a hot core with a comet tail and a light.
            (Team::Player, HitKind::Spell) => {
                let r = hb.half.y * 0.95;
                commands
                    .entity(e)
                    .insert((base, interp, Visibility::default()))
                    .with_children(|p| {
                        p.spawn((
                            NotShadowCaster,
                            Mesh3d(a.orb.clone()),
                            MeshMaterial3d(a.bolt_core.clone()),
                            Transform::from_scale(Vec3::splat(r)),
                        ));
                        p.spawn((
                            NotShadowCaster,
                            Mesh3d(a.tail.clone()),
                            MeshMaterial3d(a.add_ember.clone()),
                            Transform::from_scale(Vec3::new(
                                hb.half.x * 3.4 * dir,
                                r * 2.2,
                                r * 2.2,
                            ))
                            .with_rotation(Quat::IDENTITY),
                        ));
                        p.spawn((
                            Flame {
                                base: 420_000.0,
                                phase: pos.0.x,
                            },
                            PointLight {
                                intensity: 420_000.0,
                                range: 9.0,
                                color: Color::srgb(1.0, 0.6, 0.25),
                                shadows_enabled: false,
                                ..default()
                            },
                            Transform::from_xyz(0.0, 0.0, 0.8),
                        ));
                    });
            }
            // Something an enemy or boss threw or summoned.
            (_, HitKind::Projectile) => {
                let (brain, boss) = owners.get(hb.owner).unwrap_or((None, None));
                if brain.is_some() {
                    // The Spitter's acid: a glob with a dripping tail.
                    let r = hb.half.x * 1.1;
                    commands
                        .entity(e)
                        .insert((base, interp, Visibility::default()))
                        .with_children(|p| {
                            p.spawn((
                                NotShadowCaster,
                                Mesh3d(a.orb.clone()),
                                MeshMaterial3d(a.acid_core.clone()),
                                Transform::from_scale(Vec3::splat(r)),
                            ));
                            let ang = v.y.atan2(v.x);
                            p.spawn((
                                NotShadowCaster,
                                Mesh3d(a.tail.clone()),
                                MeshMaterial3d(a.add_acid.clone()),
                                Transform::from_scale(Vec3::new(r * 3.2, r * 1.8, r * 1.8))
                                    .with_rotation(Quat::from_rotation_z(ang)),
                            ));
                            p.spawn((
                                Flame {
                                    base: 160_000.0,
                                    phase: pos.0.y,
                                },
                                PointLight {
                                    intensity: 160_000.0,
                                    range: 6.0,
                                    color: Color::srgb(0.5, 1.0, 0.3),
                                    shadows_enabled: false,
                                    ..default()
                                },
                                Transform::from_xyz(0.0, 0.0, 0.6),
                            ));
                        });
                } else if boss.is_some() || v.y.abs() <= v.x.abs() {
                    // A ground shockwave: a cresting wave of fire.
                    commands
                        .entity(e)
                        .insert((base, interp, Visibility::default()))
                        .with_children(|p| {
                            p.spawn((
                                NotShadowCaster,
                                Mesh3d(a.wave.clone()),
                                MeshMaterial3d(a.wave_mat.clone()),
                                Transform::from_xyz(0.0, -hb.half.y, 0.0).with_scale(Vec3::new(
                                    hb.half.x * 2.0 * dir,
                                    hb.half.y / 0.6,
                                    1.4,
                                )),
                            ));
                            p.spawn((
                                Flame {
                                    base: 300_000.0,
                                    phase: pos.0.x,
                                },
                                PointLight {
                                    intensity: 300_000.0,
                                    range: 8.0,
                                    color: Color::srgb(1.0, 0.5, 0.2),
                                    shadows_enabled: false,
                                    ..default()
                                },
                                Transform::from_xyz(0.0, 0.0, 1.0),
                            ));
                        });
                } else {
                    // A falling bell, with a streak of speed above it.
                    let r = hb.half.x * 1.25;
                    commands
                        .entity(e)
                        .insert((base, interp, Visibility::default()))
                        .with_children(|p| {
                            p.spawn((
                                Mesh3d(a.bell.clone()),
                                MeshMaterial3d(a.bell_mat.clone()),
                                Transform::from_xyz(0.0, hb.half.y - 1.55 * r, 0.0)
                                    .with_scale(Vec3::splat(r)),
                            ));
                            p.spawn((
                                NotShadowCaster,
                                Mesh3d(a.tail.clone()),
                                MeshMaterial3d(a.streak_mat.clone()),
                                Transform::from_xyz(0.0, hb.half.y, 0.0)
                                    .with_scale(Vec3::new(hb.half.y * 4.0, r * 1.6, r * 1.6))
                                    .with_rotation(Quat::from_rotation_z(-FRAC_PI_2)),
                            ));
                        });
                }
            }
            // A boss's melee arc: a red crescent swept across the hitbox.
            (Team::Enemy, HitKind::Contact) if follow.is_some() => {
                let side = follow.map_or(1.0, |f| if f.rel.x < 0.0 { -1.0 } else { 1.0 });
                commands
                    .entity(e)
                    .insert((
                        base,
                        interp,
                        Visibility::default(),
                        ArcFade {
                            life: 0.2,
                            max: 0.2,
                            base_y: hb.half.y,
                        },
                    ))
                    .with_children(|p| {
                        p.spawn((
                            NotShadowCaster,
                            Mesh3d(a.arc.clone()),
                            MeshMaterial3d(a.arc_mat.clone()),
                            // The crescent is authored bulging toward +x, from x = 0.
                            Transform::from_xyz(-hb.half.x * side, 0.0, 0.0).with_scale(Vec3::new(
                                hb.half.x * 2.0 * side,
                                hb.half.y,
                                1.0,
                            )),
                        ));
                    });
            }
            // Anything else keeps a plain translucent box, so nothing is invisible.
            _ => debug_box(&mut commands, &mut meshes, &pal, e, hb, base, interp),
        }
    }
}

fn debug_box(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    pal: &Palette,
    e: Entity,
    hb: &Hitbox,
    base: Transform,
    interp: Interpolated,
) {
    let mat = match hb.kind {
        HitKind::Spell => &pal.bolt,
        HitKind::Projectile => &pal.hazard,
        _ => &pal.slash,
    };
    commands.entity(e).insert((
        Mesh3d(meshes.add(Cuboid::new(hb.half.x * 2.0, hb.half.y * 2.0, 0.6))),
        MeshMaterial3d(mat.clone()),
        base,
        interp,
    ));
}

/// Melee arcs thin out over their short life.
fn fade_arcs(
    time: Res<Time>,
    mut q: Query<(&mut ArcFade, &Children)>,
    mut t: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (mut f, children) in &mut q {
        f.life = (f.life - dt).max(0.0);
        let k = (f.life / f.max).clamp(0.0, 1.0);
        for c in children.iter() {
            if let Ok(mut tr) = t.get_mut(c) {
                tr.scale.y = f.base_y * (0.45 + 0.55 * k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projectile_meshes_are_well_formed() {
        shockwave_mesh().validate().expect("wave");
        arc_mesh().validate().expect("arc");
        tail_mesh(1.0, 0.5).validate().expect("tail");
    }

    #[test]
    fn the_shockwave_fits_its_unit_box() {
        let (lo, hi) = shockwave_mesh().bounds();
        assert!(lo.x >= -0.51 && hi.x <= 0.51, "width {lo:?} {hi:?}");
        assert!(lo.y >= -0.01 && hi.y <= 1.21, "height {lo:?} {hi:?}");
    }

    #[test]
    fn the_arc_sweeps_forward_from_the_origin() {
        let (lo, hi) = arc_mesh().bounds();
        assert!(lo.x >= -0.01 && hi.x <= 1.01, "x in [0, 1]: {lo:?} {hi:?}");
        assert!(hi.y > 0.8 && lo.y < -0.8, "tall enough to cover the hitbox");
    }

    #[test]
    fn the_tail_streams_backwards() {
        let (lo, hi) = tail_mesh(2.0, 0.4).bounds();
        assert!(hi.x <= 0.01 && lo.x >= -2.01, "tail goes -x: {lo:?} {hi:?}");
    }
}
