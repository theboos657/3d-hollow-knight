//! Sandbox room for feeling the controller and combat: wall-jump shaft,
//! one-way platforms, punching-bag dummies, a spike pit with a pogo target.
//! Uses the real simulation; only the placement and drawing live here.
//! (The proper room loader and camera rig arrive in M4/M6.)

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use hk_sim::combat::*;
use hk_sim::components::{Aabb, PrevPos, SimPos, Velocity};
use hk_sim::enemy::{spawn_enemy, Brain, EnemyKind, EnemyState};
use hk_sim::player::{spawn_player, Abilities, Facing, Player, PlayerState};
use hk_sim::world::{Tile, TileGrid};
use hk_sim::SimTick;

use crate::interp::{Interpolated, RenderPrepSet};
use crate::scene::{spawn_backdrop, MainCamera, Palette, CAM_DIST, FOV_DEG};

const W: i32 = 64;
const H: i32 = 26;
const START: Vec2 = Vec2::new(4.0, 2.0 + 0.75 + 0.001);

pub struct SandboxPlugin;

impl Plugin for SandboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, (build_sandbox, spawn_hud))
            .add_systems(
                Update,
                (
                    attach_body_visuals,
                    attach_hit_visuals,
                    attach_player_extras,
                    update_nose,
                    player_fx,
                    update_hud,
                    enemy_fx,
                    update_shield_plates,
                    respawn_enemies,
                )
                    .after(RenderPrepSet),
            )
            .add_systems(Update, camera_follow.after(RenderPrepSet));
    }
}

fn fill(g: &mut TileGrid, x: i32, y: i32, w: i32, h: i32, t: Tile) {
    for j in y..y + h {
        for i in x..x + w {
            g.set(i, j, t);
        }
    }
}

fn build_grid() -> TileGrid {
    let mut g = TileGrid::new(W, H);
    fill(&mut g, 0, 0, W, 2, Tile::Solid); // floor
    fill(&mut g, 0, 0, 1, H, Tile::Solid); // left wall
    fill(&mut g, W - 1, 0, 1, H, Tile::Solid); // right wall
    fill(&mut g, 0, H - 1, W, 1, Tile::Solid); // ceiling

    // Wall-jump shaft: two walls 4 apart, then a ledge off the top.
    fill(&mut g, 12, 2, 1, 14, Tile::Solid);
    fill(&mut g, 17, 2, 1, 14, Tile::Solid);
    fill(&mut g, 18, 15, 6, 1, Tile::Solid);

    // One-way platforms (jump up through them, Down+Jump to drop).
    fill(&mut g, 26, 5, 5, 1, Tile::OneWay);
    fill(&mut g, 32, 8, 5, 1, Tile::OneWay);

    // Spike pit: the floor drops one tile for 10 tiles.
    fill(&mut g, 42, 1, 10, 1, Tile::Empty);

    // Ledge on the far side.
    fill(&mut g, 56, 5, 5, 1, Tile::Solid);
    g
}

fn spawn_level_meshes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    pal: &Palette,
    grid: &TileGrid,
) {
    for j in 0..grid.height() {
        let mut i = 0;
        while i < grid.width() {
            let t = grid.get(i, j);
            if !matches!(t, Tile::Solid | Tile::OneWay) {
                i += 1;
                continue;
            }
            let start = i;
            while i < grid.width() && grid.get(i, j) == t {
                i += 1;
            }
            let w = (i - start) as f32;
            let (h, y_off, mat) = match t {
                Tile::OneWay => (0.25, 0.875, pal.one_way.clone()),
                _ => (1.0, 0.5, pal.stone.clone()),
            };
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, h, 4.0))),
                MeshMaterial3d(mat),
                Transform::from_xyz(start as f32 + w * 0.5, j as f32 + y_off, 0.0),
            ));
        }
    }
}

/// What can stand at a sandbox spawn point. `Dummy` is a passive punching bag.
#[derive(Clone, Copy)]
enum Spawn {
    Dummy,
    Enemy(EnemyKind),
}

