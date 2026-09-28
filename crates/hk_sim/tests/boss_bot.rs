//! Balance checks: a bot with human-like reactions must be able to beat each
//! boss, in a sensible time, without it being trivial.

mod common;

use bevy_math::Vec2;
use common::*;
use hk_sim::boss::spawn_boss;
use hk_sim::bot::{run_boss_fight, BotConfig, FightResult};
use hk_sim::player::{spawn_player, Abilities};
use hk_sim::rng::SimRng;
use hk_sim::testing::Harness;
use hk_sim::world::TileGrid;

/// A 44 x 14 arena: floor top y = 2, ceiling bottom y = 14.
fn arena() -> Vec<String> {
    let mut v = vec!["#".repeat(44)];
    for _ in 0..11 {
        v.push(format!("#{}#", ".".repeat(42)));
    }
    v.push("#".repeat(44));
    v.push("#".repeat(44));
    v
}

pub fn fight(boss_id: &str, seed: u64, cfg: BotConfig, max_ticks: u64) -> FightResult {
    let rows = arena();
    let rows: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let mut h = Harness::new();
    h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
    *h.world_mut().resource_mut::<SimRng>() = SimRng::new(seed);
    let all = Abilities {
        dash: true,
        wall_grip: true,
    };
    let p = spawn_player(h.world_mut(), Vec2::new(8.0, REST_Y), all);
    let arena_bounds = (Vec2::new(1.0, 2.0), Vec2::new(43.0, 13.0));
    let b = spawn_boss(h.world_mut(), boss_id, Vec2::new(34.0, 2.0), arena_bounds).unwrap();
    h.tick_n(3);
    run_boss_fight(&mut h, p, b, cfg, max_ticks)
}

fn summarize(name: &str, results: &[FightResult]) -> String {
    let wins = results.iter().filter(|r| r.won).count();
    let avg_t: f32 = results.iter().map(|r| r.seconds()).sum::<f32>() / results.len() as f32;
    let avg_hits: f32 =
        results.iter().map(|r| r.hits_taken as f32).sum::<f32>() / results.len() as f32;
    let avg_heals: f32 = results.iter().map(|r| r.heals as f32).sum::<f32>() / results.len() as f32;
    let avg_left: f32 =
        results.iter().map(|r| r.boss_hp_left as f32).sum::<f32>() / results.len() as f32;
    let phases: Vec<u8> = results.iter().map(|r| r.max_phase).collect();
    format!(
        "{name}: won {wins}/{} | avg {avg_t:.0}s | avg hits taken {avg_hits:.1} | avg heals {avg_heals:.1} | avg boss hp left {avg_left:.0} | phases reached {phases:?}",
        results.len()
    )
}

#[test]
#[ignore = "calibration report: cargo test -p hk_sim --test boss_bot report -- --ignored --nocapture"]
fn report() {
    for id in ["matron", "bellwarden"] {
        for mistakes in [0.0f32, 0.05, 0.10] {
            let results: Vec<FightResult> = (1..=16)
                .map(|seed| {
                    fight(
                        id,
                        seed,
                        BotConfig {
                            seed,
                            mistake_rate: mistakes,
                            ..BotConfig::default()
                        },
                        120 * 600,
                    )
                })
                .collect();
            eprintln!(
                "{}",
                summarize(
                    &format!("{id} (mistakes {:.0}%)", mistakes * 100.0),
                    &results
                )
            );
            if mistakes == 0.0 || mistakes == 0.05 {
                let names: Vec<String> = hk_sim::tuning::Tuning::default()
                    .bosses
                    .get(id)
                    .unwrap()
                    .attacks
                    .iter()
                    .map(|a| a.name.clone())
                    .collect();
                let mut hist: std::collections::BTreeMap<String, u32> = Default::default();
                for r in &results {
                    for (_, kind, a, st) in &r.hit_log {
                        let n = a.map_or("(no attack)".to_string(), |i| names[i].clone());
                        *hist
                            .entry(format!("{n} [{kind:?}] during {st:?}"))
                            .or_default() += 1;
                    }
                }
                for (k, v) in &hist {
                    eprintln!("      hit by {k}: {v}");
                }
            }
        }
    }
}

fn wins(id: &str, seeds: std::ops::RangeInclusive<u64>) -> (usize, usize, f32) {
    let results: Vec<FightResult> = seeds
        .map(|seed| {
            fight(
                id,
                seed,
                BotConfig {
                    seed,
                    mistake_rate: 0.0,
                    ..BotConfig::default()
                },
                120 * 600,
            )
        })
        .collect();
    let won: Vec<&FightResult> = results.iter().filter(|r| r.won).collect();
    let fastest = won.iter().map(|r| r.seconds()).fold(f32::MAX, f32::min);
    (won.len(), results.len(), fastest)
}

#[test]
fn matron_is_beatable_but_not_trivial() {
    let (won, total, fastest) = wins("matron", 1..=6);
    assert!(
        won * 2 > total,
        "a clean bot should usually beat the Matron: {won}/{total}"
    );
    assert!(fastest > 30.0, "a Matron kill in {fastest:.0}s is too easy");
}

#[test]
fn bellwarden_is_beatable_but_not_trivial() {
    let (won, total, fastest) = wins("bellwarden", 1..=6);
    assert!(
        won >= 1,
        "a clean bot should be able to beat the Bellwarden: {won}/{total}"
    );
    assert!(
        fastest > 60.0,
        "a Bellwarden kill in {fastest:.0}s is too easy"
    );
}
