//! Teaching by doing. Instead of a paragraph of controls, a short prompt
//! appears at the moment it is useful, uses the player's own key bindings, and
//! goes away once they have done the thing:
//!
//! * **Move**: at the start: move and jump.
//! * **Attack**: when something you can hit is near (a floating marker also
//!   hangs over the first training dummy until you hit it).
//! * **Tells**: the first time an enemy notices you: what the colours mean.
//! * **Pogo**: near spikes: strike down in the air.
//! * **Bench**: beside one: rest to heal and save.
//! * **Focus**: hurt with soul to spend: hold to heal.
//! * **Dash** and **Grip**: when you learn them.
//!
//! Each shows once (remembered in the settings file). All the logic that
//! decides *which* tip is pure (`next_tip`, `is_done`) and unit-tested.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use hk_sim::combat::{AttackDir, CombatState, Health, Hit, HitKind, Hitbox, Hurtbox, Soul, Team};
use hk_sim::components::SimPos;
use hk_sim::enemy::{Brain, EnemyState};
use hk_sim::input::Action;
use hk_sim::player::{Player, PlayerState};
use hk_sim::world::progress::{AbilityGained, BenchRested};
use hk_sim::world::room::{Ability, Bench, RoomEntered};
use serde::{Deserialize, Serialize};

use crate::menu::Screen;
use crate::models::enemies::CreatureRig;
use crate::rig::creature::Species;
use crate::scene::MainCamera;
use crate::settings::Settings;

pub struct TutorialPlugin;

impl Plugin for TutorialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Progress>()
            .init_resource::<Current>()
            .add_systems(PostStartup, spawn_ui)
            .add_systems(Update, (gather, advance, show, dummy_marker).chain());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tip {
    Move,
    Attack,
    Tells,
    Pogo,
    Bench,
    Focus,
    Dash,
    Grip,
}

/// The order tips are considered in.
const ORDER: [Tip; 8] = [
    Tip::Move,
    Tip::Attack,
    Tip::Tells,
    Tip::Pogo,
    Tip::Bench,
    Tip::Focus,
    Tip::Dash,
    Tip::Grip,
];

/// What is true right now, and what the player has done so far.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    // -- happening now --
    pub enemy_near: bool,
    pub notice_now: bool,
    pub spikes_near: bool,
    pub bench_near: bool,
    pub can_focus: bool,
    pub dash_gained: bool,
    pub grip_gained: bool,
    // -- things the player has done --
    pub moved: bool,
    pub attacked: bool,
    pub hit_enemy: bool,
    pub pogoed: bool,
    pub rested: bool,
    pub focused: bool,
}

/// The first unseen tip whose moment has come.
pub fn next_tip(f: &Facts, seen: &[Tip]) -> Option<Tip> {
    ORDER.into_iter().find(|t| {
        if seen.contains(t) {
            return false;
        }
        match t {
            Tip::Move => true,
            // Move first: never pile prompts on top of each other.
            Tip::Attack => seen.contains(&Tip::Move) && f.enemy_near && !f.hit_enemy,
            Tip::Tells => f.notice_now,
            Tip::Pogo => f.spikes_near && f.attacked,
            Tip::Bench => f.bench_near,
            Tip::Focus => f.can_focus && f.attacked,
            Tip::Dash => f.dash_gained,
            Tip::Grip => f.grip_gained,
        }
    })
}

/// Has the player done what the tip asks (so it can go away)?
pub fn is_done(t: Tip, f: &Facts) -> bool {
    match t {
        Tip::Move => f.moved,
        Tip::Attack => f.hit_enemy,
        Tip::Pogo => f.pogoed,
        Tip::Bench => f.rested,
        Tip::Focus => f.focused,
        // These have no single action to wait for: they simply time out.
        Tip::Tells | Tip::Dash | Tip::Grip => false,
    }
}

/// How long a tip stays up at most, in seconds.
pub fn max_seconds(t: Tip) -> f32 {
    match t {
        Tip::Move => 14.0,
        Tip::Attack | Tip::Pogo | Tip::Focus | Tip::Bench => 16.0,
        Tip::Tells => 8.0,
        Tip::Dash | Tip::Grip => 9.0,
    }
}

