//! Drawing the simulation: bodies, slash boxes, projectiles, enemy state
//! tells, the player's state colour and flicker, HUD and the room banner.

use bevy::prelude::*;
use hk_sim::boss::{Boss, Pendulum};
use hk_sim::combat::*;
use hk_sim::components::{Aabb, SimPos};
use hk_sim::enemy::{Brain, EnemyKind, EnemyState};
use hk_sim::player::{Facing, Player, PlayerState};
use hk_sim::world::room::{CurrentRoom, RoomEntered, RoomLibrary};
use hk_sim::SimTick;

use crate::interp::{Interpolated, RenderPrepSet};
use crate::scene::Palette;

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_hud).add_systems(
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
                room_banner,
            )
                .after(RenderPrepSet),
        );
    }
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
        (Added<Hurtbox>, Without<Boss>, Without<Pendulum>),
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
        // Transform + Visibility first, so the children below never see a
        // parent that lacks them (Bevy warns B0004 about that).
        commands
            .entity(e)
            .insert((Transform::default(), Visibility::default()));
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
    // A full-width row centres the banner text (justify_content applies to a
    // node's children, not to the node's own text).
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(84.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                RoomBanner { timer: 0.0 },
                Text::new(""),
                TextFont {
                    font_size: 34.0,
                    ..default()
                },
                TextColor(Color::srgba(0.95, 0.93, 0.85, 0.0)),
            ));
        });
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

#[derive(Component)]
struct RoomBanner {
    timer: f32,
}

/// The room's name fades in and out at the top of the screen when you enter.
fn room_banner(
    time: Res<Time>,
    mut entered: MessageReader<RoomEntered>,
    library: Res<RoomLibrary>,
    current: Res<CurrentRoom>,
    mut q: Query<(&mut Text, &mut TextColor, &mut RoomBanner)>,
) {
    let Ok((mut text, mut color, mut banner)) = q.single_mut() else {
        return;
    };
    if entered.read().count() > 0 {
        if let Some(def) = library.get(&current.id) {
            **text = def.name.to_uppercase();
            banner.timer = 3.0;
        }
    }
    banner.timer = (banner.timer - time.delta_secs()).max(0.0);
    // Fade in over 0.5 s, hold, fade out over the last second.
    let a = (banner.timer.min(1.0))
        .min((3.0 - banner.timer) * 2.0)
        .clamp(0.0, 1.0);
    color.0 = Color::srgba(0.95, 0.93, 0.85, a);
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
