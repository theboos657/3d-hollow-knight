//! Drives the 3D camera with the pure rig from `hk_sim::camera`: follow,
//! lookahead, deadzone, look up/down, fall look, room bounds, and trauma shake
//! fed by combat messages.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use hk_sim::camera::{camera_step, Bounds, CameraInput, CameraState};
use hk_sim::combat::{Blocked, EnemyDied, Hit, HitKind, PlayerDied, Team};
use hk_sim::components::Velocity;
use hk_sim::input::{Action, InputState};
use hk_sim::player::{Motor, Player, PlayerState};
use hk_sim::tuning::Tuning;
use hk_sim::world::room::{CurrentRoom, RoomEntered, RoomLibrary};

use crate::interp::RenderPrepSet;
use crate::scene::MainCamera;

#[derive(Resource)]
pub struct Rig(pub CameraState);

pub struct CameraRigPlugin;

impl Plugin for CameraRigPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Rig(CameraState::new(
            Vec2::new(8.0, 8.0),
            Bounds::new(Vec2::ZERO, Vec2::new(64.0, 26.0)),
        )))
        .add_systems(PostStartup, setup_projection)
        .add_systems(Update, camera_rig.after(RenderPrepSet));
    }
}

fn setup_projection(tuning: Res<Tuning>, mut q: Query<&mut Projection, With<MainCamera>>) {
    for mut p in &mut q {
        if let Projection::Perspective(persp) = &mut *p {
            persp.fov = tuning.camera.fov_deg.to_radians();
        }
    }
}
fn camera_rig(
    time: Res<Time>,
    tuning: Res<Tuning>,
    input: Res<InputState>,
    library: Res<RoomLibrary>,
    current: Res<CurrentRoom>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut rig: ResMut<Rig>,
    mut entered: MessageReader<RoomEntered>,
    mut hits: MessageReader<Hit>,
    mut blocked: MessageReader<Blocked>,
    mut enemy_died: MessageReader<EnemyDied>,
    mut player_died: MessageReader<PlayerDied>,
    player: Query<
        (&Transform, &Velocity, &Motor, &PlayerState),
        (With<Player>, Without<MainCamera>),
    >,
    mut cam: Query<&mut Transform, With<MainCamera>>,
) {
    let (Ok((pt, vel, motor, state)), Ok(mut ct), Ok(w)) =
        (player.single(), cam.single_mut(), windows.single())
    else {
        return;
    };
    let t = &tuning.camera;
    let aspect = w.width() / w.height().max(1.0);
    let target = pt.translation.truncate();
    let bounds = library
        .get(&current.id)
        .map(|d| Bounds::new(Vec2::ZERO, Vec2::new(d.width() as f32, d.height() as f32)));

    // New room: jump straight to the player, no glide across the map.
    if entered.read().count() > 0 {
        if let Some(b) = bounds {
            rig.0.snap_to(target, b, t, aspect);
        }
    }

    // Shake from what just happened.
    for h in hits.read() {
        let add = match (h.victim_team, h.kind) {
            (Team::Player, _) => 0.55,
            (_, HitKind::Nail) => 0.16,
            (_, HitKind::Spell) => 0.10,
            _ => 0.0,
        };
        rig.0.add_trauma(add);
    }
    for _ in blocked.read() {
        rig.0.add_trauma(0.10);
    }
    for _ in enemy_died.read() {
        rig.0.add_trauma(0.30);
    }
    for _ in player_died.read() {
        rig.0.add_trauma(0.90);
    }

    let dt = time.delta_secs().min(0.1);
    camera_step(
        &mut rig.0,
        &CameraInput {
            target,
            vel: vel.0,
            grounded: motor.grounded,
            wall_slide: *state == PlayerState::WallSlide,
            look_up: input.held(Action::Up),
            look_down: input.held(Action::Down),
            run_speed: tuning.player.run_speed,
            aspect,
        },
        t,
        dt,
    );
    let p = rig.0.pos() + rig.0.shake_offset(t);
    ct.translation = Vec3::new(p.x, p.y, t.distance);
}
