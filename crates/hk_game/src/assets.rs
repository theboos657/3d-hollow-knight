//! Finding the `assets/` directory (tuning and rooms are plain files so they
//! can be edited without rebuilding).

use std::path::PathBuf;

/// `HK_ASSETS` if set, otherwise the nearest `assets/` (containing `rooms/`)
/// above the working directory or the executable.
pub fn find_assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("HK_ASSETS") {
        return PathBuf::from(p);
    }
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        starts.push(d);
    }
    for start in starts {
        let mut dir: Option<PathBuf> = Some(start);
        for _ in 0..6 {
            let Some(d) = dir else { break };
            if d.join("assets").join("rooms").is_dir() {
                return d.join("assets");
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    PathBuf::from("assets")
}
