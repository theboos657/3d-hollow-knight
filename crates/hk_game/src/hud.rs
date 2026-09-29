//! The heads-up display: masks (health), the soul vessel, the hurt vignette and
//! the "you died" message. (What the controls are is taught by `tutorial`.)

use bevy::prelude::*;
use hk_sim::combat::{CombatState, Health, Hit, Soul, Team};
use hk_sim::player::Player;

use crate::hud_art::{HudArt, FLASK};
use crate::menu::Screen;
use crate::settings::Settings;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn).add_systems(
            Update,
            (
                update_masks,
                update_soul,
                update_text,
                hurt_flash,
                visible_only_in_game,
            ),
        );
    }
}

#[derive(Component)]
struct HudRoot;
#[derive(Component)]
struct MaskRow;
#[derive(Component)]
struct Mask(i32);
/// The liquid of the soul vessel (cropped to show how full it is).
#[derive(Component)]
struct SoulLiquid;
/// The halo that pulses when there is enough soul to use.
#[derive(Component)]
struct SoulGlow;
/// The red vignette that flashes when you are hurt.
#[derive(Component)]
struct HurtOverlay;

#[derive(Component)]
struct DiedText;
/// The vessel's size on screen, in pixels.
const FLASK_PX: f32 = 84.0;
const MASK_PX: (f32, f32) = (33.0, 37.0);
/// Soul that one Focus or Bolt costs (the pulse shows when you have that much).
const SOUL_READY: i32 = 33;

/// How to show a vessel that is `fraction` full: the image row where the
/// liquid's top is (the disc spans rows 9..87 of the 96-pixel picture) and the
/// fraction of the node's height that stays visible.
pub fn soul_crop(fraction: f32) -> (f32, f32) {
    let c = FLASK as f32 / 2.0;
    let (bottom, top) = (c + 39.0, c - 39.0);
    let y_top = bottom - fraction.clamp(0.0, 1.0) * (bottom - top);
    (y_top, (FLASK as f32 - y_top) / FLASK as f32)
}

fn spawn(mut commands: Commands, art: Res<HudArt>) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(12.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            },
            Visibility::default(),
        ))
        .with_children(|root| {
            // The soul vessel: halo, glass, liquid.
            root.spawn((
                Node {
                    width: Val::Px(FLASK_PX),
                    height: Val::Px(FLASK_PX),
                    ..default()
                },
                Visibility::default(),
            ))
            .with_children(|v| {
                v.spawn((
                    SoulGlow,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(-14.0),
                        top: Val::Px(-14.0),
                        width: Val::Px(FLASK_PX + 28.0),
                        height: Val::Px(FLASK_PX + 28.0),
                        ..default()
                    },
                    ImageNode::new(art.glow.clone()).with_color(Color::srgba(1.0, 1.0, 1.0, 0.0)),
                ));
                v.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(FLASK_PX),
                        height: Val::Px(FLASK_PX),
                        ..default()
                    },
                    ImageNode::new(art.flask.clone()),
                ));
                v.spawn((
                    SoulLiquid,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        width: Val::Px(FLASK_PX),
                        height: Val::Px(0.0),
                        ..default()
                    },
                    ImageNode::new(art.liquid.clone()),
                ));
            });
            root.spawn((
                MaskRow,
                Node {
                    column_gap: Val::Px(5.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
                Visibility::default(),
            ));
        });

    // The hurt vignette: transparent until you are hit.
    commands.spawn((
        HurtOverlay,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(art.hurt.clone()).with_color(Color::srgba(1.0, 1.0, 1.0, 0.0)),
        GlobalZIndex(-40),
    ));

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
}

/// A mask's short-lived effects: how long its "just broke" crack still shows,
/// and how long its "just healed" glow.
#[derive(Component, Default)]
struct MaskFx {
    crack: f32,
    heal: f32,
}

