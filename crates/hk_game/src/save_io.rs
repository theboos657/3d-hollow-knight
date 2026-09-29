//! Reading and writing the save file (`saves/slot1.ron`), and autosaving
//! whenever something worth keeping happens.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hk_sim::boss::BossDefeated;
use hk_sim::world::progress::{AbilityGained, BenchRested, SaveData};

const FILE: &str = "slot1.ron";

/// `HK_SAVE_DIR` if set, otherwise `saves/` next to the `assets/` directory.
pub fn save_dir(assets_dir: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("HK_SAVE_DIR") {
        return PathBuf::from(p);
    }
    assets_dir
        .parent()
        .map(|p| p.join("saves"))
        .unwrap_or_else(|| PathBuf::from("saves"))
}

pub fn load(dir: &Path) -> Option<SaveData> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    match SaveData::from_ron(&text) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("ignoring unreadable save ({e})");
            None
        }
    }
}

/// Writes atomically (temp file, then rename) so a crash can't leave half a save.
pub fn write(dir: &Path, data: &SaveData) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("could not create {}: {e}", dir.display());
        return;
    }
    let tmp = dir.join(format!("{FILE}.tmp"));
    let dst = dir.join(FILE);
    let result = std::fs::write(&tmp, data.to_ron()).and_then(|_| std::fs::rename(&tmp, &dst));
    if let Err(e) = result {
        eprintln!("could not save: {e}");
    }
}

#[derive(Resource)]
pub struct SaveDir(pub PathBuf);

pub struct SavePlugin {
    pub enabled: bool,
    pub dir: PathBuf,
}

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SaveDir(self.dir.clone()));
        if self.enabled {
            app.add_systems(Update, autosave);
        }
    }
}

/// Sitting at a bench, learning an ability and beating a boss all save.
fn autosave(
    mut commands: Commands,
    dir: Res<SaveDir>,
    mut rested: MessageReader<BenchRested>,
    mut gained: MessageReader<AbilityGained>,
    mut beaten: MessageReader<BossDefeated>,
) {
    let n = rested.read().count() + gained.read().count() + beaten.read().count();
    if n == 0 {
        return;
    }
    let dir = dir.0.clone();
    commands.queue(move |world: &mut World| {
        let data = SaveData::capture(world);
        write(&dir, &data);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;
    use hk_sim::world::progress::SAVE_VERSION;
    use hk_sim::SimPlugin;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hk_save_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn app(dir: &Path) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(SimPlugin)
            .add_plugins(SavePlugin {
                enabled: true,
                dir: dir.to_path_buf(),
            });
        app.world_mut()
            .resource_mut::<hk_sim::world::progress::Checkpoint>()
            .room = "B3".into();
        app.update();
        app
    }

    #[test]
    fn a_bench_rest_writes_a_save_that_loads_back() {
        let dir = tmp("bench");
        let mut a = app(&dir);
        assert!(load(&dir).is_none(), "nothing saved yet");
        a.world_mut()
            .resource_mut::<Messages<BenchRested>>()
            .write(BenchRested);
        a.update();
        let s = load(&dir).expect("autosave wrote a file");
        assert_eq!(s.version, SAVE_VERSION);
        assert_eq!(s.room, "B3");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn learning_an_ability_and_beating_a_boss_also_save() {
        for which in 0..2 {
            let dir = tmp(&format!("other{which}"));
            let mut a = app(&dir);
            if which == 0 {
                a.world_mut()
                    .resource_mut::<Messages<AbilityGained>>()
                    .write(AbilityGained {
                        ability: hk_sim::world::room::Ability::Dash,
                    });
            } else {
                a.world_mut()
                    .resource_mut::<Messages<BossDefeated>>()
                    .write(BossDefeated {
                        id: "matron".into(),
                        tag: Some(7),
                    });
            }
            a.update();
            assert!(load(&dir).is_some(), "event {which} did not save");
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn nothing_happening_writes_nothing() {
        let dir = tmp("quiet");
        let mut a = app(&dir);
        for _ in 0..5 {
            a.update();
        }
        assert!(!dir.join(FILE).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_save_is_ignored_not_fatal() {
        let dir = tmp("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "(this is not a save").unwrap();
        assert!(load(&dir).is_none());
        // ...and a real save can replace it, leaving no temp file behind.
        write(&dir, &SaveData::default());
        assert!(load(&dir).is_some());
        assert!(!dir.join(format!("{FILE}.tmp")).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
