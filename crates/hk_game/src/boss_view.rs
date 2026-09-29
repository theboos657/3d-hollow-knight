//! Boss presentation: the body and its state tells, warning glyphs, swinging
//! bells and their chains, the health bar, the sealed-exit tint and the camera
//! shake for a boss's big moments. All gameplay lives in `hk_sim::boss`.

use bevy::prelude::*;
use hk_sim::boss::{
    ArenaLock, Boss, BossAwoke, BossBrain, BossDefeated, BossPhaseChanged, BossState, Glyph,
    Pendulum,
};
use hk_sim::combat::Health;
use hk_sim::components::SimPos;
use hk_sim::tuning::Tuning;

use crate::camera_rig::Rig;
use crate::interp::{Interpolated, RenderPrepSet};
use crate::look::fixtures::{ExitLight, ExitVeil};

pub struct BossViewPlugin;

impl Plugin for BossViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_boss_bar).add_systems(
            Update,
            (
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
            Mesh3d(meshes.add(crate::look::kits::bell(0.0, 0.7, 0.72, 0.0).to_mesh())),
            MeshMaterial3d(bob),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
        commands.spawn((
            Chain(e),
            Mesh3d(meshes.add(Cylinder::new(0.05, 1.0))),
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
    veils: Query<&MeshMaterial3d<StandardMaterial>, With<ExitVeil>>,
    mut lights: Query<&mut PointLight, With<ExitLight>>,
) {
    let (tint, light) = if lock.0 {
        (
            Color::linear_rgb(2.2, 0.20, 0.12),
            Color::srgb(1.0, 0.2, 0.15),
        )
    } else {
        (Color::WHITE, Color::srgb(0.55, 0.82, 1.0))
    };
    for handle in &veils {
        if mats.get(&handle.0).is_some_and(|m| m.base_color != tint) {
            if let Some(m) = mats.get_mut(&handle.0) {
                m.base_color = tint;
            }
        }
    }
    for mut l in &mut lights {
        if l.color != light {
            l.color = light;
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
