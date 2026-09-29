//! The level's stone, as geometry.
//!
//! Every solid tile becomes a chamfered block, so the wall reads as laid
//! stone (a bright bevel on top and left, a dark groove between blocks) instead
//! of one flat slab; blocks near open air are lit and blocks deep inside the
//! rock fall into shadow, which gives every ledge a rim and every cave depth.
//! Exposed floors get a lighter lip on top and one-way platforms are planks on
//! corbels. All of it is pure maths returning [`MeshData`], so the shape rules
//! are unit-tested.

use bevy::math::{Vec2, Vec3};
use hk_sim::world::grid::{Tile, TileGrid};

use crate::rig::meshkit::{extrude, hash3, MeshData};

/// Front face of the level blocks and how far back they go.
pub const FRONT: f32 = 1.2;
pub const BACK: f32 = -1.2;
/// Chamfer of each block.
const BEVEL: f32 = 0.055;
/// Tiles per mesh chunk along x (so the far parts of a long room can be culled).
pub const CHUNK: i32 = 16;

/// The level's meshes, ready to spawn.
#[derive(Default)]
pub struct LevelGeometry {
    /// Stone blocks, one mesh per chunk of `CHUNK` columns: `(first column, mesh)`.
    pub chunks: Vec<(i32, MeshData)>,
    /// The lit lip on exposed floors.
    pub caps: MeshData,
    /// One-way platforms and their brackets.
    pub planks: MeshData,
}

fn solid(grid: &TileGrid, i: i32, j: i32) -> bool {
    grid.get(i, j) == Tile::Solid
}

/// For every tile, how far (in tiles, capped) it is from open air; open tiles
/// are 0. Tiles outside the map count as rock, so a room's own walls do not
/// glow just because the map ends there.
pub fn air_distance(grid: &TileGrid) -> Vec<f32> {
    const R: i32 = 6;
    let (w, h) = (grid.width(), grid.height());
    let mut out = vec![0.0; (w * h).max(0) as usize];
    for j in 0..h {
        for i in 0..w {
            if !solid(grid, i, j) {
                continue;
            }
            let mut best = R as f32;
            'search: for r in 1..=R {
                for dj in -r..=r {
                    for di in -r..=r {
                        if di.abs().max(dj.abs()) != r {
                            continue;
                        }
                        let (x, y) = (i + di, j + dj);
                        let inside = x >= 0 && y >= 0 && x < w && y < h;
                        if inside && !solid(grid, x, y) {
                            best = r as f32;
                            break 'search;
                        }
                    }
                }
            }
            out[(j * w + i) as usize] = best;
        }
    }
    out
}

/// How bright a block is, from its distance to open air.
pub fn shade(d: f32) -> f32 {
    0.30 + 0.70 * (-0.62 * d).exp()
}

fn brightness_at(grid: &TileGrid, dist: &[f32], x: i32, y: i32) -> f32 {
    // The corner at (x, y) touches tiles (x-1..x, y-1..y): average their shade.
    let mut sum = 0.0;
    for (di, dj) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
        let (i, j) = (x + di, y + dj);
        let d = if i >= 0 && j >= 0 && i < grid.width() && j < grid.height() {
            dist[(j * grid.width() + i) as usize]
        } else {
            6.0
        };
        sum += shade(d);
    }
    sum * 0.25
}

fn col(b: f32, tint: f32) -> [f32; 4] {
    let v = (b * tint).min(1.4);
    [v, v, v, 1.0]
}

