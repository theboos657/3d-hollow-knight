//! Tile grid and swept-AABB movement.
//!
//! One tile = 1 world unit. Tile `(i, j)` covers `[i, i+1) x [j, j+1)`.
//! There is no physics engine: bodies move axis by axis (X then Y) in
//! sub-steps no longer than [`MAX_STEP`], and are pushed out to a tiny
//! [`SKIN`] gap from whatever they hit. That keeps movement deterministic and
//! tunneling-free at every speed the game uses (max ~24 u/s = 0.2 u/tick).

use bevy_ecs::prelude::*;
use bevy_math::Vec2;

/// Gap left between a body and the surface it rests against.
pub const SKIN: f32 = 1e-3;
/// Longest single displacement step (well under one tile).
pub const MAX_STEP: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tile {
    #[default]
    Empty,
    Solid,
    /// Blocks only downward movement from above; can be dropped through.
    OneWay,
    /// Not solid. Damages on overlap; pogo-able.
    Spike,
}

impl Tile {
    pub fn from_char(c: char) -> Tile {
        match c {
            '#' => Tile::Solid,
            '=' => Tile::OneWay,
            '^' => Tile::Spike,
            _ => Tile::Empty,
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct TileGrid {
    width: i32,
    height: i32,
    tiles: Vec<Tile>,
}

impl Default for TileGrid {
    fn default() -> Self {
        Self::new(0, 0)
    }
}

impl TileGrid {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            tiles: vec![Tile::Empty; (width * height) as usize],
        }
    }

    /// Builds a grid from ASCII rows listed **top to bottom** (row 0 of the
    /// slice is the highest row). Unknown characters are empty.
    pub fn from_ascii(rows: &[&str]) -> Self {
        let height = rows.len() as i32;
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        let mut g = Self::new(width, height);
        for (row, line) in rows.iter().enumerate() {
            let j = height - 1 - row as i32;
            for (i, c) in line.chars().enumerate() {
                g.set(i as i32, j, Tile::from_char(c));
            }
        }
        g
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn get(&self, i: i32, j: i32) -> Tile {
        if i < 0 || j < 0 || i >= self.width || j >= self.height {
            Tile::Empty
        } else {
            self.tiles[(j * self.width + i) as usize]
        }
    }

    pub fn set(&mut self, i: i32, j: i32, t: Tile) {
        if i >= 0 && j >= 0 && i < self.width && j < self.height {
            self.tiles[(j * self.width + i) as usize] = t;
        }
    }

    /// Tile index range overlapped by an interval, treating exact edges as
    /// non-overlapping on the far side.
    fn span(min: f32, max: f32) -> (i32, i32) {
        (min.floor() as i32, max.ceil() as i32 - 1)
    }

    /// Any tile of kind `kind` overlapped by the box.
    pub fn overlaps(&self, center: Vec2, half: Vec2, kind: Tile) -> bool {
        let (i0, i1) = Self::span(center.x - half.x, center.x + half.x);
        let (j0, j1) = Self::span(center.y - half.y, center.y + half.y);
        (j0..=j1).any(|j| (i0..=i1).any(|i| self.get(i, j) == kind))
    }
}

/// Result of [`move_body`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveOutcome {
    pub pos: Vec2,
    /// Direction of the wall that stopped horizontal motion (+1 right, -1
    /// left, 0 none).
    pub blocked_x: i8,
    /// Landed on ground/one-way this move (moving down and stopped).
    pub landed: bool,
    /// Hit a ceiling this move (moving up and stopped).
    pub bonked: bool,
}

/// Moves a box by `delta`, resolving against `grid`. `drop_through` makes
/// one-way platforms passable this move.
pub fn move_body(
    grid: &TileGrid,
    mut pos: Vec2,
    half: Vec2,
    delta: Vec2,
    drop_through: bool,
) -> MoveOutcome {
    let steps = ((delta.abs().max_element() / MAX_STEP).ceil() as i32).max(1);
    let step = delta / steps as f32;
    let mut out = MoveOutcome {
        pos,
        blocked_x: 0,
        landed: false,
        bonked: false,
    };

    for _ in 0..steps {
        // ---- X ----
        if step.x != 0.0 {
            let tx = pos.x + step.x;
            let (j0, j1) = TileGrid::span(pos.y - half.y, pos.y + half.y);
            let (i0, i1) = TileGrid::span(tx - half.x, tx + half.x);
            let mut hit: Option<i32> = None;
            for i in i0..=i1 {
                if (j0..=j1).any(|j| grid.get(i, j) == Tile::Solid) {
                    hit = Some(match (hit, step.x > 0.0) {
                        (None, _) => i,
                        (Some(h), true) => h.min(i),
                        (Some(h), false) => h.max(i),
                    });
                }
            }
            match hit {
                Some(i) if step.x > 0.0 => {
                    pos.x = i as f32 - half.x - SKIN;
                    out.blocked_x = 1;
                }
                Some(i) => {
                    pos.x = (i + 1) as f32 + half.x + SKIN;
                    out.blocked_x = -1;
                }
                None => pos.x = tx,
            }
        }

        // ---- Y ----
        if step.y != 0.0 {
            let prev_bottom = pos.y - half.y;
            let ty = pos.y + step.y;
            let (i0, i1) = TileGrid::span(pos.x - half.x, pos.x + half.x);
            let (j0, j1) = TileGrid::span(ty - half.y, ty + half.y);
            let mut hit: Option<i32> = None;
            for j in j0..=j1 {
                let blocks = (i0..=i1).any(|i| match grid.get(i, j) {
                    Tile::Solid => true,
                    Tile::OneWay => {
                        step.y < 0.0
                            && !drop_through
                            && prev_bottom >= (j + 1) as f32 - 2.0 * SKIN
                    }
                    _ => false,
                });
                if blocks {
                    hit = Some(match (hit, step.y > 0.0) {
                        (None, _) => j,
                        (Some(h), true) => h.min(j),
                        (Some(h), false) => h.max(j),
                    });
                }
            }
            match hit {
                Some(j) if step.y > 0.0 => {
                    pos.y = j as f32 - half.y - SKIN;
                    out.bonked = true;
                }
                Some(j) => {
                    pos.y = (j + 1) as f32 + half.y + SKIN;
                    out.landed = true;
                }
                None => pos.y = ty,
            }
        }
    }
    out.pos = pos;
    out
}

/// Probe directions for [`probe`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
    Left,
    Right,
}

