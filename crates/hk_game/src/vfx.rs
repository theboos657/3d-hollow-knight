//! Juice: streak sparks and impact rings for hits, blocks and deaths, dust for
//! landings and wall slides, afterimages for dashes. Purely visual: nothing
//! here feeds back into the simulation.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use hk_sim::combat::{Blocked, EnemyDied, Hit, HitKind, PlayerDied, Team};
use hk_sim::components::{Aabb, SimPos, Velocity};
use hk_sim::player::{Facing, Motor, Player, PlayerState};

use crate::interp::RenderPrepSet;
use crate::models::knight::KnightAssets;
use crate::rig::meshkit::ring as ring_mesh;

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VfxRng>()
            .add_systems(Startup, setup_assets)
            .add_systems(
                Update,
                (
                    spawn_hit_vfx,
                    dust_and_trails,
                    animate_particles,
                    animate_rings,
                    animate_ghosts,
                )
                    .after(RenderPrepSet),
            );
    }
}

#[derive(Resource)]
struct VfxAssets {
    cube: Handle<Mesh>,
    ring: Handle<Mesh>,
    // Unlit additive: sparks and rings glow.
    spark: Handle<StandardMaterial>,
    hurt: Handle<StandardMaterial>,
    spell: Handle<StandardMaterial>,
    block: Handle<StandardMaterial>,
    death: Handle<StandardMaterial>,
    hazard: Handle<StandardMaterial>,
    // Chunky bits (dust) are ordinary lit-looking grey.
    dust: Handle<StandardMaterial>,
}

/// Cheap deterministic randomness for visuals only.
#[derive(Resource)]
struct VfxRng(u32);

impl Default for VfxRng {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl VfxRng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// A spark or a puff. Streaks stretch along their velocity.
#[derive(Component)]
struct Particle {
    vel: Vec3,
    life: f32,
    max: f32,
    gravity: f32,
    size: f32,
    /// Streak length per unit of speed (0 = a plain cube).
    stretch: f32,
}

/// An expanding, fading ring.
#[derive(Component)]
struct Ring {
    life: f32,
    max: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
    colour: LinearRgba,
}

/// A dash afterimage.
#[derive(Component)]
struct Ghost {
    life: f32,
    max: f32,
    material: Handle<StandardMaterial>,
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mut glow = |r: f32, g: f32, b: f32, k: f32| {
        mats.add(StandardMaterial {
            base_color: Color::linear_rgb(r * k, g * k, b * k),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        })
    };
    let spark = glow(1.0, 0.85, 0.5, 3.0);
    let hurt = glow(1.0, 0.18, 0.12, 3.0);
    let spell = glow(1.0, 0.5, 0.15, 3.0);
    let block = glow(0.5, 0.8, 1.0, 3.0);
    let death = glow(1.0, 0.45, 0.25, 3.0);
    let hazard = glow(0.75, 0.3, 1.0, 3.0);
    let dust = mats.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.6, 0.58),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.insert_resource(VfxAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        // A thin torus in the XY plane, facing the camera.
        ring: meshes.add(
            ring_mesh(1.0, 0.045, 36, 6)
                .transformed(Mat4::from_rotation_x(FRAC_PI_2))
                .to_mesh(),
        ),
        spark,
        hurt,
        spell,
        block,
        death,
        hazard,
        dust,
    });
}

/// `count` sparks fanning out around `dir` (radians) by up to `spread`.
#[allow(clippy::too_many_arguments)]
fn sparks(
    commands: &mut Commands,
    a: &VfxAssets,
    rng: &mut VfxRng,
    pos: Vec2,
    dir: f32,
    spread: f32,
    count: u32,
    speed: (f32, f32),
    mat: &Handle<StandardMaterial>,
    life: f32,
    size: f32,
    gravity: f32,
) {
    for _ in 0..count {
        let ang = dir + rng.range(-spread, spread);
        let sp = rng.range(speed.0, speed.1);
        let vel = Vec3::new(ang.cos() * sp, ang.sin() * sp, rng.range(-1.0, 1.0));
        let l = life * rng.range(0.6, 1.0);
        commands.spawn((
            Particle {
                vel,
                life: l,
                max: l,
                gravity,
                size,
                stretch: 0.055,
            },
            NotShadowCaster,
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.x, pos.y, 0.7).with_scale(Vec3::new(
                size,
                size * 0.35,
                size * 0.35,
            )),
        ));
    }
}