/// Sandbox spawn table. The index is the `SpawnTag`, so a dead enemy comes
/// back at the same spot.
const SPAWNS: &[(Spawn, Vec2)] = &[
    (Spawn::Dummy, Vec2::new(8.5, 2.6)),   // 0 ground bag
    (Spawn::Dummy, Vec2::new(22.0, 12.0)), // 1 floating (up-slash)
    (Spawn::Dummy, Vec2::new(47.0, 5.5)),  // 2 over the pit (pogo)
    (Spawn::Dummy, Vec2::new(58.0, 6.6)),  // 3 far ledge
    (Spawn::Enemy(EnemyKind::Husk), Vec2::new(14.0, 2.7)), // 4 husk near the start
    (Spawn::Enemy(EnemyKind::Shieldbearer), Vec2::new(30.0, 2.9)), // 5
    (Spawn::Enemy(EnemyKind::Wisp), Vec2::new(36.0, 9.0)), // 6
    (Spawn::Enemy(EnemyKind::Spitter), Vec2::new(57.0, 2.7)), // 7
];

fn spawn_at(commands: &mut Commands, tag: u32) {
    let (kind, pos) = SPAWNS[tag as usize];
    match kind {
        Spawn::Dummy => {
            let half = Vec2::new(0.5, 0.6);
            commands.spawn((
                SpawnTag(tag),
                SimPos(pos),
                PrevPos(pos),
                Velocity::default(),
                Aabb { half },
                Hurtbox {
                    half,
                    team: Team::Enemy,
                },
                Health::full(40),
                Pogoable,
            ));
        }
        Spawn::Enemy(k) => {
            commands.queue(move |world: &mut World| {
                let e = spawn_enemy(world, k, pos);
                world.entity_mut(e).insert(SpawnTag(tag));
            });
        }
    }
}

fn build_sandbox(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, pal: Res<Palette>) {
    let grid = build_grid();
    spawn_level_meshes(&mut commands, &mut meshes, &pal, &grid);
    spawn_backdrop(&mut commands, &mut meshes, &pal, W as f32);
    commands.insert_resource(grid);
    commands.insert_resource(RespawnPoint(START));

    // Everything unlocked so all moves can be tried.
    commands.queue(|world: &mut World| {
        spawn_player(
            world,
            START,
            Abilities {
                dash: true,
                wall_grip: true,
            },
        );
    });

    for tag in 0..SPAWNS.len() as u32 {
        spawn_at(&mut commands, tag);
    }

    // Spikes on the pit floor. Hurt on touch, and pogo-able from above.
    let spikes = commands.spawn_empty().id();
    let half = Vec2::new(5.0, 0.25);
    let pos = Vec2::new(47.0, 1.25);
    commands.entity(spikes).insert((
        SimPos(pos),
        PrevPos(pos),
        Hitbox {
            half,
            team: Team::Hazard,
            damage: 1,
            kind: HitKind::Hazard,
            attack_dir: AttackDir::Forward,
            once: false,
            owner: spikes,
        },
        Hurtbox {
            half,
            team: Team::Hazard,
        },
        Pogoable,
    ));
}

// ---------------------------------------------------------------- visuals --

fn kind_color(kind: EnemyKind) -> Color {
    match kind {
        EnemyKind::Husk => Color::srgb(0.7, 0.3, 0.2),
        EnemyKind::Wisp => Color::srgb(0.6, 0.35, 0.9),
        EnemyKind::Shieldbearer => Color::srgb(0.25, 0.55, 0.6),
        EnemyKind::Spitter => Color::srgb(0.4, 0.7, 0.3),
    }
}

#[derive(Component)]
struct ShieldPlate;

