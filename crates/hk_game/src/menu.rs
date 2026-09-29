//! Screens and menus: title, pause, options, key rebinding and the end card.
//! Keyboard and gamepad both work; nothing here touches the simulation except
//! by starting the game or pausing time.

use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow};
use hk_sim::boss::BossDefeated;
use hk_sim::input::{Action, InputState};
use hk_sim::world::progress::RunStats;
use hk_sim::SimTick;

use crate::audio::{play_sfx, Sounds};
use crate::save_io::{self, SaveDir};
use crate::settings::{self, Settings};
use crate::world_view::{begin_game, StartMode};

pub struct MenuPlugin {
    /// Start straight in the game (developer and scripted runs).
    pub skip_title: bool,
}

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(if self.skip_title {
            Screen::Playing
        } else {
            Screen::Title
        })
        .init_resource::<Menu>()
        .init_resource::<EndTimer>()
        .add_systems(Startup, init_menu)
        .add_systems(
            Update,
            (
                menu_input,
                pause_time,
                release_inputs,
                end_watch,
                apply_settings,
                menu_ui,
            )
                .chain(),
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Back {
    Title,
    Pause,
}

/// Which screen is up. Everything but `Playing` freezes the game.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,
    Playing,
    Paused,
    Options(Back),
    Controls(Back),
    Ended,
}

pub fn is_playing(screen: Res<Screen>) -> bool {
    *screen == Screen::Playing
}

#[derive(Resource, Default)]
struct Menu {
    sel: usize,
    /// A saved game exists (looked up once, at startup).
    has_save: bool,
    /// Waiting for a key for this action.
    rebinding: Option<Action>,
    note: String,
}

#[derive(Resource, Default)]
struct EndTimer(Option<f32>);

#[derive(Clone, PartialEq, Debug)]
enum Item {
    Continue,
    NewGame,
    Options,
    Quit,
    Resume,
    Master,
    Music,
    Sfx,
    Shake,
    Vsync,
    Controls,
    Back,
    Bind(Action),
    ResetKeys,
    KeepExploring,
}

fn init_menu(dir: Res<SaveDir>, mut menu: ResMut<Menu>) {
    menu.has_save = save_io::load(&dir.0).is_some();
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::Left => "Move left",
        Action::Right => "Move right",
        Action::Up => "Up / look up / rest",
        Action::Down => "Down / look down",
        Action::Jump => "Jump",
        Action::Attack => "Attack",
        Action::Dash => "Dash",
        Action::Focus => "Focus (heal)",
        Action::Cast => "Ember Bolt",
    }
}

fn items(screen: &Screen, has_save: bool) -> Vec<Item> {
    match screen {
        Screen::Title => {
            let mut v = Vec::new();
            if has_save {
                v.push(Item::Continue);
            }
            v.extend([Item::NewGame, Item::Options, Item::Quit]);
            v
        }
        Screen::Paused => vec![Item::Resume, Item::Options, Item::Quit],
        Screen::Options(_) => vec![
            Item::Master,
            Item::Music,
            Item::Sfx,
            Item::Shake,
            Item::Vsync,
            Item::Controls,
            Item::Back,
        ],
        Screen::Controls(_) => {
            let mut v: Vec<Item> = Action::ALL.iter().map(|a| Item::Bind(*a)).collect();
            v.push(Item::ResetKeys);
            v.push(Item::Back);
            v
        }
        Screen::Ended => vec![Item::KeepExploring, Item::Quit],
        Screen::Playing => vec![],
    }
}

