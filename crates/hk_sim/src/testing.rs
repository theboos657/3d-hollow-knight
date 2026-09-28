//! Headless harness: drives the sim one tick at a time with scripted input.
//! Used by unit tests, replay tests and the `hk_tools` bots.

use bevy_app::{App, FixedUpdate};
use bevy_ecs::prelude::*;

use crate::input::{Action, InputState};
use crate::{SimPlugin, SimTick};

pub struct Harness {
    pub app: App,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    pub fn new() -> Self {
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        Self { app }
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Ticks that have already run.
    pub fn tick_count(&self) -> u64 {
        self.world().resource::<SimTick>().0
    }

    /// Runs exactly one simulation tick.
    pub fn tick(&mut self) {
        self.app.world_mut().run_schedule(FixedUpdate);
    }

    pub fn tick_n(&mut self, n: u32) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Latches a device change so the *next* tick sees it (age 0 for presses).
    pub fn set(&mut self, action: Action, down: bool) {
        let next = self.tick_count() + 1;
        self.world_mut()
            .resource_mut::<InputState>()
            .set(action, down, next);
    }

    pub fn press(&mut self, action: Action) {
        self.set(action, true);
    }

    pub fn release(&mut self, action: Action) {
        self.set(action, false);
    }

    /// Hold `action` for `ticks` ticks, then release it (release applies to
    /// the tick after the hold, so the hold is exactly `ticks` ticks long).
    pub fn hold_for(&mut self, action: Action, ticks: u32) {
        self.press(action);
        self.tick_n(ticks);
        self.release(action);
    }
}