/// Which faces of a block are visible from outside the rock.
#[derive(Clone, Copy)]
struct Faces {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

/// One chamfered block spanning `[x0, x1] x [y0, y1]` from `z0` to `front`,
/// with per-corner brightness `b` (bottom-left, bottom-right, top-right,
/// top-left) and a hash-driven texture window.
#[allow(clippy::too_many_arguments)]
fn add_block(
    m: &mut MeshData,
    (x0, x1, y0, y1): (f32, f32, f32, f32),
    z0: f32,
    (edge_z, front): (f32, f32),
    bevel: f32,
    b: [f32; 4],
    tint: f32,
    faces: Faces,
    uv_seed: (f32, f32),
) {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    // The flat walls end at the same depth on every block (so neighbours meet
    // without a slit); only the inset face stands more or less proud.
    let f = edge_z;
    let (ux, uy) = uv_seed;
    let uv = |u: f32, w: f32| Vec2::new(ux + u * 0.5, uy + w * 0.5);
    let c = |k: usize| col(b[k], tint);
    let mid = |k: usize, l: usize| col((b[k] + b[l]) * 0.5, tint);
    // Face brightness is slightly lifted on the lit sides.
    let lit = |k: usize, gain: f32| col(b[k] * gain, tint);

    // The inset front.
    let (a0, a1, c0, c1) = (x0 + bevel, x1 - bevel, y0 + bevel, y1 - bevel);
    m.add_quad(
        [
            v(a0, c0, front),
            v(a1, c0, front),
            v(a1, c1, front),
            v(a0, c1, front),
        ],
        Vec3::Z,
        [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
        [c(0), c(1), c(2), c(3)],
    );
    // Chamfers: each faces out and forward, so the light catches the top and
    // left and the groove between blocks stays dark.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    m.add_quad(
        [
            v(x0, y0, f),
            v(x1, y0, f),
            v(a1, c0, front),
            v(a0, c0, front),
        ],
        Vec3::new(0.0, -s, s),
        [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 0.0), uv(0.0, 0.0)],
        [lit(0, 0.55), lit(1, 0.55), lit(1, 0.55), lit(0, 0.55)],
    );
    m.add_quad(
        [
            v(x1, y0, f),
            v(x1, y1, f),
            v(a1, c1, front),
            v(a1, c0, front),
        ],
        Vec3::new(s, 0.0, s),
        [uv(1.0, 0.0), uv(1.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0)],
        [lit(1, 0.65), lit(2, 0.65), lit(2, 0.65), lit(1, 0.65)],
    );
    m.add_quad(
        [
            v(x1, y1, f),
            v(x0, y1, f),
            v(a0, c1, front),
            v(a1, c1, front),
        ],
        Vec3::new(0.0, s, s),
        [uv(1.0, 1.0), uv(0.0, 1.0), uv(0.0, 1.0), uv(1.0, 1.0)],
        [lit(2, 1.5), lit(3, 1.5), lit(3, 1.5), lit(2, 1.5)],
    );
    m.add_quad(
        [
            v(x0, y1, f),
            v(x0, y0, f),
            v(a0, c0, front),
            v(a0, c1, front),
        ],
        Vec3::new(-s, 0.0, s),
        [uv(0.0, 1.0), uv(0.0, 0.0), uv(0.0, 0.0), uv(0.0, 1.0)],
        [lit(3, 1.2), lit(0, 1.2), lit(0, 1.2), lit(3, 1.2)],
    );
    // Exposed sides (only where there is air next to the block).
    if faces.top {
        m.add_quad(
            [v(x0, y1, f), v(x1, y1, f), v(x1, y1, z0), v(x0, y1, z0)],
            Vec3::Y,
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            [lit(3, 1.3), lit(2, 1.3), lit(2, 1.3), lit(3, 1.3)],
        );
    }
    if faces.bottom {
        m.add_quad(
            [v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, f), v(x0, y0, f)],
            Vec3::NEG_Y,
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            [mid(0, 1), mid(0, 1), mid(0, 1), mid(0, 1)],
        );
    }
    if faces.left {
        m.add_quad(
            [v(x0, y0, z0), v(x0, y0, f), v(x0, y1, f), v(x0, y1, z0)],
            Vec3::NEG_X,
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            [lit(0, 1.0), lit(0, 1.0), lit(3, 1.0), lit(3, 1.0)],
        );
    }
    if faces.right {
        m.add_quad(
            [v(x1, y0, f), v(x1, y0, z0), v(x1, y1, z0), v(x1, y1, f)],
            Vec3::X,
            [uv(0.0, 0.0), uv(1.0, 0.0), uv(1.0, 1.0), uv(0.0, 1.0)],
            [lit(1, 0.9), lit(1, 0.9), lit(2, 0.9), lit(2, 0.9)],
        );
    }
}

