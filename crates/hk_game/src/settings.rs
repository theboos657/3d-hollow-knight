//! Player settings: volumes, screen shake, vsync and the key bindings. Kept
//! as RON next to the save file, and edited from the options menu.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hk_sim::input::Action;
use serde::{Deserialize, Serialize};

const FILE: &str = "settings.ron";

macro_rules! key_table {
    ($($code:ident => $name:expr),* $(,)?) => {
        /// Every key that can be bound, and the name it is stored and shown under.
        const KEY_TABLE: &[(KeyCode, &str)] = &[$((KeyCode::$code, $name)),*];
    };
}

key_table! {
    KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E", KeyF => "F", KeyG => "G",
    KeyH => "H", KeyI => "I", KeyJ => "J", KeyK => "K", KeyL => "L", KeyM => "M", KeyN => "N",
    KeyO => "O", KeyP => "P", KeyQ => "Q", KeyR => "R", KeyS => "S", KeyT => "T", KeyU => "U",
    KeyV => "V", KeyW => "W", KeyX => "X", KeyY => "Y", KeyZ => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    ArrowLeft => "Left", ArrowRight => "Right", ArrowUp => "Up", ArrowDown => "Down",
    Space => "Space", Enter => "Enter", Tab => "Tab", Backspace => "Backspace",
    ShiftLeft => "L-Shift", ShiftRight => "R-Shift", ControlLeft => "L-Ctrl",
    ControlRight => "R-Ctrl", AltLeft => "L-Alt", AltRight => "R-Alt",
    Comma => "Comma", Period => "Period", Slash => "Slash", Semicolon => "Semicolon",
    Quote => "Quote", BracketLeft => "[", BracketRight => "]", Minus => "Minus", Equal => "Equal",
}

pub fn key_name(k: KeyCode) -> Option<&'static str> {
    KEY_TABLE.iter().find(|(c, _)| *c == k).map(|(_, n)| *n)
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEY_TABLE.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
}

/// How much the renderer is asked to do. Medium is the default; Low is for
/// weak or integrated GPUs, High adds sharper edges and ambient occlusion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }

    /// The next tier, wrapping around (`forward = false` goes the other way).
    pub fn step(self, forward: bool) -> Quality {
        let i = Quality::ALL.iter().position(|q| *q == self).unwrap_or(1);
        let n = Quality::ALL.len();
        Quality::ALL[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }]
    }

    pub fn parse(s: &str) -> Option<Quality> {
        Quality::ALL
            .into_iter()
            .find(|q| q.name().eq_ignore_ascii_case(s))
    }
}

/// A ready-made set of key bindings.
///
/// * **WASD** (the default): the left hand moves and aims (W up, S down), the right
///   hand fights: J attack, K jump, L dash, I Ember Bolt, and F focus. Space, X and
///   Shift still work as second keys, and so do the arrow keys.
/// * **Classic**: arrows move, Z jumps, X attacks, C dashes (WASD and J/Space/Shift
///   are second keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Wasd,
    Classic,
}

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Wasd, Layout::Classic];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Wasd => "WASD",
            Layout::Classic => "Classic",
        }
    }

    /// The bindings, main key first.
    pub fn keys(self) -> Vec<(Action, Vec<String>)> {
        let k = |a: Action, keys: &[&str]| (a, keys.iter().map(|s| s.to_string()).collect());
        match self {
            Layout::Wasd => vec![
                k(Action::Left, &["A", "Left"]),
                k(Action::Right, &["D", "Right"]),
                k(Action::Up, &["W", "Up"]),
                k(Action::Down, &["S", "Down"]),
                k(Action::Jump, &["K", "Space"]),
                k(Action::Attack, &["J", "X"]),
                k(Action::Dash, &["L", "L-Shift"]),
                k(Action::Focus, &["F", "U"]),
                k(Action::Cast, &["I", "V"]),
            ],
            Layout::Classic => vec![
                k(Action::Left, &["Left", "A"]),
                k(Action::Right, &["Right", "D"]),
                k(Action::Up, &["Up", "W"]),
                k(Action::Down, &["Down", "S"]),
                k(Action::Jump, &["Space", "Z"]),
                k(Action::Attack, &["X", "J"]),
                k(Action::Dash, &["C", "L-Shift"]),
                k(Action::Focus, &["F"]),
                k(Action::Cast, &["V"]),
            ],
        }
    }

    /// The other layout (the menu cycles through them).
    pub fn other(self) -> Layout {
        match self {
            Layout::Wasd => Layout::Classic,
            Layout::Classic => Layout::Wasd,
        }
    }
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
    pub shake: bool,
    pub vsync: bool,
    pub quality: Quality,
    /// Tutorial prompts the player has already been shown.
    pub tips_seen: Vec<crate::tutorial::Tip>,
    /// Keys per action (by name); the first is the one the menu rebinds.
    pub keys: Vec<(Action, Vec<String>)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master: 0.8,
            music: 0.6,
            sfx: 0.9,
            shake: true,
            vsync: true,
            quality: Quality::Medium,
            tips_seen: Vec::new(),
            keys: Layout::Wasd.keys(),
        }
    }
}

