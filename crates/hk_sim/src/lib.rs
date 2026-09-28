//! Renderer-free gameplay simulation for Hollow Knight 3D.
//!
//! Everything that decides *what happens* lives here and runs in the
//! `FixedUpdate` schedule at [`TICK_HZ`]. The `hk_game` crate only draws it.

use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::prelude::*;

pub mod components;
pub mod input;
pub mod player;
pub mod rng;
pub mod testing;
pub mod tuning;
pub mod world;

/// Simulation rate. All authored durations are in milliseconds and converted
/// with [`ms_to_ticks`], so changing this never changes feel.
pub const TICK_HZ: f64 = 120.0;
/// Fixed simulation step in seconds.
pub const DT: f32 = (1.0 / TICK_HZ) as f32;

/// Converts an authored duration in milliseconds to whole simulation ticks.
pub fn ms_to_ticks(ms: f32) -> u32 {
    (ms * 0.001 * TICK_HZ as f32).round().max(0.0) as u32
}

/// Monotonic tick counter, incremented once at the start of every tick.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimTick(pub u64);

/// Ordered phases of one simulation tick (see plan: fixed-tick pipeline).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Input,
    Intent,
    Motion,
    Collision,
    HitDetect,
    HitResolve,
    Status,
    Cleanup,
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimTick>()
            .init_resource::<input::InputState>()
            .init_resource::<rng::SimRng>()
            .init_resource::<tuning::Tuning>()
            .init_resource::<world::TileGrid>()
            .configure_sets(
                FixedUpdate,
                (
                    SimSet::Input,
                    SimSet::Intent,
                    SimSet::Motion,
                    SimSet::Collision,
                    SimSet::HitDetect,
                    SimSet::HitResolve,
                    SimSet::Status,
                    SimSet::Cleanup,
                )
                    .chain(),
            );
        app.add_systems(FixedUpdate, advance_tick.before(SimSet::Input));
        app.add_systems(FixedUpdate, components::snapshot_prev.in_set(SimSet::Input));
        app.add_systems(FixedUpdate, player::player_movement.in_set(SimSet::Motion));
    }
}

fn advance_tick(mut tick: ResMut<SimTick>) {
    tick.0 += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Message)]
    struct Ping(u32);

    #[derive(Resource, Default)]
    struct Seen(Vec<u32>);

    fn send(mut w: MessageWriter<Ping>, tick: Res<SimTick>) {
        w.write(Ping(tick.0 as u32));
    }
    fn recv(mut r: MessageReader<Ping>, mut seen: ResMut<Seen>) {
        for p in r.read() {
            seen.0.push(p.0);
        }
    }

    #[test]
    fn ticks_advance_when_schedule_runs() {
        let mut app = App::new();
        app.add_plugins(SimPlugin);
        for _ in 0..5 {
            app.world_mut().run_schedule(FixedUpdate);
        }
        assert_eq!(app.world().resource::<SimTick>().0, 5);
    }

    #[test]
    fn messages_flow_between_sets() {
        let mut app = App::new();
        app.add_plugins(SimPlugin)
            .add_message::<Ping>()
            .init_resource::<Seen>()
            .add_systems(
                FixedUpdate,
                (send.in_set(SimSet::Intent), recv.in_set(SimSet::HitResolve)),
            );
        for _ in 0..3 {
            app.world_mut().run_schedule(FixedUpdate);
        }
        assert_eq!(app.world().resource::<Seen>().0, vec![1, 2, 3]);
    }

    #[test]
    fn ms_conversion() {
        assert_eq!(ms_to_ticks(80.0), 10);
        assert_eq!(ms_to_ticks(100.0), 12);
        assert_eq!(ms_to_ticks(1300.0), 156);
    }
}