/// The lip on top of an exposed floor tile: a rounded, slightly overhanging
/// slab.
fn cap_piece(i: i32, j: i32, seed: u32) -> MeshData {
    let profile = [
        Vec2::new(-1.32, 0.0),
        Vec2::new(1.32, 0.0),
        Vec2::new(1.32, 0.08),
        Vec2::new(1.24, 0.15),
        Vec2::new(0.9, 0.19),
        Vec2::new(-0.9, 0.19),
        Vec2::new(-1.24, 0.15),
        Vec2::new(-1.32, 0.08),
    ];
    let h = hash3(seed, i, j, 7);
    let tint = 0.86 + 0.28 * h;
    // The profile's x becomes depth (z); the extrusion runs along x.
    extrude(&profile, 1.0)
        .transformed(bevy::math::Mat4::from_rotation_y(
            -std::f32::consts::FRAC_PI_2,
        ))
        .transformed(bevy::math::Mat4::from_translation(Vec3::new(
            i as f32 + 0.5,
            j as f32 + 1.0 - 0.01,
            0.0,
        )))
        .recolor(move |p| {
            // Brighter along the top edge, with slight noise along the run.
            let k = if p.y > j as f32 + 1.1 { 1.0 } else { 0.72 };
            let v = k * tint;
            [v, v, v, 1.0]
        })
}

/// A one-way platform tile: a chamfered plank on a bracket.
fn plank(m: &mut MeshData, i: i32, j: i32, seed: u32, grid: &TileGrid) {
    let x0 = i as f32;
    let (y0, y1) = (j as f32 + 0.72, j as f32 + 1.0);
    let uv = (hash3(seed, i, j, 3), hash3(seed, i, j, 4));
    let tint = 0.9 + 0.2 * hash3(seed, i, j, 5);
    let left_end = grid.get(i - 1, j) != Tile::OneWay;
    let right_end = grid.get(i + 1, j) != Tile::OneWay;
    add_block(
        m,
        (x0, x0 + 1.0, y0, y1),
        -0.9,
        (0.9 - 0.04, 0.9),
        0.04,
        [1.0; 4],
        tint,
        Faces {
            top: true,
            bottom: true,
            left: left_end,
            right: right_end,
        },
        uv,
    );
    // A corbel under the middle of every second plank.
    if (i + j) % 2 == 0 {
        let corbel = extrude(
            &[
                Vec2::new(-0.22, 0.0),
                Vec2::new(0.22, 0.0),
                Vec2::new(0.0, -0.36),
            ],
            0.34,
        )
        .transformed(bevy::math::Mat4::from_translation(Vec3::new(
            x0 + 0.5,
            y0 + 0.01,
            0.0,
        )))
        .recolor(|_| [0.55, 0.55, 0.55, 1.0]);
        m.merge(&corbel);
    }
}

