//! Boss presentation: the body and its state tells, warning glyphs, swinging
//! bells and their chains, the health bar, the sealed-exit tint and the camera
//! shake for a boss's big moments. All gameplay lives in `hk_sim::boss`.

use bevy::prelude::*;
use hk_sim::boss::{
    ArenaLock, Boss, BossAwoke, BossBrain, BossDefeated, BossPhaseChanged, BossState, Glyph,
    Pendulum,
};
use hk_sim::combat::Health;
use hk_sim::components::{Aabb, SimPos};
use hk_sim::tuning::Tuning;
use hk_sim::world::room::RoomExit;
use hk_sim::SimTick;

use crate::camera_rig::Rig;
use crate::interp::{Interpolated, RenderPrepSet};

pub struct BossViewPlugin;

impl Plugin for BossViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_boss_bar).add_systems(
            Update,
            (
                attach_boss_visuals,
                update_boss_eyes,
                boss_fx,
                attach_glyph_visuals,
                glyph_fx,
                attach_pendulum_visuals,
                update_chains,
                update_boss_bar,
                exit_lock_tint,
                boss_shake,
            )
                .after(RenderPrepSet),
        );
    }
}

// ------------------------------------------------------------------- body --

struct Look {
    body: Color,
    trim: Color,
    eye: Color,
    eye_glow: LinearRgba,
}

fn look(id: &str) -> Look {
    match id {
        // The Matron: a hunched, mossy brute with acid-green eyes.
        "matron" => Look {
            body: Color::srgb(0.30, 0.38, 0.22),
            trim: Color::srgb(0.16, 0.21, 0.12),
            eye: Color::srgb(0.7, 1.0, 0.4),
            eye_glow: LinearRgba::rgb(1.4, 3.2, 0.6),
        },
        // The Bellwarden: a bronze bell-keeper with furnace-orange eyes.
        _ => Look {
            body: Color::srgb(0.52, 0.36, 0.16),
            trim: Color::srgb(0.30, 0.20, 0.08),
            eye: Color::srgb(1.0, 0.7, 0.3),
            eye_glow: LinearRgba::rgb(3.4, 1.8, 0.4),
        },
    }
}

#[derive(Component)]
struct BossEyes {
    reach: f32,
}

/// A boss is a big box with a domed "bell" crown and two glowing eyes that
/// look the way it faces. It owns its material so state tints don't leak.
fn attach_boss_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &Boss, &Aabb, &SimPos), Added<Boss>>,
) {
    for (e, boss, aabb, pos) in &q {
        let l = look(&boss.id);
        let h = aabb.half;
        let body = mats.add(StandardMaterial {
            base_color: l.body,
            perceptual_roughness: 0.65,
            metallic: 0.25,
            ..default()
        });
        let trim = mats.add(StandardMaterial {
            base_color: l.trim,
            perceptual_roughness: 0.8,
            ..default()
        });
        let eye = mats.add(StandardMaterial {
            base_color: l.eye,
            emissive: l.eye_glow,
            ..default()
        });
        commands
            .entity(e)
            .insert((
                Mesh3d(meshes.add(Cuboid::new(h.x * 2.0, h.y * 2.0, 1.8))),
                MeshMaterial3d(body),
                Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
                Interpolated {
                    z: 0.0,
                    offset: Vec2::ZERO,
                },
            ))
            .with_children(|p| {
                // Crown: a squashed dome sitting on the shoulders.
                p.spawn((
                    Mesh3d(meshes.add(Sphere::new(h.x * 0.9))),
                    MeshMaterial3d(trim.clone()),
                    Transform::from_xyz(0.0, h.y, 0.0).with_scale(Vec3::new(1.0, 0.55, 0.75)),
                ));
                // Belt band, to break up the silhouette.
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(h.x * 2.08, h.y * 0.28, 1.9))),
                    MeshMaterial3d(trim),
                    Transform::from_xyz(0.0, -h.y * 0.25, 0.0),
                ));
                for dy in [0.0, 0.32] {
                    p.spawn((
                        BossEyes { reach: h.x * 0.5 },
                        Mesh3d(meshes.add(Cuboid::new(0.42, 0.16, 0.3))),
                        MeshMaterial3d(eye.clone()),
                        Transform::from_xyz(-h.x * 0.5, h.y * (0.35 + dy), 0.95),
                    ));
                }
            });
    }
}

fn update_boss_eyes(
    bosses: Query<(&BossBrain, &Children)>,
    mut eyes: Query<(&BossEyes, &mut Transform)>,
) {
    for (b, children) in &bosses {
        for c in children.iter() {
            if let Ok((eye, mut t)) = eyes.get_mut(c) {
                t.translation.x = eye.reach * b.facing as f32;
            }
        }
    }
}