/// Bodies (player, dummies, enemies, spikes) get a box the size of their
/// hurt/collision box. Real enemies get their own material so state tints work.
#[allow(clippy::type_complexity)]
fn attach_body_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pal: Res<Palette>,
    q: Query<
        (
            Entity,
            &Hurtbox,
            Option<&Aabb>,
            &SimPos,
            Option<&Brain>,
            Has<Guard>,
        ),
        Added<Hurtbox>,
    >,
) {
    for (e, hu, aabb, pos, brain, guard) in &q {
        let half = aabb.map_or(hu.half, |a| a.half);
        let (mesh, mat) = if let Some(b) = brain {
            let mesh = if b.kind == EnemyKind::Wisp {
                meshes.add(Sphere::new(half.x))
            } else {
                meshes.add(Cuboid::new(half.x * 2.0, half.y * 2.0, 0.8))
            };
            let m = mats.add(StandardMaterial {
                base_color: kind_color(b.kind),
                ..default()
            });
            (mesh, m)
        } else {
            let mat = match hu.team {
                Team::Player => &pal.player,
                Team::Enemy => &pal.enemy,
                Team::Hazard => &pal.hazard,
            };
            (
                meshes.add(Cuboid::new(half.x * 2.0, half.y * 2.0, 0.8)),
                mat.clone(),
            )
        };
        commands.entity(e).insert((
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
        if guard {
            commands.entity(e).with_children(|p| {
                p.spawn((
                    ShieldPlate,
                    Mesh3d(meshes.add(Cuboid::new(0.18, half.y * 1.7, 1.1))),
                    MeshMaterial3d(pal.player.clone()),
                    Transform::from_xyz(half.x + 0.1, 0.0, 0.1),
                ));
            });
        }
    }
}

/// The shield plate sits on the side the Shieldbearer is guarding.
fn update_shield_plates(
    guards: Query<(&Guard, &Children)>,
    mut plates: Query<&mut Transform, With<ShieldPlate>>,
) {
    for (g, children) in &guards {
        for c in children.iter() {
            if let Ok(mut t) = plates.get_mut(c) {
                t.translation.x = 0.65 * g.facing as f32;
            }
        }
    }
}

/// Telegraph colours: what an enemy is doing must be readable at a glance.
fn enemy_fx(
    tick: Res<SimTick>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(&Brain, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (b, handle) in &q {
        let Some(m) = mats.get_mut(&handle.0) else {
            continue;
        };
        let base = kind_color(b.kind).to_linear();
        m.emissive = match b.state {
            EnemyState::Idle => LinearRgba::rgb(base.red * 0.1, base.green * 0.1, base.blue * 0.1),
            EnemyState::Chase => LinearRgba::rgb(base.red * 0.3, base.green * 0.3, base.blue * 0.3),
            EnemyState::Notice => LinearRgba::rgb(2.0, 1.8, 0.2),
            EnemyState::Windup if (tick.0 / 4) & 1 == 0 => LinearRgba::rgb(4.0, 2.4, 0.4),
            EnemyState::Windup => LinearRgba::rgb(1.6, 0.8, 0.1),
            EnemyState::Attack => LinearRgba::rgb(4.0, 0.3, 0.3),
            EnemyState::Recover => LinearRgba::rgb(0.1, 0.35, 1.4),
            EnemyState::Stagger => LinearRgba::rgb(2.0, 2.0, 2.0),
        };
    }
}

/// Slash boxes and projectiles are drawn as translucent boxes.
#[allow(clippy::type_complexity)]
fn attach_hit_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    pal: Res<Palette>,
    q: Query<(Entity, &Hitbox, &SimPos), (Added<Hitbox>, Without<Hurtbox>)>,
) {
    for (e, hb, pos) in &q {
        let mat = match hb.kind {
            HitKind::Spell => &pal.bolt,
            HitKind::Projectile => &pal.hazard,
            _ => &pal.slash,
        };
        commands.entity(e).insert((
            Mesh3d(meshes.add(Cuboid::new(hb.half.x * 2.0, hb.half.y * 2.0, 0.6))),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.4),
            Interpolated {
                z: 0.4,
                offset: Vec2::ZERO,
            },
        ));
    }
}

#[derive(Component)]
struct Nose;

/// Lantern light and a "nose" block that shows which way the player faces.
fn attach_player_extras(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    pal: Res<Palette>,
    q: Query<Entity, Added<Player>>,
) {
    for e in &q {
        commands.entity(e).with_children(|p| {
            p.spawn((
                PointLight {
                    intensity: 900_000.0,
                    range: 24.0,
                    color: Color::srgb(1.0, 0.85, 0.6),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.6, 2.5),
            ));
            p.spawn((
                Nose,
                Mesh3d(meshes.add(Cuboid::new(0.3, 0.2, 0.5))),
                MeshMaterial3d(pal.marker.clone()),
                Transform::from_xyz(0.32, 0.35, 0.5),
            ));
        });
    }
}

fn update_nose(
    player: Query<(&Facing, &Children), With<Player>>,
    mut nose: Query<&mut Transform, With<Nose>>,
) {
    for (facing, children) in &player {
        for c in children.iter() {
            if let Ok(mut t) = nose.get_mut(c) {
                t.translation.x = 0.32 * facing.0 as f32;
            }
        }
    }
}

/// State colouring and i-frame flicker for the player.
fn player_fx(
    tick: Res<SimTick>,
    pal: Res<Palette>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&PlayerState, Has<Invulnerable>, &mut Visibility), With<Player>>,
) {
    let Ok((state, invuln, mut vis)) = q.single_mut() else {
        return;
    };
    if let Some(m) = mats.get_mut(&pal.player) {
        m.emissive = match state {
            PlayerState::Focus => LinearRgba::rgb(0.2, 1.4, 0.5),
            PlayerState::Dash => LinearRgba::rgb(0.6, 1.8, 3.0),
            PlayerState::Hurt => LinearRgba::rgb(2.5, 0.2, 0.2),
            PlayerState::WallSlide => LinearRgba::rgb(1.4, 1.1, 0.2),
            PlayerState::Dead => LinearRgba::rgb(0.0, 0.0, 0.0),
            _ => LinearRgba::rgb(0.1, 0.1, 0.16),
        };
    }
    *vis = if invuln && (tick.0 / 6) & 1 == 0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
}

