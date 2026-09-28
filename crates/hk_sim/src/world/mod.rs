//! World geometry: tile grids, swept collision, rooms.

pub mod grid;

pub use grid::{move_body, probe, MoveOutcome, Side, Tile, TileGrid, SKIN};