/// Is something solid touching the given side of the box? (One-ways count
/// only from above and only when the box rests on their top surface.)
pub fn probe(grid: &TileGrid, pos: Vec2, half: Vec2, side: Side, drop_through: bool) -> bool {
    let d = 3.0 * SKIN;
    let shifted = match side {
        Side::Below => pos - Vec2::new(0.0, d),
        Side::Above => pos + Vec2::new(0.0, d),
        Side::Left => pos - Vec2::new(d, 0.0),
        Side::Right => pos + Vec2::new(d, 0.0),
    };
    let (i0, i1) = TileGrid::span(shifted.x - half.x, shifted.x + half.x);
    let (j0, j1) = TileGrid::span(shifted.y - half.y, shifted.y + half.y);
    let bottom = pos.y - half.y;
    for j in j0..=j1 {
        for i in i0..=i1 {
            match grid.get(i, j) {
                Tile::Solid => return true,
                Tile::OneWay
                    if side == Side::Below
                        && !drop_through
                        && bottom >= (j + 1) as f32 - 2.0 * SKIN =>
                {
                    return true
                }
                _ => {}
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor_grid() -> TileGrid {
        // 12 wide, floor two rows thick, wall on the right, a one-way ledge.
        TileGrid::from_ascii(&[
            ".........#..",
            ".........#..",
            ".........#..",
            "...===...#..",
            ".........#..",
            ".........#..",
            "############",
            "############",
        ])
    }

    const HALF: Vec2 = Vec2::new(0.4, 0.75);

    #[test]
    fn from_ascii_is_top_to_bottom() {
        let g = floor_grid();
        assert_eq!(g.get(0, 0), Tile::Solid);
        assert_eq!(g.get(0, 1), Tile::Solid);
        assert_eq!(g.get(0, 2), Tile::Empty);
        assert_eq!(g.get(3, 4), Tile::OneWay);
    }

    #[test]
    fn lands_on_floor_and_probes_ground() {
        let g = floor_grid();
        let out = move_body(&g, Vec2::new(1.5, 5.0), HALF, Vec2::new(0.0, -10.0), false);
        assert!(out.landed);
        assert!((out.pos.y - (2.0 + HALF.y + SKIN)).abs() < 1e-5);
        assert!(probe(&g, out.pos, HALF, Side::Below, false));
        assert!(!probe(&g, out.pos, HALF, Side::Above, false));
    }

    #[test]
    fn wall_stops_horizontal_motion() {
        let g = floor_grid();
        let start = Vec2::new(7.0, 2.0 + HALF.y + SKIN);
        let out = move_body(&g, start, HALF, Vec2::new(5.0, 0.0), false);
        assert_eq!(out.blocked_x, 1);
        assert!((out.pos.x - (9.0 - HALF.x - SKIN)).abs() < 1e-5);
        assert!(probe(&g, out.pos, HALF, Side::Right, false));
    }

    #[test]
    fn one_way_passes_up_lands_down_and_drops() {
        let g = floor_grid();
        let x = 4.5;
        // Rising through the ledge from below is allowed.
        let up = move_body(&g, Vec2::new(x, 3.0), HALF, Vec2::new(0.0, 2.0), false);
        assert!(!up.bonked, "one-way must not block upward motion");
        // Falling onto it lands on top (ledge top is y = 5).
        let down = move_body(&g, Vec2::new(x, 7.0), HALF, Vec2::new(0.0, -3.0), false);
        assert!(down.landed);
        assert!((down.pos.y - (5.0 + HALF.y + SKIN)).abs() < 1e-5);
        // Dropping through ignores it.
        let drop = move_body(&g, down.pos, HALF, Vec2::new(0.0, -1.0), true);
        assert!(!drop.landed);
        assert!(drop.pos.y < down.pos.y);
    }

    #[test]
    fn high_speed_does_not_tunnel() {
        let g = floor_grid();
        // 30 tiles in one call: sub-stepping must still stop at the floor.
        let out = move_body(&g, Vec2::new(1.5, 5.0), HALF, Vec2::new(0.0, -30.0), false);
        assert!(out.landed);
        assert!(out.pos.y > 2.0);
    }

    #[test]
    fn exact_edge_contact_is_not_overlap() {
        let g = floor_grid();
        // Box bottom exactly on the floor top (y = 2.0): not overlapping row 1.
        assert!(!g.overlaps(Vec2::new(1.5, 2.0 + HALF.y), HALF, Tile::Solid));
    }
}
