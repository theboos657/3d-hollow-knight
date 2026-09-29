//! Fire and light in the level: braziers that pool warm light on the stone and
//! flicker, placed where the floor is open enough to show them off.

use bevy::prelude::*;
use hk_sim::world::grid::{Tile, TileGrid};

use crate::rig::meshkit::{cone, hash3, lathe, MeshData};

/// The iron stand and the flame of a brazier (feet at the origin, ~1.1 tall).
pub fn brazier_meshes() -> (MeshData, MeshData) {
    let stand = lathe(
        &[
            (0.0, 0.0),
            (0.20, 0.0),
            (0.12, 0.05),
            (0.06, 0.30),
            (0.10, 0.38),
            (0.24, 0.54),
            (0.27, 0.62),
            (0.21, 0.62),
            (0.0, 0.58),
        ],
        14,
    );
    let mut flame =
        cone(0.19, 0.55, 8).transformed(Mat4::from_translation(Vec3::new(0.0, 0.58, 0.0)));
    flame.merge(
        &cone(0.11, 0.42, 8).transformed(Mat4::from_translation(Vec3::new(0.02, 0.60, 0.0))),
    );
    (stand, flame)
}

/// Tiles where a brazier can stand: a floor with plenty of open air above,
/// nothing hazardous next to it, spaced well apart. Deterministic in `seed`.
pub fn pick_spots(grid: &TileGrid, seed: u32, max: usize) -> Vec<(i32, i32)> {
    let mut candidates: Vec<(i32, i32)> = Vec::new();
    for j in 0..grid.height() {
        for i in 1..grid.width() - 1 {
            let floor = grid.get(i, j) == Tile::Solid;
            // Three open tiles above, inside the map (not the roof's outer face).
            let open = j + 3 < grid.height() && (1..=3).all(|k| grid.get(i, j + k) == Tile::Empty);
            let calm = (-1..=1).all(|d| grid.get(i + d, j + 1) != Tile::Spike);
            if floor && open && calm {
                candidates.push((i, j));
            }
        }
    }
    // Shuffle by hash, then take greedily with a minimum spacing.
    candidates.sort_by(|a, b| {
        let (ha, hb) = (hash3(seed, a.0, a.1, 11), hash3(seed, b.0, b.1, 11));
        ha.total_cmp(&hb)
    });
    let mut chosen: Vec<(i32, i32)> = Vec::new();
    for c in candidates {
        let far = chosen
            .iter()
            .all(|p| (p.0 - c.0).abs() >= 13 || (p.1 - c.1).abs() >= 8);
        if far {
            chosen.push(c);
            if chosen.len() == max {
                break;
            }
        }
    }
    chosen.sort();
    chosen
}

/// A flickering flame light.
#[derive(Component)]
pub struct Flame {
    pub base: f32,
    pub phase: f32,
}

/// The multiplier a flame's light takes at time `t`: a sum of sines that never
/// repeats visibly and stays within roughly 0.7..1.2.
pub fn flicker_factor(t: f32, phase: f32) -> f32 {
    0.92 + 0.10 * (t * 7.3 + phase).sin()
        + 0.07 * (t * 13.1 + phase * 2.3).sin()
        + 0.04 * (t * 29.0 + phase * 0.7).sin()
}

pub fn flicker(time: Res<Time>, mut q: Query<(&Flame, &mut PointLight)>) {
    let t = time.elapsed_secs();
    for (f, mut l) in &mut q {
        l.intensity = f.base * flicker_factor(t, f.phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hall() -> TileGrid {
        // 60 wide, 12 high; a floor with a pit of spikes in the middle.
        let mut rows: Vec<String> = vec![".".repeat(60); 10];
        rows.push(format!("{}{}{}", "#".repeat(28), "^^^^", "#".repeat(28)));
        rows.push("#".repeat(60));
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        TileGrid::from_ascii(&refs)
    }

    #[test]
    fn braziers_stand_on_open_floor_far_apart_and_away_from_spikes() {
        let g = hall();
        let spots = pick_spots(&g, 5, 6);
        assert!(!spots.is_empty() && spots.len() <= 6);
        for &(i, j) in &spots {
            assert_eq!(g.get(i, j), Tile::Solid);
            assert_eq!(g.get(i, j + 1), Tile::Empty);
            assert!((-1..=1).all(|d| g.get(i + d, j + 1) != Tile::Spike));
        }
        for (a, b) in spots.iter().zip(spots.iter().skip(1)) {
            assert!(
                (a.0 - b.0).abs() >= 13 || (a.1 - b.1).abs() >= 8,
                "{a:?} and {b:?} too close"
            );
        }
        assert_eq!(spots, pick_spots(&g, 5, 6), "deterministic");
    }

    #[test]
    fn a_room_with_no_open_floor_has_no_braziers() {
        let g = TileGrid::from_ascii(&["####", "####", "####"]);
        assert!(pick_spots(&g, 1, 4).is_empty());
    }

    #[test]
    fn flames_flicker_within_bounds() {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for k in 0..2000 {
            let v = flicker_factor(k as f32 * 0.01, 0.7);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo > 0.6 && hi < 1.3, "{lo}..{hi}");
        assert!(hi - lo > 0.15, "it must actually flicker");
    }

    #[test]
    fn the_brazier_meshes_are_well_formed() {
        let (stand, flame) = brazier_meshes();
        stand.validate().expect("stand");
        flame.validate().expect("flame");
    }
}