fn label(item: &Item, s: &Settings, menu: &Menu) -> String {
    let pct = |v: f32| format!("{:>3}%", (v * 100.0).round() as i32);
    let onoff = |b: bool| if b { "On" } else { "Off" };
    match item {
        Item::Continue => "Continue".into(),
        Item::NewGame => "New Game".into(),
        Item::Options => "Options".into(),
        Item::Quit => "Quit".into(),
        Item::Resume => "Resume".into(),
        Item::Master => format!("Master volume   < {} >", pct(s.master)),
        Item::Music => format!("Music volume    < {} >", pct(s.music)),
        Item::Sfx => format!("Effects volume  < {} >", pct(s.sfx)),
        Item::Shake => format!("Screen shake    < {} >", onoff(s.shake)),
        Item::Vsync => format!("VSync           < {} >", onoff(s.vsync)),
        Item::Controls => "Controls".into(),
        Item::Back => "Back".into(),
        Item::Bind(a) => {
            let keys = if menu.rebinding == Some(*a) {
                "press a key...".to_string()
            } else {
                s.label(*a)
            };
            format!("{:<22}{}", action_name(*a), keys)
        }
        Item::ResetKeys => "Reset keys to defaults".into(),
        Item::KeepExploring => "Keep exploring".into(),
    }
}

// ------------------------------------------------------------------- input --

#[derive(Default, Clone, Copy)]
struct Nav {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    confirm: bool,
    back: bool,
    start: bool,
}

