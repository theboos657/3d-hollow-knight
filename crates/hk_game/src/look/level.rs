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

use super::pbr::{fbm, noise2};
use crate::rig::meshkit::{extrude, hash3, MeshData};

/// Front face of the level blocks and how far back they go.
pub const FRONT: f32 = 1.2;
pub const BACK: f32 = -1.2;
/// Chamfer of each block.
const BEVEL: f32 = 0.055;
/// Texture repeats per world unit on stone and planks: one texture spans eight
/// tiles, so a block shows an eighth of it; blocks at the open edge look
/// through their own random window, deep rock continues the same grain.
pub const STONE_UV: f32 = 0.125;
/// The chamber wall's texture spans this many world units.
pub const WALL_UV: f32 = 1.0 / 8.0;
/// The bevel between blocks deep in the rock, where there is no open edge to
/// chisel: a seam, not a groove.
const SEAM: f32 = 0.006;
const _: () = assert!(
    SEAM < BEVEL / 4.0,
    "seams are hairlines next to real grooves"
);
/// How far the whole rock face swells and sinks, in world units.
const SWELL: f32 = 0.09;
/// Segments along each side of a block's face.
const FACE_SEGS: usize = 4;
/// How far the wall's masonry bulges and sinks, in world units.
pub const WALL_RELIEF: f32 = 0.55;
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

/// The shape of a block's inset face: its four corners stand a little proud
/// or sunk (so each face is a slightly tilted slab, as if laid by hand) and
/// the middle is dished and bumped by noise. The border follows the corners in
/// a straight line, so it always meets the chamfer without a crack.
#[derive(Clone, Copy)]
struct FaceShape {
    /// Bottom-left, bottom-right, top-right, top-left.
    corner_dz: [f32; 4],
    /// Amplitude of the noise in the middle.
    relief: f32,
    noise: u32,
    /// Slow swell of the whole rock face, a function of world position, so a
    /// mass of blocks rolls like one surface instead of a tiled floor.
    swell: f32,
    swell_seed: u32,
}

/// Wavelength of the slowest swell, in tiles.
const SWELL_SPAN: f32 = 64.0;

impl FaceShape {
    const FLAT: FaceShape = FaceShape {
        corner_dz: [0.0; 4],
        relief: 0.0,
        noise: 0,
        swell: 0.0,
        swell_seed: 0,
    };

    /// The swell at a world position.
    fn swell_at(&self, x: f32, y: f32) -> f32 {
        if self.swell == 0.0 {
            return 0.0;
        }
        let f = fbm(
            self.swell_seed,
            (x / SWELL_SPAN).rem_euclid(1.0),
            (y / SWELL_SPAN).rem_euclid(1.0),
            10,
            3,
            0.5,
        );
        (f - 0.5) * 2.0 * self.swell
    }

    /// Height of the face at `(u, w)` in `[0, 1]` across the inset region
    /// whose lower-left corner is `org` and size `size`.
    fn dz(&self, u: f32, w: f32, org: Vec2, size: Vec2) -> f32 {
        let [c0, c1, c2, c3] = self.corner_dz;
        let mut z = lerp(lerp(c0, c1, u), lerp(c3, c2, u), w)
            + self.swell_at(org.x + u * size.x, org.y + w * size.y);
        if self.relief != 0.0 {
            // Zero on the border, so only the middle bulges.
            let fade = (std::f32::consts::PI * u).sin() * (std::f32::consts::PI * w).sin();
            z += self.relief * (noise2(self.noise, u, w, 3, 3) - 0.5) * 2.0 * fade.max(0.0);
        }
        z
    }

