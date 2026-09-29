//! Drawing the simulation: bodies, slash boxes, projectiles, enemy state
//! tells, the player's state colour and flicker, HUD and the room banner.

use bevy::prelude::*;
use hk_sim::boss::{Boss, Pendulum};
use hk_sim::combat::*;
use hk_sim::components::{Aabb, SimPos};
use hk_sim::enemy::{Brain, EnemyKind, EnemyState};
use hk_sim::player::Player;
use hk_sim::world::room::{CurrentRoom, RoomEntered, RoomLibrary};
use hk_sim::SimTick;

use crate::interp::{Interpolated, RenderPrepSet};
use crate::scene::Palette;

/// `--show-hitboxes`: also draw the player's nail hitbox (a debugging aid; the
/// sword and its trail are what the player sees).
#[derive(Resource, Default)]
pub struct ShowHitboxes(pub bool);

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowHitboxes>()
            .add_systems(PostStartup, spawn_hud)
            .add_systems(
                Update,
                (
                    attach_body_visuals,
                    attach_hit_visuals,
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
        (
            Added<Hurtbox>,
            Without<Boss>,
            Without<Pendulum>,
            Without<Player>,
        ),
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
                Team::Player => &pal.marker,
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
            let plate = mats.add(StandardMaterial {
                base_color: Color::srgb(0.75, 0.80, 0.85),
                metallic: 0.6,
                perceptual_roughness: 0.35,
                ..default()
            });
            commands.entity(e).with_children(|p| {
                p.spawn((
                    ShieldPlate,
                    Mesh3d(meshes.add(Cuboid::new(0.18, half.y * 1.7, 1.1))),
                    MeshMaterial3d(plate.clone()),
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
    show: Res<ShowHitboxes>,
    q: Query<(Entity, &Hitbox, &SimPos), (Added<Hitbox>, Without<Hurtbox>)>,
) {
    for (e, hb, pos) in &q {
        if hb.team == Team::Player && hb.kind == HitKind::Nail && !show.0 {
            continue;
        }
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

// -------------------------------------------------------------------- HUD --

fn spawn_hud(mut commands: Commands) {
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
    mut last: Local<String>,
) {
    let Ok((mut text, mut color, mut banner)) = q.single_mut() else {
        return;
    };
    // Announce a room when you arrive in it, not when it merely reloads
    // (resting at a bench, coming back after a death).
    if entered.read().count() > 0 && *last != current.id {
        if let Some(def) = library.get(&current.id) {
            **text = def.name.to_uppercase();
            banner.timer = 3.0;
            *last = current.id.clone();
        }
    }
    banner.timer = (banner.timer - time.delta_secs()).max(0.0);
    // Fade in over 0.5 s, hold, fade out over the last second.
    let a = (banner.timer.min(1.0))
        .min((3.0 - banner.timer) * 2.0)
        .clamp(0.0, 1.0);
    color.0 = Color::srgba(0.95, 0.93, 0.85, a);
}
