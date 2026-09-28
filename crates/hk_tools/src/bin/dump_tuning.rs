//! Writes the built-in default tuning as RON files, so the shipped files are
//! generated from the code (never hand-copied) and cannot drift from it.
//!
//! Usage: `cargo run -p hk_tools --bin dump_tuning -- assets/tuning [--force]`
//! Without `--force` an existing file is left alone.

use std::path::Path;

use hk_sim::tuning::Tuning;
use serde::Serialize;

fn write<T: Serialize>(dir: &Path, name: &str, value: &T, header: &str, force: bool) {
    let path = dir.join(name);
    if path.exists() && !force {
        println!("kept    {}", path.display());
        return;
    }
    let cfg = ron::ser::PrettyConfig::default().indentor("    ");
    let body = ron::ser::to_string_pretty(value, cfg).expect("serialize");
    std::fs::write(&path, format!("{header}\n{body}\n")).expect("write file");
    println!("wrote   {}", path.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("assets/tuning");
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).expect("create dir");

    let t = Tuning::default();
    write(
        dir,
        "bosses.ron",
        &t.bosses,
        "// Boss definitions: HP, phases and attack timings (ms). Times are authored here and\n\
         // converted to ticks in code. Regenerate from the code defaults with:\n\
         //   cargo run -p hk_tools --bin dump_tuning -- assets/tuning --force",
        force,
    );
}
