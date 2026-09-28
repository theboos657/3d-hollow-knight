//! Input latch: the sim's only view of the player's controls.
//!
//! Devices are sampled once per *rendered* frame (in `hk_game`), but the sim
//! runs a variable number of fixed ticks per frame (0, 1 or 2+). Bevy's own
//! `just_pressed` is cleared each frame, so a press landing in a frame that
//! runs zero ticks would be lost. Instead, every press edge is stamped with
//! the first tick allowed to see it and stays buffered until a system
//! *consumes* it or it ages out.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::SimTick;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Left,
    Right,
    Up,
    Down,
    Jump,
    Attack,
    Dash,
    Focus,
    Cast,
}

impl Action {
    pub const COUNT: usize = 9;
    pub const ALL: [Action; Action::COUNT] = [
        Action::Left,
        Action::Right,
        Action::Up,
        Action::Down,
        Action::Jump,
        Action::Attack,
        Action::Dash,
        Action::Focus,
        Action::Cast,
    ];

    fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Resource, Clone, Debug, Default)]
pub struct InputState {
    held: [bool; Action::COUNT],
    /// First tick allowed to consume the latest un-consumed press edge.
    press_stamp: [Option<u64>; Action::COUNT],
}

impl InputState {
    /// Records the device state of `action`. `next_tick` is the number of the
    /// next tick that will run, i.e. `SimTick.0 + 1`. A rising edge is stamped
    /// so that tick sees it at age 0.
    pub fn set(&mut self, action: Action, down: bool, next_tick: u64) {
        let i = action.idx();
        if down && !self.held[i] {
            self.press_stamp[i] = Some(next_tick);
        }
        self.held[i] = down;
    }

    pub fn held(&self, action: Action) -> bool {
        self.held[action.idx()]
    }

    /// -1 (left), 0 or +1 (right). Opposing directions cancel.
    pub fn axis_x(&self) -> i8 {
        self.held(Action::Right) as i8 - self.held(Action::Left) as i8
    }

    /// -1 (down), 0 or +1 (up). Opposing directions cancel.
    pub fn axis_y(&self) -> i8 {
        self.held(Action::Up) as i8 - self.held(Action::Down) as i8
    }

    /// True if the action was pressed within the last `window` ticks (inclusive)
    /// as seen from `now` (the tick currently running) and not yet consumed.
    pub fn buffered(&self, action: Action, now: u64, window: u32) -> bool {
        match self.press_stamp[action.idx()] {
            Some(stamp) => now >= stamp && now - stamp <= window as u64,
            None => false,
        }
    }

    /// Like [`buffered`](Self::buffered) but clears the press when it fires.
    pub fn consume(&mut self, action: Action, now: u64, window: u32) -> bool {
        if self.buffered(action, now, window) {
            self.press_stamp[action.idx()] = None;
            true
        } else {
            false
        }
    }

    /// Discards a pending press (e.g. on room change or death).
    pub fn clear_press(&mut self, action: Action) {
        self.press_stamp[action.idx()] = None;
    }

    /// Bitset of held actions, used by replay recording.
    pub fn held_bits(&self) -> u16 {
        Action::ALL
            .iter()
            .fold(0u16, |b, a| b | ((self.held(*a) as u16) << a.idx()))
    }
}

/// Bit of `action` in a held-bitset (see [`InputState::held_bits`]).
pub fn bit(action: Action) -> u16 {
    1 << action.idx()
}

/// Convenience for tests and replays: apply a held-bitset for the *next* tick.
pub fn apply_bits(input: &mut InputState, bits: u16, tick: &SimTick) {
    for a in Action::ALL {
        input.set(a, bits & (1 << a.idx()) != 0, tick.0 + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_survives_frames_with_no_ticks() {
        let mut i = InputState::default();
        // Press latched while 10 ticks have run; released again before any tick.
        i.set(Action::Jump, true, 11);
        i.set(Action::Jump, false, 11);
        assert!(!i.held(Action::Jump));
        assert!(i.buffered(Action::Jump, 11, 0), "tap must not be lost");
    }

    #[test]
    fn buffer_window_is_inclusive_and_consume_clears() {
        let mut i = InputState::default();
        i.set(Action::Jump, true, 100);
        assert!(i.buffered(Action::Jump, 100, 12));
        assert!(i.buffered(Action::Jump, 112, 12), "age 12 still in window");
        assert!(!i.buffered(Action::Jump, 113, 12), "age 13 expired");
        assert!(i.consume(Action::Jump, 105, 12));
        assert!(!i.consume(Action::Jump, 105, 12), "consumed only once");
    }

    #[test]
    fn holding_does_not_retrigger() {
        let mut i = InputState::default();
        i.set(Action::Dash, true, 1);
        assert!(i.consume(Action::Dash, 1, 0));
        i.set(Action::Dash, true, 2); // still held, no new edge
        assert!(!i.buffered(Action::Dash, 2, 12));
        i.set(Action::Dash, false, 3);
        i.set(Action::Dash, true, 4);
        assert!(i.buffered(Action::Dash, 4, 0));
    }

    #[test]
    fn axes_cancel() {
        let mut i = InputState::default();
        i.set(Action::Left, true, 1);
        assert_eq!(i.axis_x(), -1);
        i.set(Action::Right, true, 1);
        assert_eq!(i.axis_x(), 0);
    }
}