/// Builds the level's geometry for a room.
pub fn build_level(grid: &TileGrid, seed: u32) -> LevelGeometry {
    let dist = air_distance(grid);
    let mut geo = LevelGeometry::default();
    let chunks = (grid.width() + CHUNK - 1) / CHUNK;
    let mut meshes: Vec<MeshData> = (0..chunks.max(0)).map(|_| MeshData::default()).collect();
    for j in 0..grid.height() {
        for i in 0..grid.width() {
            match grid.get(i, j) {
                Tile::Solid => {
                    let faces = Faces {
                        top: !solid(grid, i, j + 1),
                        bottom: !solid(grid, i, j - 1),
                        left: !solid(grid, i - 1, j),
                        right: !solid(grid, i + 1, j),
                    };
                    let b = [
                        brightness_at(grid, &dist, i, j),
                        brightness_at(grid, &dist, i + 1, j),
                        brightness_at(grid, &dist, i + 1, j + 1),
                        brightness_at(grid, &dist, i, j + 1),
                    ];
                    // A little hand-laid irregularity: brightness, texture
                    // window and how far each block's face stands proud.
                    let tint = 0.88 + 0.24 * hash3(seed, i, j, 1);
                    let proud = (hash3(seed, i, j, 2) - 0.5) * 0.05;
                    let uv = (hash3(seed, i, j, 3), hash3(seed, i, j, 4));
                    add_block(
                        &mut meshes[(i / CHUNK) as usize],
                        (i as f32, i as f32 + 1.0, j as f32, j as f32 + 1.0),
                        BACK,
                        (FRONT - BEVEL, FRONT + proud),
                        BEVEL,
                        b,
                        tint,
                        faces,
                        uv,
                    );
                    if faces.top {
                        geo.caps.merge(&cap_piece(i, j, seed));
                    }
                }
                Tile::OneWay => plank(&mut geo.planks, i, j, seed, grid),
                _ => {}
            }
        }
    }
    geo.chunks = meshes
        .into_iter()
        .enumerate()
        .filter(|(_, m)| m.vertex_count() > 0)
        .map(|(k, m)| (k as i32 * CHUNK, m))
        .collect();
    geo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> TileGrid {
        TileGrid::from_ascii(&[
            "##########",
            "#........#",
            "#..==....#",
            "#........#",
            "#...^....#",
            "##########",
        ])
    }

    #[test]
    fn every_generated_mesh_is_well_formed() {
        let geo = build_level(&grid(), 1);
        assert!(!geo.chunks.is_empty());
        for (_, m) in &geo.chunks {
            m.validate().expect("stone chunk");
        }
        geo.caps.validate().expect("caps");
        geo.planks.validate().expect("planks");
    }

    #[test]
    fn only_solid_tiles_become_blocks_and_only_exposed_floors_get_a_lip() {
        let g = grid();
        let geo = build_level(&g, 1);
        // Caps: one per solid tile with air above (the floor row's 8 open
        // columns, the top wall's tiles have no air above: only the inner floor).
        let solid_tiles_with_air_above = (0..g.width())
            .flat_map(|i| (0..g.height()).map(move |j| (i, j)))
            .filter(|&(i, j)| solid(&g, i, j) && !solid(&g, i, j + 1))
            .count();
        let per_cap = cap_piece(0, 0, 1).vertex_count();
        assert_eq!(
            geo.caps.vertex_count(),
            solid_tiles_with_air_above * per_cap
        );
        // No block exists where there is no rock: the chunk's x range is inside the map.
        for (first, m) in &geo.chunks {
            let (lo, hi) = m.bounds();
            assert!(lo.x >= *first as f32 - 0.01 && hi.x <= (first + CHUNK) as f32 + 0.01);
            assert!(lo.y >= -0.01 && hi.y <= g.height() as f32 + 0.01);
        }
    }

    #[test]
    fn rock_near_air_is_lit_and_deep_rock_is_dark() {
        let g = TileGrid::from_ascii(&[
            "###########",
            "###########",
            "###########",
            "###########",
            "###########",
            "###...#####",
            "###########",
        ]);
        let d = air_distance(&g);
        let at = |i: i32, j: i32| d[(j * g.width() + i) as usize];
        // Row 1 (from the bottom) is the open row: 3 tiles above it is deep.
        assert_eq!(at(4, 1), 0.0, "open air");
        assert_eq!(at(4, 2), 1.0, "touching air");
        assert!(at(4, 5) > at(4, 3), "farther from air, deeper");
        assert!(shade(0.0) > shade(1.0) && shade(1.0) > shade(3.0) && shade(3.0) > shade(6.0));
        assert!(shade(6.0) > 0.25, "never pitch black");
        assert!(shade(0.0) <= 1.0 + 1e-6);
    }

    #[test]
    fn the_map_edge_is_not_an_opening() {
        // A room's own outer walls are rock: they must not light up because the
        // grid ends there.
        let g = TileGrid::from_ascii(&["###", "#.#", "###"]);
        let d = air_distance(&g);
        assert_eq!(d[0], 1.0, "the corner touches the open middle diagonally");
        let g2 = TileGrid::from_ascii(&["####", "####", "####"]);
        assert!(
            air_distance(&g2).iter().all(|&v| v >= 6.0),
            "all rock: deep"
        );
    }

    #[test]
    fn one_way_planks_are_thin_and_sit_on_the_top_of_their_tile() {
        let g = TileGrid::from_ascii(&["....", ".==.", "....", "####"]);
        let geo = build_level(&g, 3);
        let (lo, hi) = geo.planks.bounds();
        // Tiles (1,2),(2,2): the plank fills the top quarter of the tile.
        assert!(
            hi.y <= 3.0 + 1e-3 && lo.y >= 2.0 - 0.4,
            "plank {lo:?} {hi:?}"
        );
        assert!(lo.x >= 1.0 - 1e-3 && hi.x <= 3.0 + 1e-3);
    }
}

