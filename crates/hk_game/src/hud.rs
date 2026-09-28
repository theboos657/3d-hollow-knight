//! The heads-up display: masks (health), soul, the "you died" message and a
//! controls reminder that fades away after a while.

use bevy::prelude::*;
use hk_sim::combat::{CombatState, Health, Soul};
use hk_sim::player::Player;

use crate::menu::Screen;
use crate::settings::Settings;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn).add_systems(
            Update,
            (update_masks, update_soul, update_text, visible_only_in_game),
        );
    }
}

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct MaskRow;
#[derive(Component)]
struct Mask(i32);
#[derive(Component)]
struct SoulFill(usize);
#[derive(Component)]
struct DiedText;
#[derive(Component)]
struct ControlsHint {
    age: f32,
}

const MASK_FULL: Color = Color::srgb(0.96, 0.94, 0.88);
const MASK_EMPTY: Color = Color::srgba(0.1, 0.1, 0.14, 0.75);
const SOUL_SEGMENTS: usize = 3;

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(18.0),
                top: Val::Px(16.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            Visibility::default(),
        ))
        .with_children(|root| {
            root.spawn((
                MaskRow,
                Node {
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                Visibility::default(),
            ));
            // Soul: three segments, one per Focus / Bolt (33 each).
            root.spawn((
                Node {
                    column_gap: Val::Px(4.0),
                    ..default()
                },
                Visibility::default(),
            ))
            .with_children(|row| {
                for i in 0..SOUL_SEGMENTS {
                    row.spawn((
                        Node {
                            width: Val::Px(54.0),
                            height: Val::Px(10.0),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.05, 0.08, 0.14, 0.75)),
                        Visibility::default(),
                    ))
                    .with_children(|seg| {
                        seg.spawn((
                            SoulFill(i),
                            Node {
                                width: Val::Percent(0.0),
                                height: Val::Percent(100.0),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.6, 0.85, 1.0)),
                        ));
                    });
                }
            });
        });

    // "YOU DIED", centred.
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Percent(38.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                DiedText,
                Text::new("YOU DIED"),
                TextFont {
                    font_size: 64.0,
                    ..default()
                },
                TextColor(Color::srgba(0.85, 0.25, 0.25, 0.0)),
            ));
        });

    commands.spawn((
        HudRoot,
        ControlsHint { age: 0.0 },
        Text::new(
            "Move: arrows / WASD    Jump: Space / Z    Attack: X / J  (Up / Down + attack; Down in the air = pogo)\n\
             Dash: C / Shift    Focus (hold, heals): F    Ember Bolt: V    Rest at a bench: Up    Pause: Esc",
        ),
        TextFont {
            font_size: 15.0,
            ..default()
        },
        TextColor(Color::srgba(0.8, 0.85, 1.0, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(10.0),
            ..default()
        },
    ));
}

/// Keeps one mask icon per point of maximum health, filled or empty.
fn update_masks(
    mut commands: Commands,
    player: Query<&Health, With<Player>>,
    row: Query<(Entity, Option<&Children>), With<MaskRow>>,
    mut masks: Query<(&Mask, &mut BackgroundColor)>,
) {
    let (Ok(hp), Ok((row, children))) = (player.single(), row.single()) else {
        return;
    };
    let have = children.map_or(0, |c| c.len()) as i32;
    if have != hp.max {
        // Rebuild for a new maximum (rare).
        commands.entity(row).despawn_children();
        commands.entity(row).with_children(|r| {
            for i in 0..hp.max {
                r.spawn((
                    Mask(i),
                    Node {
                        width: Val::Px(26.0),
                        height: Val::Px(30.0),
                        border_radius: BorderRadius::new(
                            Val::Px(13.0),
                            Val::Px(13.0),
                            Val::Px(6.0),
                            Val::Px(6.0),
                        ),
                        ..default()
                    },
                    BackgroundColor(MASK_FULL),
                    Visibility::default(),
                ));
            }
        });
        return;
    }
    for (m, mut bg) in &mut masks {
        let c = if m.0 < hp.hp { MASK_FULL } else { MASK_EMPTY };
        if bg.0 != c {
            bg.0 = c;
        }
    }
}

fn update_soul(player: Query<&Soul, With<Player>>, mut fills: Query<(&SoulFill, &mut Node)>) {
    let Ok(soul) = player.single() else {
        return;
    };
    let per = soul.max as f32 / SOUL_SEGMENTS as f32;
    for (f, mut node) in &mut fills {
        let lo = per * f.0 as f32;
        let frac = ((soul.value as f32 - lo) / per).clamp(0.0, 1.0);
        node.width = Val::Percent(frac * 100.0);
    }
}

fn update_text(
    time: Res<Time<Real>>,
    player: Query<&CombatState, With<Player>>,
    mut died: Query<&mut TextColor, (With<DiedText>, Without<ControlsHint>)>,
    mut hint: Query<(&mut ControlsHint, &mut TextColor), Without<DiedText>>,
) {
    if let (Ok(cs), Ok(mut c)) = (player.single(), died.single_mut()) {
        c.0 = Color::srgba(0.85, 0.25, 0.25, if cs.dead { 0.95 } else { 0.0 });
    }
    if let Ok((mut h, mut c)) = hint.single_mut() {
        h.age += time.delta_secs();
        // Shown for 25 s, then fades over 4.
        let a = ((29.0 - h.age) / 4.0).clamp(0.0, 1.0) * 0.8;
        c.0 = Color::srgba(0.8, 0.85, 1.0, a);
    }
}

/// The HUD hides behind menus and on the title screen.
fn visible_only_in_game(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut roots: Query<&mut Visibility, With<HudRoot>>,
) {
    let _ = settings;
    let v = if *screen == Screen::Playing || *screen == Screen::Ended {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut r in &mut roots {
        if *r != v {
            *r = v;
        }
    }
}