fn read_nav(keys: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>) -> Nav {
    let k = |c: &[KeyCode]| c.iter().any(|k| keys.just_pressed(*k));
    let mut n = Nav {
        up: k(&[KeyCode::ArrowUp, KeyCode::KeyW]),
        down: k(&[KeyCode::ArrowDown, KeyCode::KeyS]),
        left: k(&[KeyCode::ArrowLeft, KeyCode::KeyA]),
        right: k(&[KeyCode::ArrowRight, KeyCode::KeyD]),
        confirm: k(&[KeyCode::Enter, KeyCode::Space, KeyCode::KeyZ, KeyCode::KeyX]),
        back: k(&[KeyCode::Escape, KeyCode::Backspace]),
        start: k(&[KeyCode::Escape]),
    };
    for p in pads {
        n.up |= p.just_pressed(GamepadButton::DPadUp);
        n.down |= p.just_pressed(GamepadButton::DPadDown);
        n.left |= p.just_pressed(GamepadButton::DPadLeft);
        n.right |= p.just_pressed(GamepadButton::DPadRight);
        n.confirm |= p.just_pressed(GamepadButton::South);
        n.back |= p.just_pressed(GamepadButton::East);
        n.start |= p.just_pressed(GamepadButton::Start);
    }
    n
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    save_dir: Res<SaveDir>,
    sounds: Res<Sounds>,
    mut screen: ResMut<Screen>,
    mut menu: ResMut<Menu>,
    mut settings: ResMut<Settings>,
    mut exit: MessageWriter<AppExit>,
) {
    let has_save = *screen == Screen::Title && menu.has_save;

    // --- waiting for a key to bind ---
    if let Some(action) = menu.rebinding {
        if keys.just_pressed(KeyCode::Escape) {
            menu.rebinding = None;
            menu.note.clear();
            return;
        }
        if let Some(key) = keys.get_just_pressed().next().copied() {
            if settings::key_name(key).is_some() {
                settings.bind(action, key);
                menu.note.clear();
                menu.rebinding = None;
                play_sfx(&mut commands, &sounds, &settings, "ui_select", 1.0);
            } else {
                menu.note = "That key can't be used.".into();
            }
        }
        return;
    }

    let nav = read_nav(&keys, &pads);

    // --- in the game: Escape pauses ---
    if *screen == Screen::Playing {
        if nav.start {
            *screen = Screen::Paused;
            menu.sel = 0;
        }
        return;
    }

    let list = items(&screen, has_save);
    if list.is_empty() {
        return;
    }
    if menu.sel >= list.len() {
        menu.sel = list.len() - 1;
    }
    if nav.up {
        menu.sel = (menu.sel + list.len() - 1) % list.len();
        play_sfx(&mut commands, &sounds, &settings, "ui_move", 1.0);
    }
    if nav.down {
        menu.sel = (menu.sel + 1) % list.len();
        play_sfx(&mut commands, &sounds, &settings, "ui_move", 1.0);
    }
    let item = list[menu.sel].clone();

    // Sliders and switches respond to left/right too.
    let step = if nav.right {
        0.1
    } else if nav.left {
        -0.1
    } else {
        0.0
    };
    if step != 0.0 || nav.confirm {
        let bump = |v: &mut f32| *v = ((*v + step) * 10.0).round() / 10.0;
        match item {
            Item::Master if step != 0.0 => bump(&mut settings.master),
            Item::Music if step != 0.0 => bump(&mut settings.music),
            Item::Sfx if step != 0.0 => {
                bump(&mut settings.sfx);
                play_sfx(&mut commands, &sounds, &settings, "hit", 1.0);
            }
            Item::Shake => settings.shake = !settings.shake,
            Item::Vsync => settings.vsync = !settings.vsync,
            _ => {}
        }
        settings.clamp();
    }

    let mut confirmed = nav.confirm;
    // Escape / East: go back one level (or resume).
    if nav.back {
        match screen.clone() {
            Screen::Paused => {
                *screen = Screen::Playing;
                return;
            }
            Screen::Options(Back::Title) | Screen::Controls(Back::Title) => {
                *screen = Screen::Title;
                menu.sel = 0;
                return;
            }
            Screen::Options(Back::Pause) => {
                *screen = Screen::Paused;
                menu.sel = 0;
                return;
            }
            Screen::Controls(b) => {
                *screen = Screen::Options(b);
                menu.sel = 0;
                return;
            }
            _ => {}
        }
        confirmed = false;
    }
    if !confirmed {
        return;
    }
    play_sfx(&mut commands, &sounds, &settings, "ui_select", 1.0);
    match item {
        Item::Continue => {
            if let Some(save) = save_io::load(&save_dir.0) {
                start(&mut commands, StartMode::Continue(save));
                *screen = Screen::Playing;
            }
        }
        Item::NewGame => {
            start(&mut commands, StartMode::New);
            *screen = Screen::Playing;
        }
        Item::Resume | Item::KeepExploring => *screen = Screen::Playing,
        Item::Quit => {
            exit.write(AppExit::Success);
        }
        Item::Options => {
            let back = if *screen == Screen::Title {
                Back::Title
            } else {
                Back::Pause
            };
            *screen = Screen::Options(back);
            menu.sel = 0;
        }
        Item::Controls => {
            if let Screen::Options(b) = *screen {
                *screen = Screen::Controls(b);
                menu.sel = 0;
            }
        }
        Item::Back => {
            *screen = match screen.clone() {
                Screen::Options(Back::Title) => Screen::Title,
                Screen::Options(Back::Pause) => Screen::Paused,
                Screen::Controls(b) => Screen::Options(b),
                other => other,
            };
            menu.sel = 0;
        }
        Item::Bind(a) => {
            menu.rebinding = Some(a);
            menu.note.clear();
        }
        Item::ResetKeys => {
            settings.keys = Settings::default().keys;
        }
        _ => {}
    }
}

fn start(commands: &mut Commands, mode: StartMode) {
    commands.queue(move |world: &mut World| begin_game(world, mode));
}

// -------------------------------------------------- time, inputs, settings --

/// Menus stop the world: virtual time (and so the fixed-step sim) pauses.
fn pause_time(screen: Res<Screen>, mut vtime: ResMut<Time<Virtual>>) {
    if !screen.is_changed() {
        return;
    }
    if *screen == Screen::Playing {
        vtime.unpause();
    } else {
        vtime.pause();
    }
}

/// Entering a menu lets go of every button, so nothing keeps running or
/// jumping when the game resumes.
fn release_inputs(screen: Res<Screen>, tick: Res<SimTick>, mut input: ResMut<InputState>) {
    if screen.is_changed() {
        for a in Action::ALL {
            input.set(a, false, tick.0 + 1);
        }
    }
}