/// Chunky bits: dust and debris (plain cubes).
#[allow(clippy::too_many_arguments)]
fn chunks(
    commands: &mut Commands,
    a: &VfxAssets,
    rng: &mut VfxRng,
    pos: Vec2,
    count: u32,
    speed: f32,
    mat: &Handle<StandardMaterial>,
    life: f32,
    size: f32,
    gravity: f32,
) {
    for _ in 0..count {
        let ang = rng.next() * std::f32::consts::TAU;
        let sp = speed * (0.4 + 0.6 * rng.next());
        let vel = Vec3::new(ang.cos() * sp, ang.sin() * sp, (rng.next() - 0.5) * 2.0);
        let l = life * (0.6 + 0.4 * rng.next());
        commands.spawn((
            Particle {
                vel,
                life: l,
                max: l,
                gravity,
                size,
                stretch: 0.0,
            },
            NotShadowCaster,
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.x, pos.y, 0.6).with_scale(Vec3::splat(size)),
        ));
    }
}

/// An expanding ring of light at `pos`.
fn ring(
    commands: &mut Commands,
    a: &VfxAssets,
    mats: &mut Assets<StandardMaterial>,
    pos: Vec2,
    radius: f32,
    colour: LinearRgba,
    life: f32,
) {
    let material = mats.add(StandardMaterial {
        base_color: Color::linear_rgba(colour.red, colour.green, colour.blue, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Ring {
            life,
            max: life,
            radius,
            material: material.clone(),
            colour,
        },
        NotShadowCaster,
        Mesh3d(a.ring.clone()),
        MeshMaterial3d(material),
        Transform::from_xyz(pos.x, pos.y, 0.8).with_scale(Vec3::splat(radius * 0.3)),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_hit_vfx(
    mut commands: Commands,
    a: Res<VfxAssets>,
    mut rng: ResMut<VfxRng>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut hits: MessageReader<Hit>,
    mut blocked: MessageReader<Blocked>,
    mut enemy_died: MessageReader<EnemyDied>,
    mut player_died: MessageReader<PlayerDied>,
    player: Query<&SimPos, With<Player>>,
) {
    for h in hits.read() {
        let along = if h.dir >= 0 {
            0.0
        } else {
            std::f32::consts::PI
        };
        match (h.victim_team, h.kind) {
            (Team::Player, _) => {
                sparks(
                    &mut commands,
                    &a,
                    &mut rng,
                    h.pos,
                    0.0,
                    std::f32::consts::PI,
                    16,
                    (5.0, 13.0),
                    &a.hurt,
                    0.4,
                    0.34,
                    8.0,
                );
                ring(
                    &mut commands,
                    &a,
                    &mut mats,
                    h.pos,
                    1.5,
                    LinearRgba::rgb(3.0, 0.5, 0.35),
                    0.35,
                );
            }
            (Team::Hazard, _) => {
                sparks(
                    &mut commands,
                    &a,
                    &mut rng,
                    h.pos,
                    FRAC_PI_2,
                    1.2,
                    8,
                    (4.0, 9.0),
                    &a.hazard,
                    0.3,
                    0.26,
                    6.0,
                );
                ring(
                    &mut commands,
                    &a,
                    &mut mats,
                    h.pos,
                    0.9,
                    LinearRgba::rgb(2.2, 0.9, 3.0),
                    0.25,
                );
            }
            (_, HitKind::Spell) => {
                sparks(
                    &mut commands,
                    &a,
                    &mut rng,
                    h.pos,
                    along,
                    1.6,
                    12,
                    (5.0, 12.0),
                    &a.spell,
                    0.32,
                    0.3,
                    5.0,
                );
                ring(
                    &mut commands,
                    &a,
                    &mut mats,
                    h.pos,
                    1.3,
                    LinearRgba::rgb(3.2, 1.6, 0.4),
                    0.3,
                );
            }
            _ => {
                // The nail: white-hot streaks thrown the way the victim is
                // pushed, or up off a pogo, and a small ring at the contact.
                let (dir, spread) = match h.attack_dir {
                    hk_sim::combat::AttackDir::Down => (FRAC_PI_2, 1.1),
                    hk_sim::combat::AttackDir::Up => (FRAC_PI_2, 0.9),
                    hk_sim::combat::AttackDir::Forward => (along, 0.75),
                };
                sparks(
                    &mut commands,
                    &a,
                    &mut rng,
                    h.pos,
                    dir,
                    spread,
                    9,
                    (7.0, 16.0),
                    &a.spark,
                    0.26,
                    0.3,
                    6.0,
                );
                ring(
                    &mut commands,
                    &a,
                    &mut mats,
                    h.pos,
                    0.85,
                    LinearRgba::rgb(3.0, 2.6, 1.6),
                    0.2,
                );
                if h.attack_dir == hk_sim::combat::AttackDir::Down {
                    // A pogo: a wider ring, so the bounce reads.
                    ring(
                        &mut commands,
                        &a,
                        &mut mats,
                        h.pos,
                        1.5,
                        LinearRgba::rgb(1.4, 2.4, 3.2),
                        0.3,
                    );
                }
            }
        }
    }
    for b in blocked.read() {
        let along = if b.dir >= 0 {
            0.0
        } else {
            std::f32::consts::PI
        };
        sparks(
            &mut commands,
            &a,
            &mut rng,
            b.pos,
            along,
            0.9,
            9,
            (5.0, 11.0),
            &a.block,
            0.24,
            0.26,
            2.0,
        );
        ring(
            &mut commands,
            &a,
            &mut mats,
            b.pos,
            1.0,
            LinearRgba::rgb(1.4, 2.4, 3.4),
            0.25,
        );
    }
    for d in enemy_died.read() {
        chunks(
            &mut commands,
            &a,
            &mut rng,
            d.pos,
            10,
            8.0,
            &a.dust,
            0.5,
            0.16,
            10.0,
        );
        sparks(
            &mut commands,
            &a,
            &mut rng,
            d.pos,
            0.0,
            std::f32::consts::PI,
            22,
            (4.0, 12.0),
            &a.death,
            0.6,
            0.3,
            9.0,
        );
        ring(
            &mut commands,
            &a,
            &mut mats,
            d.pos,
            2.4,
            LinearRgba::rgb(3.2, 1.4, 0.6),
            0.45,
        );
    }
    for _ in player_died.read() {
        if let Ok(p) = player.single() {
            sparks(
                &mut commands,
                &a,
                &mut rng,
                p.0,
                0.0,
                std::f32::consts::PI,
                34,
                (5.0, 14.0),
                &a.spark,
                0.9,
                0.34,
                6.0,
            );
            ring(
                &mut commands,
                &a,
                &mut mats,
                p.0,
                3.0,
                LinearRgba::rgb(3.0, 2.6, 2.0),
                0.7,
            );
        }
    }
}

/// Dust when landing, sparks down a wall you are sliding on, afterimages while
/// dashing.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn dust_and_trails(
    mut commands: Commands,
    a: Res<VfxAssets>,
    knight: Option<Res<KnightAssets>>,
    mut rng: ResMut<VfxRng>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut prev: Local<(bool, f32, f32)>,
    player: Query<(&Transform, &Motor, &Velocity, &PlayerState, &Aabb, &Facing), With<Player>>,
) {
    let Ok((t, motor, vel, state, aabb, facing)) = player.single() else {
        return;
    };
    let dt = time.delta_secs();
    let feet = Vec2::new(t.translation.x, t.translation.y - aabb.half.y);
    if motor.grounded && !prev.0 && prev.1 < -5.0 {
        let n = (prev.1.abs() / 3.0).clamp(4.0, 12.0) as u32;
        chunks(
            &mut commands,
            &a,
            &mut rng,
            feet,
            n,
            3.5,
            &a.dust,
            0.35,
            0.16,
            -1.0,
        );
    }
    // Sliding down a wall: a trickle of sparks from the hands.
    if *state == PlayerState::WallSlide {
        prev.2 -= dt;
        if prev.2 <= 0.0 {
            prev.2 = 0.05;
            let at = Vec2::new(
                t.translation.x + motor.wall as f32 * 0.42,
                t.translation.y + 0.2,
            );
            sparks(
                &mut commands,
                &a,
                &mut rng,
                at,
                -FRAC_PI_2 - 0.4 * motor.wall as f32,
                0.5,
                2,
                (1.5, 3.5),
                &a.spark,
                0.25,
                0.16,
                6.0,
            );
        }
    }
    // Dash: a ghost of the knight every few frames, fading to nothing.
    if *state == PlayerState::Dash {
        prev.2 -= dt;
        if prev.2 <= 0.0 {
            prev.2 = 0.03;
            if let Some(k) = &knight {
                spawn_ghost(&mut commands, k, &mut mats, feet, facing.0);
            }
        }
    }
    *prev = (motor.grounded, vel.y, prev.2);
}

fn spawn_ghost(
    commands: &mut Commands,
    k: &KnightAssets,
    mats: &mut Assets<StandardMaterial>,
    feet: Vec2,
    facing: i8,
) {
    let material = mats.add(StandardMaterial {
        base_color: Color::linear_rgba(0.35, 0.9, 1.2, 0.55),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    let yaw = if facing >= 0 {
        -0.3
    } else {
        std::f32::consts::PI + 0.3
    };
    let life = 0.28;
    commands
        .spawn((
            Ghost {
                life,
                max: life,
                material: material.clone(),
            },
            Transform::from_xyz(feet.x, feet.y, -0.1).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                NotShadowCaster,
                Mesh3d(k.cloak_mesh()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.45, 0.0),
            ));
            p.spawn((
                NotShadowCaster,
                Mesh3d(k.helm_mesh()),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 0.92, 0.0),
            ));
        });
}

fn animate_particles(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= p.gravity * dt;
        t.translation += p.vel * dt;
        let k = p.life / p.max;
        if p.stretch > 0.0 {
            // A streak: long along its velocity, thin, shrinking away.
            let speed = p.vel.truncate().length();
            t.rotation = Quat::from_rotation_z(p.vel.y.atan2(p.vel.x));
            let len = (p.size * 0.5 + speed * p.stretch) * (0.4 + 0.6 * k);
            t.scale = Vec3::new(len, p.size * 0.22 * k.max(0.2), p.size * 0.22 * k.max(0.2));
        } else {
            t.scale = Vec3::splat(p.size * k);
        }
    }
}

fn animate_rings(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Ring, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut r, mut t) in &mut q {
        r.life -= dt;
        if r.life <= 0.0 {
            mats.remove(&r.material);
            commands.entity(e).despawn();
            continue;
        }
        let age = 1.0 - r.life / r.max;
        let grow = 1.0 - (1.0 - age).powi(3);
        t.scale = Vec3::splat(r.radius * (0.3 + 0.7 * grow));
        if let Some(m) = mats.get_mut(&r.material) {
            let a = (1.0 - age).powi(2);
            m.base_color = Color::linear_rgba(r.colour.red, r.colour.green, r.colour.blue, a);
        }
    }
}

fn animate_ghosts(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Ghost)>,
) {
    let dt = time.delta_secs();
    for (e, mut g) in &mut q {
        g.life -= dt;
        if g.life <= 0.0 {
            mats.remove(&g.material);
            commands.entity(e).despawn();
            continue;
        }
        let k = g.life / g.max;
        if let Some(m) = mats.get_mut(&g.material) {
            m.base_color = Color::linear_rgba(0.35, 0.9, 1.2, 0.5 * k * k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_source_is_in_range_and_varies() {
        let mut r = VfxRng::default();
        let vals: Vec<f32> = (0..1000).map(|_| r.range(-2.0, 3.0)).collect();
        assert!(vals.iter().all(|v| (-2.0..3.0).contains(v)));
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        assert!((mean - 0.5).abs() < 0.3, "mean {mean}");
    }
}