/// Tells: the same colour language as ordinary enemies, so it's learned once.
/// Amber flicker = an attack is coming, red = it is happening, blue = punish.
fn boss_fx(
    tick: Res<SimTick>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(&BossBrain, &MeshMaterial3d<StandardMaterial>)>,
) {
    let blink = (tick.0 / 4) & 1 == 0;
    let e = LinearRgba::rgb;
    for (b, handle) in &q {
        let Some(m) = mats.get_mut(&handle.0) else {
            continue;
        };
        let calm = 0.06 + 0.05 * b.phase as f32;
        m.emissive = match b.state {
            BossState::Sleeping => e(0.0, 0.0, 0.0),
            BossState::Intro | BossState::Transition => {
                let p = 0.5 + 0.5 * (tick.0 as f32 * 0.09).sin();
                e(2.0 * p + 0.3, 1.6 * p + 0.3, 0.6 * p)
            }
            BossState::Choose | BossState::Approach => e(calm, calm * 0.5, calm * 0.3),
            BossState::Telegraph if blink => e(4.0, 2.4, 0.4),
            BossState::Telegraph => e(1.6, 0.8, 0.1),
            BossState::Active => e(4.0, 0.3, 0.3),
            BossState::Recover => e(0.1, 0.35, 1.4),
            BossState::Dying => {
                let k = (b.timer as f32 / 240.0).min(1.0);
                e(1.0 + 3.0 * k, 1.0 + 3.0 * k, 1.0 + 3.0 * k)
            }
        };
    }
}

// ----------------------------------------------------------------- glyphs --

#[derive(Component)]
struct GlyphView {
    total: u32,
}

/// The floor mark is a bit wider than the bell's hurt zone (bell half-width
/// plus the player's), so standing "just outside" it is really safe.
fn attach_glyph_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &Glyph, &SimPos), Added<Glyph>>,
) {
    for (e, g, pos) in &q {
        let width = 2.0 * (g.bell_half.x + 0.3) + 0.1;
        let mark = mats.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.75, 0.2, 0.55),
            emissive: LinearRgba::rgb(3.0, 1.6, 0.3),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        let column = mats.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.7, 0.2, 0.04),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        let column_h = (g.ceiling_y - pos.0.y).max(1.0);
        commands
            .entity(e)
            .insert((
                GlyphView { total: g.ticks },
                Mesh3d(meshes.add(Cuboid::new(width, 0.12, 1.2))),
                MeshMaterial3d(mark),
                Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
                Visibility::default(),
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(meshes.add(Cuboid::new(width, column_h, 0.4))),
                    MeshMaterial3d(column),
                    Transform::from_xyz(0.0, column_h * 0.5, 0.0),
                ));
            });
    }
}

/// The mark blinks faster and brighter as the bell's fall gets closer.
fn glyph_fx(
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(&Glyph, &GlyphView, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (g, v, handle) in &q {
        let left = g.ticks as f32 / v.total.max(1) as f32;
        let urgency = 1.0 - left;
        let on = (g.ticks as f32 / (2.0 + left * 10.0)) as u32 & 1 == 0;
        let Some(m) = mats.get_mut(&handle.0) else {
            continue;
        };
        let k = if on { 2.0 + 4.0 * urgency } else { 0.6 };
        m.emissive = LinearRgba::rgb(1.5 * k, 0.8 * k, 0.15 * k);
    }
}

// ------------------------------------------------------------- pendulums --

/// A thin bar joining a pendulum's pivot to its bell.
#[derive(Component)]
struct Chain(Entity);

fn attach_pendulum_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &SimPos), Added<Pendulum>>,
) {
    for (e, pos) in &q {
        // Gold: these are the bells you can pogo off.
        let bob = mats.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.55, 0.2),
            metallic: 0.6,
            perceptual_roughness: 0.4,
            emissive: LinearRgba::rgb(1.6, 0.9, 0.2),
            ..default()
        });
        let chain = mats.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.4, 0.36),
            emissive: LinearRgba::rgb(0.25, 0.2, 0.12),
            metallic: 0.5,
            perceptual_roughness: 0.5,
            ..default()
        });
        commands.entity(e).insert((
            Mesh3d(meshes.add(Sphere::new(0.7))),
            MeshMaterial3d(bob),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
        commands.spawn((
            Chain(e),
            Mesh3d(meshes.add(Cuboid::new(0.16, 1.0, 0.16))),
            MeshMaterial3d(chain),
            Transform::default(),
        ));
    }
}

fn update_chains(
    mut commands: Commands,
    pendulums: Query<(&Pendulum, &Transform), Without<Chain>>,
    mut chains: Query<(Entity, &Chain, &mut Transform)>,
) {
    for (ce, chain, mut t) in &mut chains {
        let Ok((p, bob)) = pendulums.get(chain.0) else {
            commands.entity(ce).despawn();
            continue;
        };
        let a = p.pivot.extend(0.0);
        let d = bob.translation - a;
        let len = d.length().max(0.01);
        t.translation = a + d * 0.5;
        t.rotation = Quat::from_rotation_arc(Vec3::Y, d / len);
        t.scale = Vec3::new(1.0, len, 1.0);
    }
}

