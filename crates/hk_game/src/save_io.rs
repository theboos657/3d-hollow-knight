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
struct SaveDir(PathBuf);

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