    /// The surface normal at `(u, w)`.
    fn normal(&self, u: f32, w: f32, org: Vec2, size: Vec2) -> Vec3 {
        let e = 0.02;
        let du = (self.dz(u + e, w, org, size) - self.dz(u - e, w, org, size)) / (2.0 * e * size.x);
        let dw = (self.dz(u, w + e, org, size) - self.dz(u, w - e, org, size)) / (2.0 * e * size.y);
        Vec3::new(-du, -dw, 1.0).normalize()
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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
    window: (f32, f32),
    shape: FaceShape,
) {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    // The flat walls end at the same depth on every block (so neighbours meet
    // without a slit); only the inset face stands more or less proud.
    let f = edge_z;
    let (ox, oy) = window;
    // Texture coordinates are physical: the same world size on every face, so
    // the grain of a wall, a ceiling and a ledge all have one scale.
    let s = STONE_UV;
    // ...and continuous across blocks unless a window offset says otherwise.
    let uv_xy = |x: f32, y: f32| Vec2::new(ox + x * s, oy + y * s);
    let uv_xz = |x: f32, z: f32| Vec2::new(ox + x * s, oy + z * s);
    let uv_zy = |z: f32, y: f32| Vec2::new(ox + z * s, oy + y * s);
    let mid = |k: usize, l: usize| col((b[k] + b[l]) * 0.5, tint);
    // Face brightness is slightly lifted on the lit sides.
    let lit = |k: usize, gain: f32| col(b[k] * gain, tint);

    // The inset front: a grid whose middle can be dished, each vertex with its
    // own normal, so the light rolls across it.
    let (a0, a1, c0, c1) = (x0 + bevel, x1 - bevel, y0 + bevel, y1 - bevel);
    let size = Vec2::new(a1 - a0, c1 - c0);
    let org = Vec2::new(a0, c0);
    let n = FACE_SEGS;
    let mut ids = Vec::with_capacity((n + 1) * (n + 1));
    for gj in 0..=n {
        for gi in 0..=n {
            let (u, w) = (gi as f32 / n as f32, gj as f32 / n as f32);
            let (x, y) = (lerp(a0, a1, u), lerp(c0, c1, w));
            let bright = lerp(lerp(b[0], b[1], u), lerp(b[3], b[2], u), w);
            ids.push(m.push(
                v(x, y, front + shape.dz(u, w, org, size)),
                shape.normal(u, w, org, size),
                uv_xy(x, y),
                col(bright, tint),
            ));
        }
    }
    for gj in 0..n {
        for gi in 0..n {
            let k = |i: usize, j: usize| ids[j * (n + 1) + i];
            let (p, q, r, t) = (k(gi, gj), k(gi + 1, gj), k(gi + 1, gj + 1), k(gi, gj + 1));
            m.tri(p, q, r);
            m.tri(p, r, t);
        }
    }
    // The chamfer meets the face's corners, wherever those stand.
    let corner =
        |k: usize, x: f32, y: f32| v(x, y, front + shape.corner_dz[k] + shape.swell_at(x, y));
    let (i0, i1, i2, i3) = (
        corner(0, a0, c0),
        corner(1, a1, c0),
        corner(2, a1, c1),
        corner(3, a0, c1),
    );
    // Chamfers: each faces out and forward, so the light catches the top and
    // left and the groove between blocks stays dark.
    let sq = std::f32::consts::FRAC_1_SQRT_2;
    m.add_quad(
        [v(x0, y0, f), v(x1, y0, f), i1, i0],
        Vec3::new(0.0, -sq, sq),
        [uv_xy(x0, y0), uv_xy(x1, y0), uv_xy(a1, c0), uv_xy(a0, c0)],
        [lit(0, 0.55), lit(1, 0.55), lit(1, 0.55), lit(0, 0.55)],
    );
    m.add_quad(
        [v(x1, y0, f), v(x1, y1, f), i2, i1],
        Vec3::new(sq, 0.0, sq),
        [uv_xy(x1, y0), uv_xy(x1, y1), uv_xy(a1, c1), uv_xy(a1, c0)],
        [lit(1, 0.65), lit(2, 0.65), lit(2, 0.65), lit(1, 0.65)],
    );
    m.add_quad(
        [v(x1, y1, f), v(x0, y1, f), i3, i2],
        Vec3::new(0.0, sq, sq),
        [uv_xy(x1, y1), uv_xy(x0, y1), uv_xy(a0, c1), uv_xy(a1, c1)],
        [lit(2, 1.5), lit(3, 1.5), lit(3, 1.5), lit(2, 1.5)],
    );
    m.add_quad(
        [v(x0, y1, f), v(x0, y0, f), i0, i3],
        Vec3::new(-sq, 0.0, sq),
        [uv_xy(x0, y1), uv_xy(x0, y0), uv_xy(a0, c0), uv_xy(a0, c1)],
        [lit(3, 1.2), lit(0, 1.2), lit(0, 1.2), lit(3, 1.2)],
    );
    // Exposed sides (only where there is air next to the block).
    if faces.top {
        m.add_quad(
            [v(x0, y1, f), v(x1, y1, f), v(x1, y1, z0), v(x0, y1, z0)],
            Vec3::Y,
            [uv_xz(x0, f), uv_xz(x1, f), uv_xz(x1, z0), uv_xz(x0, z0)],
            [lit(3, 1.3), lit(2, 1.3), lit(2, 1.3), lit(3, 1.3)],
        );
    }
    if faces.bottom {
        m.add_quad(
            [v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, f), v(x0, y0, f)],
            Vec3::NEG_Y,
            [uv_xz(x0, z0), uv_xz(x1, z0), uv_xz(x1, f), uv_xz(x0, f)],
            [mid(0, 1), mid(0, 1), mid(0, 1), mid(0, 1)],
        );
    }
    if faces.left {
        m.add_quad(
            [v(x0, y0, z0), v(x0, y0, f), v(x0, y1, f), v(x0, y1, z0)],
            Vec3::NEG_X,
            [uv_zy(z0, y0), uv_zy(f, y0), uv_zy(f, y1), uv_zy(z0, y1)],
            [lit(0, 1.0), lit(0, 1.0), lit(3, 1.0), lit(3, 1.0)],
        );
    }
    if faces.right {
        m.add_quad(
            [v(x1, y0, f), v(x1, y0, z0), v(x1, y1, z0), v(x1, y1, f)],
            Vec3::X,
            [uv_zy(f, y0), uv_zy(z0, y0), uv_zy(z0, y1), uv_zy(f, y1)],
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
        // World-continuous, so a run of caps reads as one strip.
        .box_mapped(STONE_UV * 2.0, Vec2::ZERO)
}

/// A one-way platform tile: a chamfered plank on a bracket.
fn plank(m: &mut MeshData, i: i32, j: i32, seed: u32, grid: &TileGrid) {
    let x0 = i as f32;
    let (y0, y1) = (j as f32 + 0.72, j as f32 + 1.0);
    let window = (hash3(seed, i, j, 3), hash3(seed, i, j, 4));
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
        window,
        FaceShape::FLAT,
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
        .recolor(|_| [0.55, 0.55, 0.55, 1.0])
        .box_mapped(STONE_UV, Vec2::new(0.3, 0.6));
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
                    // Rock at the edge of the open is chiselled: its own tint,
                    // tilt and grain window, and a real groove between blocks.
                    // Deep rock is one surface: a faint seam, world-continuous
                    // grain and a slow swell, so a mass reads as a cliff, not a
                    // floor of tiles.
                    let d = dist[(j * grid.width() + i) as usize];
                    let near = d < 2.5;
                    let (bevel, tint, window, shape) = if near {
                        let tint = 0.88 + 0.24 * hash3(seed, i, j, 1);
                        let window = (hash3(seed, i, j, 3), hash3(seed, i, j, 4));
                        let mut corner_dz =
                            [0, 1, 2, 3].map(|k| (hash3(seed, i, j, 10 + k) - 0.6) * 0.10);
                        // Now and then a corner has been knocked off.
                        let chip = hash3(seed, i, j, 20);
                        if chip > 0.9 {
                            corner_dz[(chip * 1000.0) as usize % 4] -= 0.07;
                        }
                        let shape = FaceShape {
                            corner_dz,
                            relief: 0.045,
                            noise: seed ^ ((i as u32) << 8) ^ (j as u32),
                            swell: SWELL,
                            swell_seed: seed ^ 0x9E37,
                        };
                        (BEVEL, tint, window, shape)
                    } else {
                        let mottle = fbm(
                            seed ^ 0x77,
                            (i as f32 / SWELL_SPAN).rem_euclid(1.0),
                            (j as f32 / SWELL_SPAN).rem_euclid(1.0),
                            24,
                            2,
                            0.5,
                        );
                        let shape = FaceShape {
                            swell: SWELL,
                            swell_seed: seed ^ 0x9E37,
                            ..FaceShape::FLAT
                        };
                        (SEAM, 0.86 + 0.28 * mottle, (0.0, 0.0), shape)
                    };
                    add_block(
                        &mut meshes[(i / CHUNK) as usize],
                        (i as f32, i as f32 + 1.0, j as f32, j as f32 + 1.0),
                        BACK,
                        (FRONT - BEVEL - 0.03, FRONT),
                        bevel,
                        b,
                        tint,
                        faces,
                        window,
                        shape,
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
    use bevy::mesh::Mesh;

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
    fn a_big_room_stays_within_a_vertex_budget_and_lights_with_finite_tangents() {
        use bevy::mesh::VertexAttributeValues;
        // A 64 x 36 room: solid rock with a wide hall cut through it.
        let mut rows = vec![String::new(); 36];
        for (y, row) in rows.iter_mut().enumerate() {
            *row = if (10..22).contains(&y) {
                format!("#{}#", ".".repeat(62))
            } else {
                "#".repeat(64)
            };
        }
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let g = TileGrid::from_ascii(&rows);
        let geo = build_level(&g, 5);
        let total: usize = geo.chunks.iter().map(|(_, m)| m.vertex_count()).sum();
        assert!(total < 160_000, "{total} stone vertices");
        for (_, m) in &geo.chunks {
            let mesh = m.to_mesh_pbr();
            let Some(VertexAttributeValues::Float32x4(t)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT)
            else {
                panic!("no tangents");
            };
            assert!(t.iter().all(|v| v.iter().all(|c| c.is_finite())));
        }
    }

    #[test]
    fn deep_rock_is_one_surface_and_the_open_edge_is_chiselled() {
        // In a solid mass, a tile far from any air gets a hairline seam; one on
        // the edge of the open gets the full groove.
        let g = TileGrid::from_ascii(&[
            "###########",
            "###########",
            "###########",
            "###########",
            "###########",
            "###########",
            "###########",
            "#.........#",
            "###########",
        ]);
        let d = air_distance(&g);
        let at = |i: i32, j: i32| d[(j * g.width() + i) as usize];
        // Rows read top to bottom; the hall is the second row from the bottom.
        assert!(at(5, 2) < 2.5, "the tile above the hall is at its edge");
        assert!(at(5, 7) >= 2.5, "the tile near the top is deep in the rock");
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

/// The chamber wall behind the play lane: a fine grid of vertices at `z`,
/// pushed and pulled by slow noise so the masonry bulges and sinks like old,
/// settled stone, with a blotchy brightness and a falloff toward the top so the
/// far reaches of tall rooms sink into fog-coloured dark. Texture coordinates
/// are physical ([`WALL_UV`]).
pub fn wall_mesh(width: f32, height: f32, z: f32, seed: u32) -> MeshData {
    let cell = 1.0;
    let (x0, y0) = (-14.0f32, -8.0f32);
    let nx = ((width + 28.0) / cell).ceil() as usize;
    let ny = ((height + 20.0) / cell).ceil() as usize;
    let (sx, sy) = (nx as f32 * cell, ny as f32 * cell);
    // Everything is a function of position, so the grid needs no seams.
    // Only ever *back* from `z`, so nothing set against the wall (windows,
    // pillars) is swallowed by a bulge.
    let relief = |x: f32, y: f32| {
        let (u, w) = ((x - x0) / sx, (y - y0) / sy);
        (fbm(seed, u, w, 6, 4, 0.5) - 1.0) * 2.0 * WALL_RELIEF
    };
    let shade_at = |x: f32, y: f32| {
        let (u, w) = ((x - x0) / sx, (y - y0) / sy);
        let blotch = fbm(seed ^ 0x51, u, w, 9, 3, 0.55);
        let fall = (1.0 - ((y - 2.0) / (height + 6.0)).clamp(0.0, 1.0) * 0.45).max(0.4);
        (0.32 + 0.5 * blotch) * fall
    };
    let mut m = MeshData::default();
    let e = 0.05;
    for iy in 0..=ny {
        for ix in 0..=nx {
            let (x, y) = (x0 + ix as f32 * cell, y0 + iy as f32 * cell);
            let dz = relief(x, y);
            let n = Vec3::new(
                -(relief(x + e, y) - relief(x - e, y)) / (2.0 * e),
                -(relief(x, y + e) - relief(x, y - e)) / (2.0 * e),
                1.0,
            )
            .normalize();
            let sh = shade_at(x, y);
            m.push(
                Vec3::new(x, y, z + dz),
                n,
                Vec2::new(x, y) * WALL_UV,
                [sh, sh, sh, 1.0],
            );
        }
    }
    let at = |ix: usize, iy: usize| (iy * (nx + 1) + ix) as u32;
    for iy in 0..ny {
        for ix in 0..nx {
            let (a, b, c, d) = (
                at(ix, iy),
                at(ix + 1, iy),
                at(ix + 1, iy + 1),
                at(ix, iy + 1),
            );
            m.tri(a, b, c);
            m.tri(a, c, d);
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
        assert!(
            m.pos
                .iter()
                .all(|p| p[2] <= -3.4 + 1e-4 && p[2] >= -3.4 - 2.0 * WALL_RELIEF - 1e-4),
            "only ever recedes from its plane, by at most twice the relief"
        );
        let (zlo, zhi) = (lo.z, hi.z);
        assert!(zhi - zlo > 0.2, "and is not flat: {zlo}..{zhi}");
        assert!(
            m.col.iter().all(|c| c[0] > 0.2 && c[0] < 1.0),
            "never black or blown"
        );
    }
}