/// Keeps one mask per point of maximum health, whole or empty; a mask that just
/// broke shows a crack for a moment, one just healed glows.
#[allow(clippy::too_many_arguments)]
fn update_masks(
    mut commands: Commands,
    time: Res<Time<Real>>,
    art: Res<HudArt>,
    player: Query<&Health, With<Player>>,
    row: Query<(Entity, Option<&Children>), With<MaskRow>>,
    mut masks: Query<(&Mask, &mut MaskFx, &mut ImageNode)>,
    mut last: Local<Option<i32>>,
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
                    MaskFx::default(),
                    Node {
                        width: Val::Px(MASK_PX.0),
                        height: Val::Px(MASK_PX.1),
                        ..default()
                    },
                    ImageNode::new(art.mask_full.clone()),
                ));
            }
        });
        *last = Some(hp.hp);
        return;
    }
    let dt = time.delta_secs();
    let before = last.unwrap_or(hp.hp);
    *last = Some(hp.hp);
    for (m, mut fx, mut img) in &mut masks {
        if hp.hp < before && m.0 >= hp.hp && m.0 < before {
            fx.crack = 0.45;
        }
        if hp.hp > before && m.0 >= before && m.0 < hp.hp {
            fx.heal = 0.6;
        }
        fx.crack = (fx.crack - dt).max(0.0);
        fx.heal = (fx.heal - dt).max(0.0);
        let want = if fx.crack > 0.0 {
            &art.mask_cracked
        } else if m.0 < hp.hp {
            &art.mask_full
        } else {
            &art.mask_empty
        };
        if img.image != *want {
            img.image = want.clone();
        }
        // A heal glows cyan-white and fades to plain.
        let k = (fx.heal / 0.6).clamp(0.0, 1.0);
        let tint = Color::srgb(1.0 - 0.35 * k, 1.0, 1.0);
        if img.color != tint {
            img.color = tint;
        }
    }
}

fn update_soul(
    time: Res<Time<Real>>,
    player: Query<&Soul, With<Player>>,
    mut liquid: Query<(&mut Node, &mut ImageNode), (With<SoulLiquid>, Without<SoulGlow>)>,
    mut glow: Query<&mut ImageNode, (With<SoulGlow>, Without<SoulLiquid>)>,
) {
    let Ok(soul) = player.single() else {
        return;
    };
    let fraction = soul.value as f32 / soul.max.max(1) as f32;
    let (y_top, visible) = soul_crop(fraction);
    for (mut node, mut img) in &mut liquid {
        node.height = Val::Px(FLASK_PX * visible);
        img.rect = Some(Rect::new(0.0, y_top, FLASK as f32, FLASK as f32));
    }
    // A halo that breathes while there is enough for a Focus or a Bolt.
    let ready = soul.value >= SOUL_READY;
    let pulse = 0.55 + 0.35 * (time.elapsed_secs() * 4.0).sin();
    for mut g in &mut glow {
        g.color = Color::srgba(1.0, 1.0, 1.0, if ready { pulse } else { 0.0 });
    }
}

/// A red vignette that flashes when the knight is hit and beats slowly when
/// only one mask is left.
fn hurt_flash(
    time: Res<Time<Real>>,
    mut hits: MessageReader<Hit>,
    player: Query<(&Health, &CombatState), With<Player>>,
    mut overlay: Query<&mut ImageNode, With<HurtOverlay>>,
    mut level: Local<f32>,
) {
    let dt = time.delta_secs();
    for h in hits.read() {
        if h.victim_team == Team::Player {
            *level = 1.0;
        }
    }
    *level = (*level - dt * 1.8).max(0.0);
    let low = player.single().is_ok_and(|(hp, cs)| hp.hp == 1 && !cs.dead);
    let beat = if low {
        0.14 + 0.10 * (time.elapsed_secs() * 3.2).sin().max(0.0)
    } else {
        0.0
    };
    let a = (*level * 0.85).max(beat);
    for mut o in &mut overlay {
        let c = Color::srgba(1.0, 1.0, 1.0, a);
        if o.color != c {
            o.color = c;
        }
    }
}

fn update_text(
    player: Query<&CombatState, With<Player>>,
    mut died: Query<&mut TextColor, With<DiedText>>,
) {
    if let (Ok(cs), Ok(mut c)) = (player.single(), died.single_mut()) {
        c.0 = Color::srgba(0.85, 0.25, 0.25, if cs.dead { 0.95 } else { 0.0 });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_liquid_crop_shows_the_bottom_of_the_disc_and_grows_upward() {
        let (empty_y, empty_vis) = soul_crop(0.0);
        let (half_y, half_vis) = soul_crop(0.5);
        let (full_y, full_vis) = soul_crop(1.0);
        assert!(
            empty_y > half_y && half_y > full_y,
            "the top rises as it fills"
        );
        assert!(empty_vis < half_vis && half_vis < full_vis);
        // Empty shows nearly nothing; full shows the whole disc down to the image's bottom.
        assert!(empty_vis < 0.1, "{empty_vis}");
        assert!(full_vis > 0.85, "{full_vis}");
        assert_eq!(soul_crop(-3.0), soul_crop(0.0));
        assert_eq!(soul_crop(9.0), soul_crop(1.0));
    }
}