// ---------------------------------------------------------------- health bar --

#[derive(Component)]
struct BossBarRoot;
#[derive(Component)]
struct BossBarName;
#[derive(Component)]
struct BossBarFill;
#[derive(Component)]
struct BossBarNotch(usize);

const BAR_WIDTH: f32 = 560.0;

fn spawn_boss_bar(mut commands: Commands) {
    commands
        .spawn((
            BossBarRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    BossBarName,
                    Text::new(""),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.9, 0.8)),
                ));
                col.spawn((
                    Node {
                        width: Val::Px(BAR_WIDTH),
                        height: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                    Visibility::default(),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        BossBarFill,
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.86, 0.74, 0.48)),
                    ));
                    // Phase marks: where the boss changes gear.
                    for i in 0..3 {
                        bar.spawn((
                            BossBarNotch(i),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(-2.0),
                                width: Val::Px(2.0),
                                height: Val::Px(16.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                            Visibility::Hidden,
                        ));
                    }
                });
            });
        });
}

fn update_boss_bar(
    tuning: Res<Tuning>,
    bosses: Query<(&Boss, &BossBrain, &Health)>,
    mut root: Query<&mut Visibility, (With<BossBarRoot>, Without<BossBarNotch>)>,
    mut name: Query<&mut Text, With<BossBarName>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), (With<BossBarFill>, Without<BossBarNotch>)>,
    mut notches: Query<
        (&BossBarNotch, &mut Node, &mut Visibility),
        (Without<BossBarFill>, Without<BossBarRoot>),
    >,
) {
    let (Ok(mut root_vis), Ok(mut text), Ok((mut fill_node, mut fill_color))) =
        (root.single_mut(), name.single_mut(), fill.single_mut())
    else {
        return;
    };
    let Some((boss, brain, hp)) = bosses
        .iter()
        .find(|(_, b, _)| b.state != BossState::Sleeping)
    else {
        *root_vis = Visibility::Hidden;
        return;
    };
    *root_vis = Visibility::Inherited;
    let def = tuning.bosses.get(&boss.id);
    **text = def.map_or(boss.id.to_uppercase(), |d| d.name.to_uppercase());
    let frac = (hp.hp.max(0) as f32 / hp.max.max(1) as f32).clamp(0.0, 1.0);
    fill_node.width = Val::Percent(frac * 100.0);
    fill_color.0 = match brain.phase {
        1 => Color::srgb(0.86, 0.74, 0.48),
        2 => Color::srgb(0.9, 0.55, 0.3),
        _ => Color::srgb(0.9, 0.3, 0.25),
    };
    let thresholds = def.map(|d| d.phase_thresholds.as_slice()).unwrap_or(&[]);
    for (n, mut node, mut vis) in &mut notches {
        match thresholds.get(n.0) {
            Some(t) => {
                node.left = Val::Percent(t * 100.0);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

// ------------------------------------------------------------ arena, shake --

/// While the fight is on the doorways glow red: there is no leaving.
fn exit_lock_tint(
    lock: Res<ArenaLock>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<&MeshMaterial3d<StandardMaterial>, With<RoomExit>>,
) {
    let (base, glow) = if lock.0 {
        (
            Color::srgba(1.0, 0.25, 0.2, 0.28),
            LinearRgba::rgb(2.0, 0.2, 0.1),
        )
    } else {
        (
            Color::srgba(0.6, 0.85, 1.0, 0.10),
            LinearRgba::rgb(0.3, 0.6, 1.0),
        )
    };
    for handle in &q {
        if mats.get(&handle.0).is_some_and(|m| m.base_color != base) {
            if let Some(m) = mats.get_mut(&handle.0) {
                m.base_color = base;
                m.emissive = glow;
            }
        }
    }
}

/// Roars, landings and wall crashes shake the camera.
fn boss_shake(
    mut rig: ResMut<Rig>,
    mut awoke: MessageReader<BossAwoke>,
    mut phase: MessageReader<BossPhaseChanged>,
    mut defeated: MessageReader<BossDefeated>,
    bosses: Query<&BossBrain>,
    mut last: Local<(bool, bool)>,
) {
    for _ in awoke.read() {
        rig.0.add_trauma(0.45);
    }
    for _ in phase.read() {
        rig.0.add_trauma(0.8);
    }
    for _ in defeated.read() {
        rig.0.add_trauma(1.0);
    }
    if let Some(b) = bosses.iter().next() {
        let (was_grounded, hit_wall) = *last;
        if b.state == BossState::Active && b.was_grounded && !was_grounded {
            rig.0.add_trauma(0.5); // a slam lands
        }
        if b.hit_wall && !hit_wall {
            rig.0.add_trauma(0.7); // a charge crashes into the wall
        }
        *last = (b.was_grounded, b.hit_wall);
    }
}