/// The words, with the player's own keys.
pub fn tip_text(t: Tip, s: &Settings) -> String {
    let k = |a: Action| format!("[{}]", s.label(a));
    match t {
        Tip::Move => format!(
            "Move  {} {}      Jump  {}",
            k(Action::Left),
            k(Action::Right),
            k(Action::Jump)
        ),
        Tip::Attack => format!(
            "Strike with your Needle  {}      aim with {} or {} while you strike",
            k(Action::Attack),
            k(Action::Up),
            k(Action::Down)
        ),
        Tip::Tells => {
            "Amber flash: about to strike.   Red: the strike.   Blue: your chance to hit back."
                .to_string()
        }
        Tip::Pogo => format!(
            "In the air, strike down ({} + {}) to bounce off spikes and enemies",
            k(Action::Down),
            k(Action::Attack)
        ),
        Tip::Bench => format!(
            "{} to rest at the bench: heal and save your place",
            k(Action::Up)
        ),
        Tip::Focus => format!(
            "Hold {} to focus: spend soul to heal a mask",
            k(Action::Focus)
        ),
        Tip::Dash => format!(
            "Dash  {}   a burst of speed, and you cannot be hurt while it lasts",
            k(Action::Dash)
        ),
        Tip::Grip => format!(
            "Press into a wall to slide down it, then {} to leap away",
            k(Action::Jump)
        ),
    }
}

// ------------------------------------------------------------------ state --

/// Things the player has done this session (the "did it" half of `Facts`).
#[derive(Resource, Default)]
struct Progress {
    origin: Option<Vec2>,
    facts: Facts,
    /// Pulses set by messages this frame.
    dash_gained: bool,
    grip_gained: bool,
}

#[derive(Resource, Default)]
struct Current {
    tip: Option<Tip>,
    age: f32,
    /// Counts down after the tip is done, while it fades out.
    leaving: Option<f32>,
}

#[derive(Component)]
struct TipPanel;
#[derive(Component)]
struct TipText;
#[derive(Component)]
struct DummyMarker;

fn spawn_ui(mut commands: Commands) {
    commands
        .spawn((
            TipPanel,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(54.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
                    border_radius: BorderRadius::all(Val::Px(14.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.03, 0.06, 0.62)),
            ))
            .with_children(|b| {
                b.spawn((
                    TipText,
                    Text::new(""),
                    TextFont {
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::srgba(0.95, 0.92, 0.8, 1.0)),
                ));
            });
        });
    // The marker that hangs over the first dummy.
    commands.spawn((
        DummyMarker,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(34.0),
            height: Val::Px(34.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(Val::Px(17.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.03, 0.06, 0.7)),
        Visibility::Hidden,
        children![(
            Text::new("X"),
            TextFont {
                font_size: 20.0,
                ..default()
            },
            TextColor(Color::srgb(1.0, 0.92, 0.6)),
        )],
    ));
}

/// Watches the game and keeps `Progress` up to date.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn gather(
    screen: Res<Screen>,
    mut progress: ResMut<Progress>,
    mut hits: MessageReader<Hit>,
    mut rested: MessageReader<BenchRested>,
    mut gained: MessageReader<AbilityGained>,
    mut entered: MessageReader<RoomEntered>,
    player: Query<(&SimPos, &PlayerState, &CombatState, &Health, &Soul), With<Player>>,
    creatures: Query<(&SimPos, &Hurtbox, Option<&Brain>)>,
    spikes: Query<(&SimPos, &Hitbox)>,
    benches: Query<&SimPos, With<Bench>>,
) {
    // Messages are always drained, whatever screen is up.
    for h in hits.read() {
        if h.kind == HitKind::Nail && h.victim_team == Team::Enemy {
            progress.facts.hit_enemy = true;
            if h.attack_dir == AttackDir::Down {
                progress.facts.pogoed = true;
            }
        }
        if h.kind == HitKind::Nail
            && h.victim_team == Team::Hazard
            && h.attack_dir == AttackDir::Down
        {
            progress.facts.pogoed = true;
        }
    }
    if rested.read().count() > 0 {
        progress.facts.rested = true;
    }
    for g in gained.read() {
        match g.ability {
            Ability::Dash => progress.dash_gained = true,
            Ability::WallGrip => progress.grip_gained = true,
        }
    }
    if entered.read().count() > 0 {
        progress.origin = None;
    }
    if *screen != Screen::Playing {
        return;
    }
    let Ok((pos, state, cs, hp, soul)) = player.single() else {
        return;
    };
    let p = pos.0;
    let origin = *progress.origin.get_or_insert(p);
    let (dash, grip) = (progress.dash_gained, progress.grip_gained);
    let f = &mut progress.facts;
    if (p - origin).length() > 3.5 {
        f.moved = true;
    }
    if cs.attack.is_some() {
        f.attacked = true;
    }
    if *state == PlayerState::Focus {
        f.focused = true;
    }
    f.enemy_near = creatures.iter().any(|(sp, hu, _)| {
        hu.team == Team::Enemy && (sp.0.x - p.x).abs() < 9.0 && (sp.0.y - p.y).abs() < 3.5
    });
    f.notice_now = creatures.iter().any(|(_, hu, b)| {
        hu.team == Team::Enemy && b.is_some_and(|b| b.state == EnemyState::Notice)
    });
    f.spikes_near = spikes.iter().any(|(sp, hb)| {
        hb.kind == HitKind::Hazard && (sp.0.x - p.x).abs() < 7.0 && (sp.0.y - p.y).abs() < 4.0
    });
    f.bench_near = benches.iter().any(|sp| (sp.0 - p).length() < 3.0);
    f.can_focus = soul.value >= 33 && hp.hp < hp.max;
    f.dash_gained = dash;
    f.grip_gained = grip;
}

