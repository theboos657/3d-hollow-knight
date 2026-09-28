//! Short messages in the lower third of the screen: abilities learned, resting
//! at a bench, a boss falling. Also the "Up: rest" hint next to a bench.

use bevy::prelude::*;
use hk_sim::boss::{ArenaLock, BossDefeated};
use hk_sim::combat::CombatState;
use hk_sim::components::{Aabb, SimPos};
use hk_sim::player::{Motor, Player};
use hk_sim::world::progress::{AbilityGained, BenchRested};
use hk_sim::world::room::{Ability, Bench, Transition};

use crate::interp::RenderPrepSet;

pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Toasts>()
            .add_systems(PostStartup, spawn_ui)
            .add_systems(Update, (collect, show, bench_hint).after(RenderPrepSet));
    }
}

/// Messages waiting to be shown: (seconds until it appears, text).
#[derive(Resource, Default)]
struct Toasts {
    queue: Vec<(f32, String)>,
    current: Option<(String, f32)>,
}

const SHOWN_SECS: f32 = 4.0;

#[derive(Component)]
struct ToastText;

#[derive(Component)]
struct BenchHint;

fn spawn_ui(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(130.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                ToastText,
                Text::new(""),
                TextFont {
                    font_size: 30.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 0.92, 0.7, 0.0)),
                TextLayout::new_with_justify(Justify::Center),
            ));
        });
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(84.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                BenchHint,
                Text::new("Up: rest at the bench"),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(Color::srgba(0.9, 0.85, 0.7, 0.0)),
            ));
        });
}

fn ability_text(a: Ability) -> &'static str {
    match a {
        Ability::Dash => {
            "DASH\nPress C or Shift to dash. You cannot be hurt for a moment as you go."
        }
        Ability::WallGrip => {
            "WALL GRIP\nHold toward a wall to slide down it, and press Jump to leap off it."
        }
    }
}

fn collect(
    mut toasts: ResMut<Toasts>,
    mut gained: MessageReader<AbilityGained>,
    mut rested: MessageReader<BenchRested>,
    mut beaten: MessageReader<BossDefeated>,
) {
    for d in beaten.read() {
        let name = match d.id.as_str() {
            "matron" => "The Gutter Matron falls.",
            "bellwarden" => "The Bellwarden falls. The bells are silent.",
            _ => "Victory.",
        };
        toasts.queue.push((2.4, name.to_string()));
    }
    for g in gained.read() {
        // After the boss's death animation, if that is what gave it.
        toasts
            .queue
            .push((4.0, ability_text(g.ability).to_string()));
    }
    for _ in rested.read() {
        toasts
            .queue
            .push((0.0, "Rested. Health restored.".to_string()));
    }
}

fn show(
    time: Res<Time>,
    mut toasts: ResMut<Toasts>,
    mut q: Query<(&mut Text, &mut TextColor), With<ToastText>>,
) {
    let Ok((mut text, mut color)) = q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    for (delay, _) in toasts.queue.iter_mut() {
        *delay -= dt;
    }
    if toasts.current.is_none() {
        if let Some(i) = toasts.queue.iter().position(|(d, _)| *d <= 0.0) {
            let (_, msg) = toasts.queue.remove(i);
            toasts.current = Some((msg, SHOWN_SECS));
        }
    }
    let mut alpha = 0.0;
    if let Some((msg, left)) = toasts.current.as_mut() {
        *left -= dt;
        // Fade in over 0.3 s, out over the last 0.8 s.
        alpha = (*left / 0.8)
            .min((SHOWN_SECS - *left) / 0.3)
            .clamp(0.0, 1.0);
        if **text != *msg {
            **text = msg.clone();
        }
        if *left <= 0.0 {
            toasts.current = None;
        }
    }
    color.0 = Color::srgba(1.0, 0.92, 0.7, alpha);
}

/// "Up: rest" while standing at a bench.
fn bench_hint(
    lock: Res<ArenaLock>,
    tr: Res<Transition>,
    players: Query<(&SimPos, &Aabb, &Motor, &CombatState), With<Player>>,
    benches: Query<(&SimPos, &Bench)>,
    mut hint: Query<&mut TextColor, With<BenchHint>>,
) {
    let Ok(mut color) = hint.single_mut() else {
        return;
    };
    let near = !lock.0
        && !tr.active()
        && players.iter().any(|(p, a, m, cs)| {
            m.grounded
                && !cs.dead
                && benches.iter().any(|(bp, b)| {
                    let d = (p.0 - bp.0).abs();
                    d.x < a.half.x + b.half.x + 0.4 && d.y < a.half.y + b.half.y + 0.2
                })
        });
    color.0 = Color::srgba(0.9, 0.85, 0.7, if near { 0.9 } else { 0.0 });
}