impl Settings {
    /// Which ready-made layout the bindings match exactly (`None` after custom rebinds).
    pub fn layout(&self) -> Option<Layout> {
        Layout::ALL.into_iter().find(|l| self.keys == l.keys())
    }

    /// The name of the main key of `a` (what a short prompt should show).
    pub fn main_key(&self, a: Action) -> String {
        self.keys
            .iter()
            .find(|(x, _)| *x == a)
            .and_then(|(_, names)| names.first().cloned())
            .unwrap_or_default()
    }

    /// Switches every binding to `layout` (custom rebinds are replaced).
    pub fn apply_layout(&mut self, layout: Layout) {
        self.keys = layout.keys();
    }

    pub fn key_codes(&self, a: Action) -> Vec<KeyCode> {
        self.keys
            .iter()
            .find(|(x, _)| *x == a)
            .map(|(_, names)| names.iter().filter_map(|n| key_from_name(n)).collect())
            .unwrap_or_default()
    }

    /// What the menu shows for an action: its keys, e.g. "Space / Z".
    pub fn label(&self, a: Action) -> String {
        self.keys
            .iter()
            .find(|(x, _)| *x == a)
            .map(|(_, names)| names.join(" / "))
            .unwrap_or_default()
    }

    /// Makes `key` the main key for `action`. If another action was using it,
    /// that action loses it (one key never does two jobs), and one that would
    /// be left with nothing gets a spare key.
    pub fn bind(&mut self, action: Action, key: KeyCode) {
        let Some(name) = key_name(key) else {
            return;
        };
        for (a, names) in self.keys.iter_mut() {
            if *a != action {
                names.retain(|n| n != name);
            }
        }
        if let Some((_, names)) = self.keys.iter_mut().find(|(a, _)| *a == action) {
            names.retain(|n| n != name);
            names.insert(0, name.to_string());
            names.truncate(2);
        }
        // Anything left bare picks a spare key.
        const SPARES: [&str; 8] = ["R", "T", "G", "H", "Y", "U", "B", "N"];
        for i in 0..self.keys.len() {
            if self.keys[i].1.is_empty() {
                let used: Vec<String> = self.keys.iter().flat_map(|(_, n)| n.clone()).collect();
                if let Some(spare) = SPARES.iter().find(|s| !used.iter().any(|u| u == *s)) {
                    self.keys[i].1.push(spare.to_string());
                }
            }
        }
    }

    pub fn clamp(&mut self) {
        for v in [&mut self.master, &mut self.music, &mut self.sfx] {
            *v = v.clamp(0.0, 1.0);
        }
    }
}

pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn load(dir: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(settings_path(dir)) else {
        return Settings::default();
    };
    match ron::from_str::<Settings>(&text) {
        Ok(mut s) => {
            s.clamp();
            // Actions missing from an old file fall back to their defaults.
            for (a, keys) in Settings::default().keys {
                if !s.keys.iter().any(|(x, _)| *x == a) {
                    s.keys.push((a, keys));
                }
            }
            s
        }
        Err(e) => {
            eprintln!("ignoring unreadable settings ({e})");
            Settings::default()
        }
    }
}