// -------------------------------------------------------------------- HUD --

#[derive(Component)]
struct Hud;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: 20.0,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.95, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(14.0),
            ..default()
        },
    ));
    commands.spawn((
        Text::new(
            "Move: Arrows/WASD   Jump: Space/Z   Attack: X/J (+Up / +Down in air = pogo)\n\
             Dash: C/Shift   Focus (hold, heals): F   Bolt: V   Down+Jump: drop through   F1: debug",
        ),
        TextFont {
            font_size: 15.0,
            ..default()
        },
        TextColor(Color::srgba(0.8, 0.85, 1.0, 0.75)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

fn update_hud(
    player: Query<(&Health, &Soul, &CombatState), With<Player>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let (Ok((hp, soul, cs)), Ok(mut text)) = (player.single(), hud.single_mut()) else {
        return;
    };
    let masks: String = (0..hp.max)
        .map(|i| if i < hp.hp { '#' } else { '-' })
        .collect();
    let status = if cs.dead {
        "  YOU DIED"
    } else if cs.focusing {
        "  focusing..."
    } else {
        ""
    };
    **text = format!("HP [{masks}]   SOUL {}/{}{status}", soul.value, soul.max);
}

// ----------------------------------------------------------------- camera --

fn camera_follow(
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Transform, (With<Player>, Without<MainCamera>)>,
    mut cam: Query<&mut Transform, With<MainCamera>>,
) {
    let (Ok(p), Ok(mut c), Ok(w)) = (player.single(), cam.single_mut(), windows.single()) else {
        return;
    };
    let half_h = CAM_DIST * (FOV_DEG.to_radians() * 0.5).tan();
    let half_w = half_h * (w.width() / w.height().max(1.0));
    let clamp = |v: f32, half: f32, max: f32| {
        if max <= half * 2.0 {
            max * 0.5
        } else {
            v.clamp(half, max - half)
        }
    };
    let target = Vec2::new(
        clamp(p.translation.x, half_w, W as f32),
        clamp(p.translation.y + 2.0, half_h, H as f32),
    );
    let k = 1.0 - (-8.0 * time.delta_secs()).exp();
    c.translation.x += (target.x - c.translation.x) * k;
    c.translation.y += (target.y - c.translation.y) * k;
}

// -------------------------------------------------------------- housekeeping --

/// Anything killed comes back at its spawn point a few seconds later.
fn respawn_enemies(
    time: Res<Time>,
    mut died: MessageReader<EnemyDied>,
    mut pending: Local<Vec<(f32, u32)>>,
    mut commands: Commands,
) {
    for d in died.read() {
        if let Some(tag) = d.tag {
            pending.push((4.0, tag));
        }
    }
    let dt = time.delta_secs();
    pending.retain_mut(|(t, tag)| {
        *t -= dt;
        if *t <= 0.0 {
            spawn_at(&mut commands, *tag);
            false
        } else {
            true
        }
    });
}