/// The end card appears a few seconds after the last boss falls.
fn end_watch(
    time: Res<Time<Real>>,
    mut beaten: MessageReader<BossDefeated>,
    mut timer: ResMut<EndTimer>,
    mut screen: ResMut<Screen>,
    mut menu: ResMut<Menu>,
) {
    for d in beaten.read() {
        if d.id == "bellwarden" {
            timer.0 = Some(6.5);
        }
    }
    if let Some(t) = timer.0.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            timer.0 = None;
            if *screen == Screen::Playing {
                *screen = Screen::Ended;
                menu.sel = 0;
            }
        }
    }
}

/// Applies settings that live outside the menu (vsync) and saves the file.
fn apply_settings(
    settings: Res<Settings>,
    dir: Res<SaveDir>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut w in &mut windows {
        w.present_mode = if settings.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };
    }
    if !settings.is_added() {
        settings::save(&dir.0, &settings);
    }
}

// ---------------------------------------------------------------------- UI --

#[derive(Component)]
struct MenuRoot;

fn menu_ui(
    mut commands: Commands,
    screen: Res<Screen>,
    menu: Res<Menu>,
    settings: Res<Settings>,
    stats: Res<RunStats>,
    old: Query<Entity, With<MenuRoot>>,
) {
    if !(screen.is_changed() || menu.is_changed() || settings.is_changed()) {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    if *screen == Screen::Playing {
        return;
    }
    let has_save = *screen == Screen::Title && menu.has_save;
    let list = items(&screen, has_save);

    let (title, subtitle, dim): (&str, String, f32) = match &*screen {
        Screen::Title => (
            "HOLLOW TOLL",
            "Nym, who cannot hear, climbs the bell-city of Vael.\nThe Bellwarden's toll hollows all who listen.".into(),
            1.0,
        ),
        Screen::Paused => ("PAUSED", String::new(), 0.8),
        Screen::Options(_) => ("OPTIONS", "Left / Right change a value.".into(), 0.9),
        Screen::Controls(_) => (
            "CONTROLS",
            if menu.note.is_empty() {
                "Enter: change a key.  Escape: back.  Gamepad: A jump, X attack, B/RT dash, LT focus, Y bolt.".into()
            } else {
                menu.note.clone()
            },
            0.9,
        ),
        Screen::Ended => {
            let secs = stats.seconds() as u32;
            (
                "SILENCE",
                format!(
                    "The last bell is still. For the first time in an age, Vael is quiet.\n\
                     Time {}:{:02}:{:02}    Deaths {}",
                    secs / 3600,
                    (secs / 60) % 60,
                    secs % 60,
                    stats.deaths
                ),
                0.85,
            )
        }
        Screen::Playing => return,
    };

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.02, 0.04, dim)),
            GlobalZIndex(50),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(title),
                TextFont {
                    font_size: if *screen == Screen::Title { 72.0 } else { 48.0 },
                    ..default()
                },
                TextColor(Color::srgb(0.93, 0.86, 0.66)),
            ));
            if !subtitle.is_empty() {
                p.spawn((
                    Text::new(subtitle),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.7, 0.72, 0.8)),
                    TextLayout::new_with_justify(Justify::Center),
                    Node {
                        margin: UiRect::bottom(Val::Px(18.0)),
                        ..default()
                    },
                ));
            }
            for (i, item) in list.iter().enumerate() {
                let selected = i == menu.sel;
                let text = label(item, &settings, &menu);
                p.spawn((
                    Text::new(if selected {
                        format!("> {text}")
                    } else {
                        format!("  {text}")
                    }),
                    TextFont {
                        font_size: 26.0,
                        ..default()
                    },
                    TextColor(if selected {
                        Color::srgb(1.0, 0.92, 0.6)
                    } else {
                        Color::srgb(0.62, 0.64, 0.72)
                    }),
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;
    use hk_sim::world::progress::SaveData;
    use hk_sim::world::room::RoomLibrary;
    use hk_sim::SimPlugin;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("hk_menu_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn app(screen: Screen, dir: std::path::PathBuf) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SimPlugin)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(SaveDir(dir))
            .init_resource::<Sounds>()
            .insert_resource(Settings::default())
            .insert_resource(RoomLibrary::default())
            .insert_resource(screen)
            .init_resource::<Menu>()
            .init_resource::<EndTimer>()
            .add_systems(Startup, init_menu)
            .add_systems(
                Update,
                (menu_input, pause_time, release_inputs, end_watch, menu_ui).chain(),
            );
        app.update();
        app
    }

    /// Presses and releases `key` over two frames, as a finger would.
    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        let mut k = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        k.release(key);
        k.clear();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    fn screen(app: &App) -> Screen {
        app.world().resource::<Screen>().clone()
    }

    fn virtual_paused(app: &App) -> bool {
        app.world().resource::<Time<Virtual>>().is_paused()
    }

    #[test]
    fn the_title_screen_offers_new_game_first_and_continue_only_with_a_save() {
        let dir = tmp_dir("title");
        let a = app(Screen::Title, dir.clone());
        assert_eq!(
            items(&screen(&a), false),
            vec![Item::NewGame, Item::Options, Item::Quit]
        );
        assert_eq!(items(&screen(&a), true)[0], Item::Continue);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn new_game_starts_playing_and_unpauses_time() {
        let mut a = app(Screen::Title, tmp_dir("new"));
        assert!(virtual_paused(&a), "time is frozen on the title screen");
        tap(&mut a, KeyCode::Enter); // "New Game" is selected
        assert_eq!(screen(&a), Screen::Playing);
        assert!(!virtual_paused(&a));
    }

    #[test]
    fn continue_is_the_default_when_a_save_exists() {
        let dir = tmp_dir("cont");
        crate::save_io::write(
            &dir,
            &SaveData {
                version: hk_sim::world::progress::SAVE_VERSION,
                room: "A1".into(),
                ..SaveData::default()
            },
        );
        let mut a = app(Screen::Title, dir.clone());
        tap(&mut a, KeyCode::Enter);
        assert_eq!(
            screen(&a),
            Screen::Playing,
            "Continue is first and selected"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn escape_pauses_and_resumes_and_freezes_the_sim() {
        let mut a = app(Screen::Playing, tmp_dir("pause"));
        assert!(!virtual_paused(&a));
        tap(&mut a, KeyCode::Escape);
        assert_eq!(screen(&a), Screen::Paused);
        assert!(virtual_paused(&a));
        tap(&mut a, KeyCode::Escape);
        assert_eq!(screen(&a), Screen::Playing);
        assert!(!virtual_paused(&a));
    }

    #[test]
    fn pausing_lets_go_of_held_buttons() {
        let mut a = app(Screen::Playing, tmp_dir("release"));
        a.world_mut()
            .resource_mut::<InputState>()
            .set(Action::Right, true, 1);
        tap(&mut a, KeyCode::Escape);
        assert!(
            !a.world().resource::<InputState>().held(Action::Right),
            "no running off when you unpause"
        );
    }

    #[test]
    fn options_sliders_move_in_tenths_and_stay_between_zero_and_one() {
        let mut a = app(Screen::Paused, tmp_dir("opts"));
        tap(&mut a, KeyCode::ArrowDown); // Options
        tap(&mut a, KeyCode::Enter);
        assert_eq!(screen(&a), Screen::Options(Back::Pause));
        let before = a.world().resource::<Settings>().master;
        tap(&mut a, KeyCode::ArrowRight);
        let after = a.world().resource::<Settings>().master;
        assert!((after - before - 0.1).abs() < 1e-4, "{before} -> {after}");
        for _ in 0..30 {
            tap(&mut a, KeyCode::ArrowRight);
        }
        assert_eq!(a.world().resource::<Settings>().master, 1.0);
        for _ in 0..30 {
            tap(&mut a, KeyCode::ArrowLeft);
        }
        assert_eq!(a.world().resource::<Settings>().master, 0.0);
        // Escape backs out to the pause menu.
        tap(&mut a, KeyCode::Escape);
        assert_eq!(screen(&a), Screen::Paused);
    }

    #[test]
    fn a_toggle_flips_and_flips_back() {
        let mut a = app(Screen::Options(Back::Pause), tmp_dir("toggle"));
        for _ in 0..3 {
            tap(&mut a, KeyCode::ArrowDown); // Master, Music, Sfx, then Shake
        }
        assert!(a.world().resource::<Settings>().shake);
        tap(&mut a, KeyCode::Enter);
        assert!(!a.world().resource::<Settings>().shake);
        tap(&mut a, KeyCode::ArrowRight);
        assert!(a.world().resource::<Settings>().shake);
    }

    #[test]
    fn rebinding_a_key_takes_the_next_press_and_escape_cancels() {
        let mut a = app(Screen::Controls(Back::Pause), tmp_dir("rebind"));
        // Select "Jump" (5th entry) and start rebinding.
        for _ in 0..4 {
            tap(&mut a, KeyCode::ArrowDown);
        }
        tap(&mut a, KeyCode::Enter);
        assert_eq!(
            a.world().resource::<Menu>().rebinding,
            Some(Action::Jump),
            "waiting for a key"
        );
        tap(&mut a, KeyCode::KeyG);
        assert_eq!(a.world().resource::<Menu>().rebinding, None);
        assert_eq!(
            a.world().resource::<Settings>().key_codes(Action::Jump)[0],
            KeyCode::KeyG
        );

        // Start again and cancel: nothing changes.
        tap(&mut a, KeyCode::Enter);
        assert!(a.world().resource::<Menu>().rebinding.is_some());
        tap(&mut a, KeyCode::Escape);
        assert_eq!(a.world().resource::<Menu>().rebinding, None);
        assert_eq!(
            a.world().resource::<Settings>().key_codes(Action::Jump)[0],
            KeyCode::KeyG
        );
        assert_eq!(
            screen(&a),
            Screen::Controls(Back::Pause),
            "cancelling stays on the controls screen"
        );
    }

    #[test]
    fn the_end_card_appears_after_the_bellwarden_falls_and_only_for_it() {
        let mut a = app(Screen::Playing, tmp_dir("end"));
        a.world_mut()
            .resource_mut::<Messages<BossDefeated>>()
            .write(BossDefeated {
                id: "matron".into(),
                tag: None,
            });
        a.update();
        assert_eq!(
            a.world().resource::<EndTimer>().0,
            None,
            "the Matron is not the end"
        );
        a.world_mut()
            .resource_mut::<Messages<BossDefeated>>()
            .write(BossDefeated {
                id: "bellwarden".into(),
                tag: None,
            });
        a.update();
        assert!(a.world().resource::<EndTimer>().0.is_some());
        a.world_mut().resource_mut::<EndTimer>().0 = Some(0.0);
        a.update();
        assert_eq!(screen(&a), Screen::Ended);
        tap(&mut a, KeyCode::Enter); // "Keep exploring"
        assert_eq!(screen(&a), Screen::Playing);
    }

    #[test]
    fn an_idle_menu_is_not_rebuilt_every_frame() {
        let mut a = app(Screen::Paused, tmp_dir("idle"));
        let ids = |a: &mut App| -> Vec<Entity> {
            a.world_mut()
                .query_filtered::<Entity, With<MenuRoot>>()
                .iter(a.world())
                .collect()
        };
        let before = ids(&mut a);
        assert_eq!(before.len(), 1);
        for _ in 0..10 {
            a.update();
        }
        assert_eq!(ids(&mut a), before, "the same menu entity, untouched");
    }
}