pub fn save(dir: &Path, s: &Settings) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    match ron::ser::to_string_pretty(s, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(settings_path(dir), text) {
                eprintln!("could not save settings: {e}");
            }
        }
        Err(e) => eprintln!("could not serialise settings: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_a_ron_round_trip_and_bind_real_keys() {
        let s = Settings::default();
        let text = ron::ser::to_string_pretty(&s, ron::ser::PrettyConfig::default()).unwrap();
        let back: Settings = ron::from_str(&text).unwrap();
        assert_eq!(s, back);
        for a in Action::ALL {
            assert!(!s.key_codes(a).is_empty(), "{a:?} has no key");
        }
    }

    #[test]
    fn both_layouts_bind_real_keys_once_each() {
        for layout in Layout::ALL {
            let keys = layout.keys();
            let mut seen = std::collections::HashSet::new();
            for a in Action::ALL {
                let names = &keys.iter().find(|(x, _)| *x == a).expect("every action").1;
                assert!(!names.is_empty(), "{layout:?}: {a:?} has no key");
                for n in names {
                    assert!(
                        key_from_name(n).is_some(),
                        "{layout:?}: `{n}` is not a bindable key"
                    );
                    assert!(seen.insert(n.clone()), "{layout:?}: `{n}` is bound twice");
                }
            }
        }
    }

    #[test]
    fn the_main_key_is_the_first_one() {
        let mut s = Settings::default();
        assert_eq!(s.main_key(Action::Attack), "J");
        s.apply_layout(Layout::Classic);
        assert_eq!(s.main_key(Action::Attack), "X");
    }

    #[test]
    fn the_default_is_a_full_wasd_layout() {
        let s = Settings::default();
        assert_eq!(s.layout(), Some(Layout::Wasd));
        let main = |a: Action| s.key_codes(a)[0];
        assert_eq!(main(Action::Left), KeyCode::KeyA);
        assert_eq!(main(Action::Right), KeyCode::KeyD);
        assert_eq!(main(Action::Up), KeyCode::KeyW);
        assert_eq!(main(Action::Down), KeyCode::KeyS);
        assert_eq!(main(Action::Attack), KeyCode::KeyJ);
        assert_eq!(main(Action::Jump), KeyCode::KeyK);
        // The keys people already know still work as second keys.
        assert!(s.key_codes(Action::Left).contains(&KeyCode::ArrowLeft));
        assert!(s.key_codes(Action::Jump).contains(&KeyCode::Space));
        assert!(s.key_codes(Action::Attack).contains(&KeyCode::KeyX));
        assert!(s.key_codes(Action::Dash).contains(&KeyCode::ShiftLeft));
    }

    #[test]
    fn layouts_are_recognised_switched_and_custom_rebinds_are_noticed() {
        let mut s = Settings::default();
        s.apply_layout(Layout::Classic);
        assert_eq!(s.layout(), Some(Layout::Classic));
        assert_eq!(s.key_codes(Action::Jump)[0], KeyCode::Space);
        assert_eq!(s.key_codes(Action::Left)[0], KeyCode::ArrowLeft);
        s.apply_layout(Layout::Classic.other());
        assert_eq!(s.layout(), Some(Layout::Wasd));
        s.bind(Action::Jump, KeyCode::KeyG);
        assert_eq!(s.layout(), None, "a rebind makes it custom");
        // A settings file from before layouts existed (arrow keys first) is Classic.
        let old: Settings = ron::from_str(
            "(keys: [(Left, [\"Left\", \"A\"]), (Right, [\"Right\", \"D\"]), (Up, [\"Up\", \"W\"]), (Down, [\"Down\", \"S\"]), (Jump, [\"Space\", \"Z\"]), (Attack, [\"X\", \"J\"]), (Dash, [\"C\", \"L-Shift\"]), (Focus, [\"F\"]), (Cast, [\"V\"])])",
        )
        .unwrap();
        assert_eq!(old.layout(), Some(Layout::Classic));
    }

    #[test]
    fn quality_steps_wrap_and_parse_by_name() {
        assert_eq!(Quality::Low.step(true), Quality::Medium);
        assert_eq!(Quality::High.step(true), Quality::Low);
        assert_eq!(Quality::Low.step(false), Quality::High);
        assert_eq!(Quality::parse("high"), Some(Quality::High));
        assert_eq!(Quality::parse("MEDIUM"), Some(Quality::Medium));
        assert_eq!(Quality::parse("ultra"), None);
        // Old settings files (from before there was a quality setting) get Medium.
        let s: Settings = ron::from_str("(master: 0.5, vsync: false)").unwrap();
        assert_eq!(s.quality, Quality::Medium);
    }

    #[test]
    fn a_partial_file_falls_back_to_defaults() {
        let s: Settings = ron::from_str("(master: 0.3)").unwrap();
        assert_eq!(s.master, 0.3);
        assert_eq!(s.sfx, Settings::default().sfx);
    }

    #[test]
    fn rebinding_moves_a_key_and_never_leaves_an_action_bare() {
        let mut s = Settings::default();
        // Give Jump the key Attack uses; Attack keeps its other key.
        s.bind(Action::Jump, KeyCode::KeyX);
        assert_eq!(s.key_codes(Action::Jump)[0], KeyCode::KeyX);
        assert!(!s.key_codes(Action::Attack).contains(&KeyCode::KeyX));
        assert!(!s.key_codes(Action::Attack).is_empty());
        // Steal the only key of Focus (F): it falls back to something.
        s.bind(Action::Cast, KeyCode::KeyF);
        assert!(!s.key_codes(Action::Focus).is_empty());
        assert_ne!(s.key_codes(Action::Focus)[0], KeyCode::KeyF);
        // Unbindable keys are ignored.
        let before = s.clone();
        s.bind(Action::Dash, KeyCode::F5);
        assert_eq!(s, before);
    }

    #[test]
    fn key_names_are_unique_and_round_trip() {
        let mut seen = std::collections::HashSet::new();
        for (code, name) in KEY_TABLE {
            assert!(seen.insert(*name), "duplicate key name {name}");
            assert_eq!(key_from_name(name), Some(*code));
        }
    }
}
