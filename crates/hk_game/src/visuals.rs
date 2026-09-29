//! Drawing the simulation: bodies, slash boxes, projectiles, enemy state
//! tells, the player's state colour and flicker, HUD and the room banner.

use bevy::prelude::*;
use hk_sim::boss::{Boss, Pendulum};
use hk_sim::combat::*;
use hk_sim::components::{Aabb, SimPos};
use hk_sim::player::Player;
use hk_sim::world::room::{CurrentRoom, RoomEntered, RoomLibrary};

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
                (attach_body_visuals, room_banner).after(RenderPrepSet),
            );
    }
}

// ---------------------------------------------------------------- visuals --

/// Hazards and player-team markers get a plain box the size of their hurt/
/// collision box. Enemies and the training dummy have real models
/// (`models::enemies`), so they are left alone here.
fn attach_body_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    pal: Res<Palette>,
    q: Query<
        (Entity, &Hurtbox, Option<&Aabb>, &SimPos, Option<&Hitbox>),
        (
            Added<Hurtbox>,
            Without<Boss>,
            Without<Pendulum>,
            Without<Player>,
        ),
    >,
) {
    for (e, hu, aabb, pos, hitbox) in &q {
        // Spikes are crystals (`look::fixtures`).
        if hitbox.is_some_and(|h| h.kind == HitKind::Hazard) {
            continue;
        }
        let mat = match hu.team {
            Team::Enemy => continue,
            Team::Player => &pal.marker,
            Team::Hazard => &pal.hazard,
        };
        let half = aabb.map_or(hu.half, |a| a.half);
        commands.entity(e).insert((
            Mesh3d(meshes.add(Cuboid::new(half.x * 2.0, half.y * 2.0, 0.8))),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Interpolated {
                z: 0.0,
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
