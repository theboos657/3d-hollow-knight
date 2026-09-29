//! Small things that make surfaces feel lived-in: tufts on the floors,
//! rubble, stalactites and hanging chains or roots on the ceilings. All of it
//! is seeded from the room's id, never touches collision, and stays out of the
//! way of the knight (low in front of the play lane, taller behind it).

use bevy::math::{Mat4, Vec3};
use hk_sim::world::grid::{Tile, TileGrid};
use hk_sim::world::room::Theme;

use super::kits::{chain, root, stalactite};
use crate::rig::meshkit::{cone, ellipsoid, hash3, MeshData};

/// The decorations of a room, merged by kind so each is one draw call.
#[derive(Default)]
pub struct Decor {
    /// Grass, moss, fronds, shards: what grows on the floors.
    pub growth: MeshData,
    /// Loose stones.
    pub rubble: MeshData,
    /// Stalactites, chains and roots hanging from the ceilings.
    pub hangers: MeshData,
}

/// How thickly a theme's floors are overgrown, 0..1.
fn lushness(t: Theme) -> f32 {
    match t {
        Theme::Sandbox => 0.25,
        Theme::Ashen => 0.35,
        Theme::Warrens => 0.9,
        Theme::Cistern => 0.6,
        Theme::Spire => 0.3,
        Theme::Throne => 0.45,
    }
}

/// The colour of a blade of growth at height `t` (0 root, 1 tip).
fn growth_colour(theme: Theme, t: f32, tint: f32) -> [f32; 4] {
    let (root, tip) = match theme {
        Theme::Sandbox => ([0.30, 0.34, 0.40], [0.60, 0.72, 0.85]),
        Theme::Ashen => ([0.28, 0.22, 0.16], [0.72, 0.60, 0.40]),
        Theme::Warrens => ([0.10, 0.28, 0.10], [0.55, 1.10, 0.45]),
        Theme::Cistern => ([0.08, 0.26, 0.28], [0.40, 0.95, 0.90]),
        Theme::Spire => ([0.26, 0.20, 0.34], [0.85, 0.75, 1.0]),
        Theme::Throne => ([0.30, 0.10, 0.08], [1.10, 0.45, 0.28]),
    };
    let c = |a: f32, b: f32| (a + (b - a) * t) * tint;
    [
        c(root[0], tip[0]),
        c(root[1], tip[1]),
        c(root[2], tip[2]),
        1.0,
    ]
}