/// Starts, times and ends tips.
fn advance(
    time: Res<Time<Real>>,
    screen: Res<Screen>,
    mut settings: ResMut<Settings>,
    progress: Res<Progress>,
    mut cur: ResMut<Current>,
) {
    if *screen != Screen::Playing {
        return;
    }
    let dt = time.delta_secs();
    match cur.tip {
        None => {
            if let Some(t) = next_tip(&progress.facts, &settings.tips_seen) {
                cur.tip = Some(t);
                cur.age = 0.0;
                cur.leaving = None;
            }
        }
        Some(t) => {
            cur.age += dt;
            if cur.leaving.is_none() && (is_done(t, &progress.facts) || cur.age > max_seconds(t)) {
                cur.leaving = Some(0.7);
                if !settings.tips_seen.contains(&t) {
                    settings.tips_seen.push(t);
                }
            }
            if let Some(l) = cur.leaving.as_mut() {
                *l -= dt;
                if *l <= 0.0 {
                    cur.tip = None;
                    cur.leaving = None;
                }
            }
        }
    }
}

/// Draws the current tip: fades in, holds, fades out.
fn show(
    cur: Res<Current>,
    settings: Res<Settings>,
    screen: Res<Screen>,
    mut panel: Query<&mut Visibility, With<TipPanel>>,
    mut text: Query<(&mut Text, &mut TextColor), With<TipText>>,
) {
    let (Ok(mut vis), Ok((mut txt, mut colour))) = (panel.single_mut(), text.single_mut()) else {
        return;
    };
    let Some(tip) = cur.tip.filter(|_| *screen == Screen::Playing) else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Inherited;
    let want = tip_text(tip, &settings);
    if txt.0 != want {
        txt.0 = want;
    }
    let fade_in = (cur.age / 0.5).clamp(0.0, 1.0);
    let fade_out = cur.leaving.map_or(1.0, |l| (l / 0.7).clamp(0.0, 1.0));
    colour.0 = Color::srgba(0.95, 0.92, 0.8, fade_in * fade_out);
}

