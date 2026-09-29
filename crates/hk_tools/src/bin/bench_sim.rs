//! Simulation performance check. The sim must fit comfortably inside a 120 Hz
//! tick (8.3 ms), leaving the rest of the frame to rendering. This runs the
//! busiest rooms and a full boss fight and reports microseconds per tick.
//!
//! Usage: `cargo run --release -p hk_tools --bin bench_sim`
//! Exits with status 1 if a budget is blown.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use bevy_math::Vec2;
use hk_sim::boss::spawn_boss;
use hk_sim::bot::{Bot, BotConfig};
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::testing::Harness;
use hk_sim::tuning::Tuning;
use hk_sim::world::room::{enter_room, RoomLibrary};

/// Mean and 99th-percentile budgets, microseconds per tick (release build).
const MEAN_BUDGET_US: f64 = 100.0;
const P99_BUDGET_US: f64 = 500.0;

struct Stats {
    mean: f64,
    p99: f64,
    max: f64,
}

fn measure(h: &mut Harness, ticks: usize, mut before: impl FnMut(&mut Harness)) -> Stats {
    let mut times = Vec::with_capacity(ticks);
    for _ in 0..ticks {
        before(h);
        let t = Instant::now();
        h.tick();
        times.push(t.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    Stats {
        mean: times.iter().sum::<f64>() / times.len() as f64,
        p99: times[(times.len() as f64 * 0.99) as usize],
        max: *times.last().unwrap(),
    }
}

fn assets() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        if dir.join("assets").join("rooms").is_dir() {
            return dir.join("assets");
        }
        if !dir.pop() {
            return PathBuf::from("assets");
        }
    }
}

fn main() -> ExitCode {
    let assets = assets();
    let (tuning, _) = Tuning::load_dir(&assets.join("tuning"));
    let library = RoomLibrary::load_dir(&assets.join("rooms")).expect("rooms load");
    let all = Abilities {
        dash: true,
        wall_grip: true,
    };
    let mut ok = true;
    let mut report = |name: &str, s: Stats| {
        let pass = s.mean <= MEAN_BUDGET_US && s.p99 <= P99_BUDGET_US;
        println!(
            "{name:<34} mean {:>7.1} us   p99 {:>7.1} us   max {:>8.1} us   {}",
            s.mean,
            s.p99,
            s.max,
            if pass { "ok" } else { "OVER BUDGET" }
        );
        ok &= pass;
    };

    // Rooms with the most creatures, the player standing among them.
    for (room, entry) in [
        ("D3", "west"),
        ("B2", "west"),
        ("C2", "west"),
        ("sandbox", "start"),
    ] {
        let mut h = Harness::new();
        h.world_mut().insert_resource(tuning.clone());
        h.world_mut().insert_resource(library.clone());
        spawn_player(h.world_mut(), Vec2::ZERO, all);
        enter_room(h.world_mut(), room, entry).expect("room");
        // Stand in the middle of the creatures so they all have something to do.
        let spots: Vec<Vec2> = h
            .world_mut()
            .query_filtered::<&hk_sim::components::SimPos, bevy_ecs::prelude::With<hk_sim::enemy::Enemy>>()
            .iter(h.world())
            .map(|p| p.0)
            .collect();
        if !spots.is_empty() {
            let mid = spots.iter().fold(Vec2::ZERO, |a, b| a + *b) / spots.len() as f32;
            let p = hk_sim::bot::find_player(h.world_mut()).expect("player");
            h.world_mut()
                .get_mut::<hk_sim::components::SimPos>(p)
                .unwrap()
                .0 = mid + Vec2::Y;
            println!("room {room}: {} creatures around the player", spots.len());
        }
        h.tick_n(30);
        let s = measure(&mut h, 20_000, |_| {});
        report(&format!("room {room} (20k ticks)"), s);
    }

    // A whole boss fight, driven by the bot.
    for id in ["matron", "bellwarden"] {
        let mut h = Harness::new();
        h.world_mut().insert_resource(tuning.clone());
        h.world_mut().insert_resource(library.clone());
        let rows: Vec<String> = {
            let mut v = vec!["#".repeat(44)];
            v.extend((0..11).map(|_| format!("#{}#", ".".repeat(42))));
            v.push("#".repeat(44));
            v.push("#".repeat(44));
            v
        };
        let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
        h.world_mut()
            .insert_resource(hk_sim::world::TileGrid::from_ascii(&refs));
        let p = spawn_player(h.world_mut(), Vec2::new(8.0, 2.8), all);
        let b = spawn_boss(
            h.world_mut(),
            id,
            Vec2::new(34.0, 2.0),
            (Vec2::new(1.0, 2.0), Vec2::new(43.0, 13.0)),
        )
        .expect("boss");
        h.tick_n(3);
        let mut bot = Bot::new(BotConfig::default());
        let mut bits;
        let s = {
            let mut times = Vec::new();
            for _ in 0..20_000 {
                bits = bot.decide(h.world_mut(), p, b);
                let tick = hk_sim::SimTick(h.tick_count());
                let mut input =
                    std::mem::take(&mut *h.world_mut().resource_mut::<hk_sim::input::InputState>());
                hk_sim::input::apply_bits(&mut input, bits, &tick);
                *h.world_mut().resource_mut::<hk_sim::input::InputState>() = input;
                let t = Instant::now();
                h.tick();
                times.push(t.elapsed().as_secs_f64() * 1e6);
            }
            times.sort_by(|a, b| a.total_cmp(b));
            Stats {
                mean: times.iter().sum::<f64>() / times.len() as f64,
                p99: times[(times.len() as f64 * 0.99) as usize],
                max: *times.last().unwrap(),
            }
        };
        report(&format!("boss fight: {id} (20k ticks)"), s);
    }

    println!(
        "\nbudget: mean <= {MEAN_BUDGET_US} us, p99 <= {P99_BUDGET_US} us (a tick has 8333 us at 120 Hz)"
    );
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