/// Builds the decor for `grid`.
pub fn build_decor(grid: &TileGrid, theme: Theme, seed: u32) -> Decor {
    let mut d = Decor::default();
    let lush = lushness(theme);
    let solid = |i: i32, j: i32| grid.get(i, j) == Tile::Solid;
    for j in 0..grid.height() {
        for i in 0..grid.width() {
            if !solid(i, j) {
                continue;
            }
            let h = |salt: i32| hash3(seed, i, j, 100 + salt);
            // ---- floors (air above) ----
            if !solid(i, j + 1) && grid.get(i, j + 1) != Tile::Spike && j + 1 < grid.height() {
                // Tufts: a few leaning blades. Behind the lane (z < 0) they may
                // stand tall; in front of it they stay ankle-high.
                if h(0) < lush {
                    let blades = 2 + (h(1) * 4.0) as i32;
                    for b in 0..blades {
                        let bh = |salt: i32| hash3(seed, i * 7 + b, j, 200 + salt);
                        let x = i as f32 + 0.08 + 0.84 * bh(0);
                        let z = -0.9 + 1.6 * bh(1);
                        let max_h = if z > 0.05 { 0.16 } else { 0.42 };
                        let height = 0.10 + (max_h - 0.10) * bh(2);
                        let lean = (bh(3) - 0.5) * 0.7;
                        let tint = 0.8 + 0.4 * bh(4);
                        let blade = cone(0.035 + 0.02 * bh(5), height, 4).recolor(move |p| {
                            growth_colour(theme, (p.y / height).clamp(0.0, 1.0), tint)
                        });
                        d.growth.merge(&blade.transformed(
                            Mat4::from_translation(Vec3::new(x, j as f32 + 1.0, z))
                                * Mat4::from_rotation_z(lean),
                        ));
                    }
                }
                // Rubble.
                if h(2) < 0.22 {
                    let r = 0.05 + 0.09 * h(3);
                    let g = 0.35 + 0.3 * h(4);
                    d.rubble.merge(
                        &ellipsoid(r * 1.4, r, r, 5, 8)
                            .recolor(move |_| [g, g * 0.95, g * 0.9, 1.0])
                            .transformed(Mat4::from_translation(Vec3::new(
                                i as f32 + 0.1 + 0.8 * h(5),
                                j as f32 + 1.0 + r * 0.4,
                                -0.6 + 1.0 * h(6),
                            ))),
                    );
                }
            }
            // ---- ceilings (air below) ----
            let below_open = !solid(i, j - 1) && grid.get(i, j - 1) != Tile::Spike && j > 0;
            if below_open {
                if h(7) < 0.3 {
                    let len = 0.4 + 1.3 * h(8);
                    let r = 0.11 + 0.16 * h(9);
                    let g = 0.45 + 0.3 * h(10);
                    d.hangers.merge(
                        &stalactite(
                            i as f32 + 0.15 + 0.7 * h(11),
                            j as f32,
                            len,
                            r,
                            -0.9 + 0.9 * h(12),
                        )
                        .recolor(move |p| {
                            let k = (-p.y / len).clamp(0.0, 1.0);
                            let v = g * (1.0 - 0.3 * k);
                            [v, v * 0.96, v * 0.92, 1.0]
                        }),
                    );
                } else if h(13) < 0.12 {
                    let x = i as f32 + 0.5;
                    let len = 1.5 + 3.5 * h(14);
                    let z = -0.8 - 0.4 * h(15);
                    let piece = match theme {
                        Theme::Warrens => root(x, j as f32, len, z, h(16) * 6.0),
                        Theme::Throne => continue,
                        _ => chain(x, j as f32, len, z),
                    };
                    d.hangers.merge(&piece.recolor(move |p| {
                        let k = (-(p.y - j as f32) / len).clamp(0.0, 1.0);
                        let v = 0.55 - 0.25 * k;
                        if theme == Theme::Warrens {
                            [v * 0.5, v * 1.1, v * 0.5, 1.0]
                        } else {
                            [v, v, v * 1.05, 1.0]
                        }
                    }));
                }
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cave() -> TileGrid {
        let mut rows: Vec<String> = vec!["#".repeat(50)];
        for _ in 0..8 {
            rows.push(format!("#{}#", ".".repeat(48)));
        }
        rows.push(format!("#{}{}{}#", ".".repeat(20), "^^^^", ".".repeat(24)));
        rows.push("#".repeat(50));
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        TileGrid::from_ascii(&refs)
    }

    #[test]
    fn decor_meshes_are_well_formed() {
        for t in [
            Theme::Ashen,
            Theme::Warrens,
            Theme::Cistern,
            Theme::Spire,
            Theme::Throne,
        ] {
            let d = build_decor(&cave(), t, 3);
            for (n, m) in [
                ("growth", &d.growth),
                ("rubble", &d.rubble),
                ("hangers", &d.hangers),
            ] {
                if m.vertex_count() > 0 {
                    m.validate().unwrap_or_else(|e| panic!("{t:?} {n}: {e}"));
                }
            }
        }
    }

    #[test]
    fn decor_is_deterministic_and_depends_on_the_room() {
        let a = build_decor(&cave(), Theme::Warrens, 1);
        let b = build_decor(&cave(), Theme::Warrens, 1);
        let c = build_decor(&cave(), Theme::Warrens, 2);
        assert_eq!(a.growth.pos, b.growth.pos);
        assert_ne!(a.growth.pos, c.growth.pos);
    }

    #[test]
    fn a_lush_area_grows_more_than_a_bare_one() {
        let lush = build_decor(&cave(), Theme::Warrens, 5)
            .growth
            .vertex_count();
        let bare = build_decor(&cave(), Theme::Ashen, 5).growth.vertex_count();
        assert!(lush > bare, "{lush} vs {bare}");
    }

    fn flat() -> TileGrid {
        // A floor whose top face is at y = 1, under open air.
        let mut rows: Vec<String> = vec![".".repeat(40); 8];
        rows.push("#".repeat(40));
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        TileGrid::from_ascii(&refs)
    }

    #[test]
    fn growth_stands_on_the_floor() {
        let d = build_decor(&flat(), Theme::Warrens, 9);
        assert!(d.growth.vertex_count() > 0);
        for p in &d.growth.pos {
            assert!(
                p[1] > 1.0 - 0.06 && p[1] < 1.0 + 0.45,
                "blade vertex at y = {}",
                p[1]
            );
            assert!(p[0] > -0.2 && p[0] < 40.2, "inside the room: x = {}", p[0]);
        }
        for p in &d.rubble.pos {
            assert!(
                p[1] > 1.0 - 0.1 && p[1] < 1.0 + 0.3,
                "rubble at y = {}",
                p[1]
            );
        }
    }

    #[test]
    fn nothing_grows_in_a_spike_pit() {
        let g = cave();
        let d = build_decor(&g, Theme::Warrens, 9);
        for p in &d.growth.pos {
            // The pit's floor tiles (x 21..25, y 0) have spikes above them.
            let inside = p[0] > 21.3 && p[0] < 24.7 && p[1] < 1.6;
            assert!(!inside, "growth among the spikes at {:?}", p);
        }
    }

    #[test]
    fn the_lane_in_front_of_the_knight_stays_ankle_high() {
        let d = build_decor(&flat(), Theme::Warrens, 4);
        for p in &d.growth.pos {
            if p[2] > 0.4 {
                assert!(
                    p[1] < 1.0 + 0.16 + 0.03,
                    "tall growth in front: y={} z={}",
                    p[1],
                    p[2]
                );
            }
        }
    }

    #[test]
    fn stalactites_hang_down_from_the_ceiling() {
        let g = cave();
        let d = build_decor(&g, Theme::Ashen, 6);
        assert!(d.hangers.vertex_count() > 0);
        let (lo, hi) = d.hangers.bounds();
        // The ceiling tile row is y = 10; things hang from its underside (y = 10).
        assert!(
            hi.y <= 10.0 + 1e-3,
            "attached at or below the ceiling: {hi:?}"
        );
        assert!(lo.y < 9.0, "and reaching down: {lo:?}");
    }
}