// -------------------------------------------------------------- back wall --

/// The chamber wall behind the play lane: a coarse grid of quads at `z`, with
/// a slow blotchy brightness and a falloff toward the top so the far reaches of
/// tall rooms sink into fog-coloured dark.
pub fn wall_mesh(width: f32, height: f32, z: f32, seed: u32) -> MeshData {
    let cell = 2.0;
    let (x0, y0) = (-14.0f32, -8.0f32);
    let nx = ((width + 28.0) / cell).ceil() as i32;
    let ny = ((height + 20.0) / cell).ceil() as i32;
    let shade_at = |ix: i32, iy: i32| {
        let n = hash3(seed, ix.div_euclid(2), iy.div_euclid(2), 21);
        let m = hash3(seed, ix, iy, 22);
        let y = y0 + iy as f32 * cell;
        let fall = (1.0 - ((y - 2.0) / (height + 6.0)).clamp(0.0, 1.0) * 0.45).max(0.4);
        (0.42 + 0.22 * n + 0.08 * m) * fall
    };
    let mut m = MeshData::default();
    for iy in 0..ny {
        for ix in 0..nx {
            let (xa, ya) = (x0 + ix as f32 * cell, y0 + iy as f32 * cell);
            let c = |dx: i32, dy: i32| {
                let s = shade_at(ix + dx, iy + dy);
                [s, s, s, 1.0]
            };
            let u = |dx: f32, dy: f32| Vec2::new((ix as f32 + dx) * 0.5, (iy as f32 + dy) * 0.5);
            m.add_quad(
                [
                    Vec3::new(xa, ya, z),
                    Vec3::new(xa + cell, ya, z),
                    Vec3::new(xa + cell, ya + cell, z),
                    Vec3::new(xa, ya + cell, z),
                ],
                Vec3::Z,
                [u(0.0, 0.0), u(1.0, 0.0), u(1.0, 1.0), u(0.0, 1.0)],
                [c(0, 0), c(1, 0), c(1, 1), c(0, 1)],
            );
        }
    }
    m
}

#[cfg(test)]
mod wall_tests {
    use super::*;

    #[test]
    fn the_wall_covers_the_room_and_a_margin_and_is_well_formed() {
        let m = wall_mesh(40.0, 20.0, -3.4, 2);
        m.validate().expect("wall");
        let (lo, hi) = m.bounds();
        assert!(lo.x <= -10.0 && hi.x >= 50.0, "wide enough for the camera");
        assert!(lo.y <= -4.0 && hi.y >= 26.0, "tall enough for the camera");
        assert!(m.pos.iter().all(|p| (p[2] + 3.4).abs() < 1e-5), "one plane");
        assert!(
            m.col.iter().all(|c| c[0] > 0.2 && c[0] < 1.0),
            "never black or blown"
        );
    }
}
