//! Juice: particles for hits, blocks, deaths, landings and dashes, plus
//! Purely visual: nothing here feeds back
//! into the simulation.

use bevy::prelude::*;
use hk_sim::combat::{Blocked, EnemyDied, Hit, HitKind, PlayerDied, Team};
use hk_sim::components::{Aabb, SimPos, Velocity};
use hk_sim::player::{Motor, Player, PlayerState};

use crate::interp::RenderPrepSet;

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VfxRng>()
            .add_systems(Startup, setup_assets)
            .add_systems(
                Update,
                (spawn_hit_vfx, dust_and_trails, animate_particles).after(RenderPrepSet),
            );
    }
}

#[derive(Resource)]
struct VfxAssets {
    cube: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    hurt: Handle<StandardMaterial>,
    spell: Handle<StandardMaterial>,
    block: Handle<StandardMaterial>,
    death: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    dash: Handle<StandardMaterial>,
    hazard: Handle<StandardMaterial>,
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
}

#[derive(Component)]
struct Particle {
    vel: Vec3,
    life: f32,
    max: f32,
    gravity: f32,
    size: f32,
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mut glow = |r: f32, g: f32, b: f32, k: f32| {
        mats.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            emissive: LinearRgba::rgb(r * k, g * k, b * k),
            unlit: true,
            ..default()
        })
    };
    let assets = VfxAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        spark: glow(1.0, 0.95, 0.7, 3.0),
        hurt: glow(1.0, 0.25, 0.2, 3.0),
        spell: glow(1.0, 0.6, 0.2, 3.0),
        block: glow(0.6, 0.85, 1.0, 3.0),
        death: glow(1.0, 0.5, 0.3, 3.0),
        dust: glow(0.55, 0.55, 0.6, 0.4),
        dash: glow(0.4, 0.9, 1.0, 2.5),
        hazard: glow(0.8, 0.3, 1.0, 3.0),
    };
    commands.insert_resource(assets);
}

fn burst(
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
            },
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.x, pos.y, 0.6).with_scale(Vec3::splat(size)),
        ));
    }
}

fn spawn_hit_vfx(
    mut commands: Commands,
    a: Res<VfxAssets>,
    mut rng: ResMut<VfxRng>,
    mut hits: MessageReader<Hit>,
    mut blocked: MessageReader<Blocked>,
    mut enemy_died: MessageReader<EnemyDied>,
    mut player_died: MessageReader<PlayerDied>,
    player: Query<&SimPos, With<Player>>,
) {
    for h in hits.read() {
        match (h.victim_team, h.kind) {
            (Team::Player, _) => burst(
                &mut commands,
                &a,
                &mut rng,
                h.pos,
                14,
                8.0,
                &a.hurt,
                0.35,
                0.16,
                8.0,
            ),
            (Team::Hazard, _) => burst(
                &mut commands,
                &a,
                &mut rng,
                h.pos,
                8,
                6.0,
                &a.hazard,
                0.3,
                0.12,
                6.0,
            ),
            (_, HitKind::Spell) => burst(
                &mut commands,
                &a,
                &mut rng,
                h.pos,
                10,
                7.0,
                &a.spell,
                0.3,
                0.14,
                4.0,
            ),
            _ => burst(
                &mut commands,
                &a,
                &mut rng,
                h.pos,
                9,
                7.0,
                &a.spark,
                0.25,
                0.13,
                6.0,
            ),
        }
    }
    for b in blocked.read() {
        burst(
            &mut commands,
            &a,
            &mut rng,
            b.pos,
            7,
            6.0,
            &a.block,
            0.22,
            0.12,
            2.0,
        );
    }
    for d in enemy_died.read() {
        burst(
            &mut commands,
            &a,
            &mut rng,
            d.pos,
            22,
            9.0,
            &a.death,
            0.55,
            0.2,
            10.0,
        );
    }
    for _ in player_died.read() {
        if let Ok(p) = player.single() {
            burst(
                &mut commands,
                &a,
                &mut rng,
                p.0,
                34,
                10.0,
                &a.spark,
                0.9,
                0.22,
                6.0,
            );
        }
    }
}

/// Puffs of dust when landing, and a streak while dashing.
fn dust_and_trails(
    mut commands: Commands,
    a: Res<VfxAssets>,
    mut rng: ResMut<VfxRng>,
    mut prev: Local<(bool, f32)>,
    player: Query<(&Transform, &Motor, &Velocity, &PlayerState, &Aabb), With<Player>>,
) {
    let Ok((t, motor, vel, state, aabb)) = player.single() else {
        return;
    };
    let feet = Vec2::new(t.translation.x, t.translation.y - aabb.half.y);
    if motor.grounded && !prev.0 && prev.1 < -5.0 {
        let n = (prev.1.abs() / 3.0).clamp(4.0, 12.0) as u32;
        burst(
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
    if *state == PlayerState::Dash {
        burst(
            &mut commands,
            &a,
            &mut rng,
            t.translation.truncate(),
            1,
            0.5,
            &a.dash,
            0.25,
            0.4,
            0.0,
        );
    }
    *prev = (motor.grounded, vel.y);
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
        t.scale = Vec3::splat(p.size * (p.life / p.max));
    }
}
