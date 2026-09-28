//! Renderer-free gameplay simulation for Hollow Knight 3D.
//!
//! Everything that decides *what happens* lives here and runs in the
//! `FixedUpdate` schedule at [`TICK_HZ`]. The `hk_game` crate only draws it.

use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::prelude::*;

pub mod boss;
pub mod camera;
pub mod combat;
pub mod components;
pub mod enemy;
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
            .init_resource::<combat::HitStop>()
            .init_resource::<combat::SimFrozen>()
            .init_resource::<combat::RespawnPoint>()
            .init_resource::<world::room::RoomLibrary>()
            .init_resource::<world::room::CurrentRoom>()
            .init_resource::<world::room::WorldFlags>()
            .init_resource::<world::room::Transition>()
            .add_message::<world::room::RoomEntered>()
            .init_resource::<boss::ArenaLock>()
            .add_message::<boss::BossAwoke>()
            .add_message::<boss::BossPhaseChanged>()
            .add_message::<boss::BossDefeated>()
            .add_message::<combat::Hit>()
            .add_message::<combat::Blocked>()
            .add_message::<combat::PlayerDied>()
            .add_message::<combat::PlayerRespawned>()
            .add_message::<combat::EnemyDied>()
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
        // Hitstop freezes everything from Intent through Status. Input (which
        // latches presses and counts the freeze down) and Cleanup still run.
        app.configure_sets(
            FixedUpdate,
            (
                SimSet::Intent,
                SimSet::Motion,
                SimSet::Collision,
                SimSet::HitDetect,
                SimSet::HitResolve,
                SimSet::Status,
            )
                .run_if(combat::not_frozen),
        );
        app.add_systems(
            FixedUpdate,
            (
                advance_tick.before(SimSet::Input),
                (
                    components::snapshot_prev,
                    world::room::run_transition.before(combat::status::advance_hitstop),
                    combat::status::advance_hitstop,
                )
                    .in_set(SimSet::Input),
                (
                    combat::attack::player_combat,
                    enemy::ai::enemy_ai,
                    boss::ai::boss_ai,
                )
                    .in_set(SimSet::Intent),
                (
                    player::player_movement,
                    combat::attack::hitbox_follow.after(player::player_movement),
                    combat::attack::projectile_motion,
                    boss::ai::glyph_tick,
                    boss::ai::pendulum_motion,
                    (enemy::ai::enemy_motion, combat::attack::knockback_motion).chain(),
                )
                    .in_set(SimSet::Motion),
                combat::detect::detect_hits.in_set(SimSet::HitDetect),
                (
                    combat::resolve::resolve_hits,
                    combat::resolve::resolve_blocks,
                )
                    .in_set(SimSet::HitResolve),
                (
                    combat::status::player_status,
                    combat::status::tick_timers,
                    world::room::detect_exits,
                    boss::ai::update_arena_lock,
                    boss::ai::record_boss_defeat,
                )
                    .in_set(SimSet::Status),
                combat::status::cleanup_expired.in_set(SimSet::Cleanup),
            ),
        );
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