/// An "X" hangs over the first training dummy until it has been hit once.
#[allow(clippy::type_complexity)]
fn dummy_marker(
    time: Res<Time<Real>>,
    screen: Res<Screen>,
    progress: Res<Progress>,
    settings: Res<Settings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    dummies: Query<(&CreatureRig, &GlobalTransform)>,
    mut marker: Query<(&mut Node, &mut Visibility), With<DummyMarker>>,
) {
    let Ok((mut node, mut vis)) = marker.single_mut() else {
        return;
    };
    let hidden = |v: &mut Visibility| {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    };
    if *screen != Screen::Playing
        || progress.facts.hit_enemy
        || settings.tips_seen.contains(&Tip::Attack)
    {
        hidden(&mut vis);
        return;
    }
    let (Ok((camera, cam_t)), Ok(_)) = (cam.single(), windows.single()) else {
        hidden(&mut vis);
        return;
    };
    let Some((_, gt)) = dummies.iter().find(|(r, _)| r.species == Species::Dummy) else {
        hidden(&mut vis);
        return;
    };
    let bob = 0.12 * (time.elapsed_secs() * 3.0).sin();
    let world = gt.translation() + Vec3::new(0.0, 1.0 + bob + 0.9, 0.0);
    match camera.world_to_viewport(cam_t, world) {
        Ok(p) => {
            node.left = Val::Px(p.x - 17.0);
            node.top = Val::Px(p.y - 17.0);
            *vis = Visibility::Inherited;
        }
        Err(_) => hidden(&mut vis),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_tip_is_moving_and_nothing_else_jumps_the_queue() {
        // Everything is happening at once, but Move comes first.
        let all = Facts {
            enemy_near: true,
            notice_now: true,
            spikes_near: true,
            bench_near: true,
            can_focus: true,
            attacked: true,
            ..Default::default()
        };
        assert_eq!(next_tip(&all, &[]), Some(Tip::Move));
        // Attack waits for Move to have been seen.
        assert_eq!(next_tip(&all, &[Tip::Move]), Some(Tip::Attack));
        // Once seen, it is not shown again, and the next moment takes over.
        assert_eq!(next_tip(&all, &[Tip::Move, Tip::Attack]), Some(Tip::Tells));
    }

    #[test]
    fn each_tip_waits_for_its_moment() {
        let seen = [Tip::Move, Tip::Attack];
        let none = Facts::default();
        assert_eq!(next_tip(&none, &seen), None, "nothing to teach right now");
        let f = |edit: fn(&mut Facts)| {
            let mut f = Facts::default();
            edit(&mut f);
            next_tip(&f, &seen)
        };
        assert_eq!(f(|f| f.notice_now = true), Some(Tip::Tells));
        // Spikes teach the pogo only to someone who has swung.
        assert_eq!(f(|f| f.spikes_near = true), None);
        assert_eq!(
            f(|f| {
                f.spikes_near = true;
                f.attacked = true;
            }),
            Some(Tip::Pogo)
        );
        assert_eq!(f(|f| f.bench_near = true), Some(Tip::Bench));
        assert_eq!(f(|f| f.can_focus = true), None, "focus needs a swing first");
        assert_eq!(
            f(|f| {
                f.can_focus = true;
                f.attacked = true;
            }),
            Some(Tip::Focus)
        );
        assert_eq!(f(|f| f.dash_gained = true), Some(Tip::Dash));
        assert_eq!(f(|f| f.grip_gained = true), Some(Tip::Grip));
    }

    #[test]
    fn hitting_something_ends_the_attack_tip_and_no_enemy_no_tip() {
        let near = Facts {
            enemy_near: true,
            ..Default::default()
        };
        assert_eq!(next_tip(&near, &[Tip::Move]), Some(Tip::Attack));
        // Already hit something: they know how; do not nag.
        let hit = Facts {
            enemy_near: true,
            hit_enemy: true,
            ..Default::default()
        };
        assert_eq!(next_tip(&hit, &[Tip::Move]), None);
        assert!(is_done(Tip::Attack, &hit));
        assert!(!is_done(Tip::Attack, &near));
    }

    #[test]
    fn a_completed_action_ends_its_tip_and_the_others_time_out() {
        let mut f = Facts::default();
        for t in [Tip::Move, Tip::Pogo, Tip::Bench, Tip::Focus] {
            assert!(!is_done(t, &f), "{t:?}");
        }
        f.moved = true;
        f.pogoed = true;
        f.rested = true;
        f.focused = true;
        for t in [Tip::Move, Tip::Pogo, Tip::Bench, Tip::Focus] {
            assert!(is_done(t, &f), "{t:?}");
        }
        for t in [Tip::Tells, Tip::Dash, Tip::Grip] {
            assert!(!is_done(t, &f) && max_seconds(t) > 0.0, "{t:?} times out");
        }
    }

    #[test]
    fn the_words_use_the_players_own_keys() {
        let mut s = Settings::default();
        assert!(tip_text(Tip::Attack, &s).contains("[X / J]"));
        assert!(tip_text(Tip::Move, &s).contains("Space / Z"));
        s.bind(Action::Attack, KeyCode::KeyK);
        assert!(
            tip_text(Tip::Attack, &s).contains('K'),
            "rebinding shows up"
        );
        assert!(!tip_text(Tip::Attack, &s).is_empty());
        for t in ORDER {
            assert!(tip_text(t, &s).len() > 15, "{t:?} says something");
        }
    }

    #[test]
    fn every_tip_is_ordered_once() {
        let mut seen = std::collections::HashSet::new();
        for t in ORDER {
            assert!(seen.insert(t), "{t:?} listed twice");
        }
        assert_eq!(seen.len(), 8);
    }
}
