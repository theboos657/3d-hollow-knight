//! Plays the map out on paper and complains about level-design mistakes.
//!
//! Starting from the first room with no abilities, it runs the real player
//! controller over every room (see `hk_sim::reach`), follows every exit it
//! can reach, collects every pickup and boss reward it can reach, and repeats
//! with the new abilities. It reports:
//!
//! * rooms that can never be reached, exits/pickups/benches that can never be
//!   touched, and spots you can get stuck in for good (softlocks);
//! * which stage (which set of abilities) each room first opens up in, so you
//!   can see that the gates really gate.
//!
//! Usage: `cargo run --release -p hk_tools --bin lint_rooms [-- --assets DIR
//! --start ROOM:ENTRY --ignore a,b]`. Exits with status 1 if anything is wrong.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use hk_sim::player::Abilities;
use hk_sim::reach::{analyse_room, analyse_world};
use hk_sim::tuning::Tuning;
use hk_sim::world::room::{Ability, RoomLibrary};

fn find_assets() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_default();
    for _ in 0..6 {
        if dir.join("assets").join("rooms").is_dir() {
            return dir.join("assets");
        }
        if !dir.pop() {
            break;
        }
    }
    PathBuf::from("assets")
}

fn ability_name(a: Ability) -> &'static str {
    match a {
        Ability::Dash => "Dash",
        Ability::WallGrip => "Wall Grip",
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_of = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let assets = value_of("--assets")
        .map(PathBuf::from)
        .unwrap_or_else(find_assets);
    let start = value_of("--start").unwrap_or_else(|| "A1:start".to_string());
    let (start_room, start_entry) = start.split_once(':').unwrap_or((&start, "start"));
    let mut ignore: Vec<String> = vec![
        "sandbox".into(),
        "dev_matron".into(),
        "dev_bellwarden".into(),
    ];
    if let Some(list) = value_of("--ignore") {
        ignore.extend(list.split(',').map(str::to_owned));
    }
    let ignore_refs: Vec<&str> = ignore.iter().map(|s| s.as_str()).collect();

    let (tuning, warnings) = Tuning::load_dir(&assets.join("tuning"));
    for w in warnings {
        println!("tuning warning: {w}");
    }
    let library = match RoomLibrary::load_dir(&assets.join("rooms")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not load rooms: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = library.validate() {
        eprintln!("INVALID: {e}");
        return ExitCode::FAILURE;
    }
    println!(
        "{} rooms loaded from {}",
        library.len(),
        assets.join("rooms").display()
    );

    // `--map ROOM[:ENTRY] [--with dash,grip]`: draw where the player can stand.
    if let Some(spec) = value_of("--map") {
        let (room, entry) = spec.split_once(':').unwrap_or((&spec, "start"));
        let with = value_of("--with").unwrap_or_default();
        let abilities = Abilities {
            dash: with.contains("dash"),
            wall_grip: with.contains("grip"),
        };
        let Some(def) = library.get(room) else {
            eprintln!("no room `{room}`");
            return ExitCode::FAILURE;
        };
        let entry = match def.entry(entry).or_else(|| def.entries.first()) {
            Some(e) => e.clone(),
            None => {
                eprintln!("room `{room}` has no entries");
                return ExitCode::FAILURE;
            }
        };
        let reach = analyse_room(
            def,
            &tuning,
            abilities,
            bevy_math::Vec2::new(entry.at.0, entry.at.1),
        );
        let mut rows: Vec<Vec<char>> = def.tiles.iter().map(|r| r.chars().collect()).collect();
        let h = rows.len();
        for n in &reach.nodes {
            let (i, j) = (n.feet.x.floor() as usize, n.feet.y.round() as usize);
            // The standing tile is the one just above the surface.
            if j < h && i < rows[0].len() {
                let row = h - 1 - j;
                if rows[row][i] == '.' {
                    rows[row][i] = 'o';
                }
            }
        }
        println!(
            "{room} from `{}` with {}{}{}: {} standing spots ('o'), {} softlock spots",
            entry.name,
            if abilities.dash { "dash " } else { "" },
            if abilities.wall_grip { "grip " } else { "" },
            if with.is_empty() { "no abilities" } else { "" },
            reach.nodes.len(),
            reach.trap_count,
        );
        for (j, row) in rows.iter().enumerate() {
            println!("{:3} {}", h - 1 - j, row.iter().collect::<String>());
        }
        for (i, x) in def.exits.iter().enumerate() {
            println!(
                "exit #{i} -> {}: {}",
                x.to,
                if reach.exits[i] {
                    "reachable"
                } else {
                    "NOT reachable"
                }
            );
        }
        return ExitCode::SUCCESS;
    }

    let t0 = Instant::now();
    let report = match analyse_world(&library, &tuning, (start_room, start_entry), &ignore_refs) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::FAILURE;
        }
    };

    for (i, stage) in report.stages.iter().enumerate() {
        let mut have: Vec<&str> = Vec::new();
        if stage.abilities.dash {
            have.push("Dash");
        }
        if stage.abilities.wall_grip {
            have.push("Wall Grip");
        }
        let what = if have.is_empty() {
            "no abilities".to_string()
        } else {
            have.join(" + ")
        };
        println!("\nStage {i}: {what}");
        println!("  new rooms: {}", stage.new_rooms.join(", "));
        for a in &stage.gained {
            println!("  -> can now collect {}", ability_name(*a));
        }
    }

    let mut bad = false;
    if !report.unreachable.is_empty() {
        bad = true;
        println!("\nUNREACHABLE rooms: {}", report.unreachable.join(", "));
    }
    for (room, i) in &report.dead_exits {
        bad = true;
        println!("DEAD EXIT: {room} exit #{i} can never be touched");
    }
    for (room, i) in &report.dead_pickups {
        bad = true;
        println!("DEAD PICKUP: {room} pickup #{i} can never be collected");
    }
    for (room, i) in &report.dead_benches {
        bad = true;
        println!("DEAD BENCH: {room} bench #{i} can never be reached");
    }
    for (room, at) in &report.traps {
        bad = true;
        println!(
            "SOFTLOCK: {room} near ({:.1}, {:.1}) has no way out",
            at.x, at.y
        );
    }
    println!("\nanalysed in {:.1}s", t0.elapsed().as_secs_f32());
    if bad {
        println!("FAIL");
        ExitCode::FAILURE
    } else {
        println!("OK: every room is reachable, in order, with no softlocks.");
        ExitCode::SUCCESS
    }
}
