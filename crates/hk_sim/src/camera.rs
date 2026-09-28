//! Camera rig math. Pure functions of (state, input, tuning, dt) so it can be
//! unit-tested headlessly; `hk_game` just feeds it the player and applies the
//! result to the 3D camera.
//!
//! Behaviour:
//! * **Follow**: critically damped smoothing (frame-rate independent).
//! * **Lookahead**: leads in the movement direction, scaled by speed; the
//!   direction only flips after moving the other way for a moment (hysteresis),
//!   so quick taps do not make the view lurch.
//! * **Deadzone**: while airborne the camera ignores small vertical movement, so
//!   ordinary jumps do not bob the view; it re-anchors when grounded, wall
//!   sliding, or when the player leaves the deadzone.
//! * **Look up/down**: hold Up/Down while standing still to pan.
//! * **Fall look**: falling fast pulls the view down to show what is below.
//! * **Bounds**: the visible footprint at z = 0 never leaves the room; rooms
//!   smaller than the view are centred. Changing rooms blends the bounds.
//! * **Shake**: trauma model (offset = max * trauma^2 * noise), decays over time.

use bevy_math::Vec2;

use crate::tuning::CameraTuning;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl Bounds {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    fn lerp(a: Bounds, b: Bounds, t: f32) -> Bounds {
        Bounds {
            min: a.min.lerp(b.min, t),
            max: a.max.lerp(b.max, t),
        }
    }
}

/// What the rig sees about the player each frame.
#[derive(Clone, Copy, Debug)]
pub struct CameraInput {
    pub target: Vec2,
    pub vel: Vec2,
    pub grounded: bool,
    pub wall_slide: bool,
    pub look_up: bool,
    pub look_down: bool,
    /// Player run speed, to scale the lookahead.
    pub run_speed: f32,
    pub aspect: f32,
}

#[derive(Clone, Debug)]
pub struct CameraState {
    pos: Vec2,
    vel: Vec2,
    look_sign: i8,
    flip_timer: f32,
    focus_y: f32,
    vlook: f32,
    vhold: f32,
    fall_off: f32,
    trauma: f32,
    time: f32,
    bounds: Bounds,
    bounds_from: Bounds,
    bounds_blend: f32,
}

/// Critically damped smoothing, using the exact closed-form solution so the
/// result does not depend on the frame time (only on how often the target is
/// sampled). Overshoot is clamped away.
fn smooth_damp(current: f32, target: f32, vel: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    let omega = 2.0 / smooth_time.max(1e-4);
    let e = (-omega * dt).exp();
    let change = current - target;
    let temp = *vel + omega * change;
    let mut out = target + (change + temp * dt) * e;
    *vel = (*vel - omega * temp * dt) * e;
    if (target - current > 0.0) == (out > target) {
        out = target;
        *vel = 0.0;
    }
    out
}

fn ease_toward(current: f32, target: f32, time: f32, dt: f32) -> f32 {
    let k = 1.0 - (-dt / time.max(1e-4)).exp();
    current + (target - current) * k
}

impl CameraState {
    pub fn new(pos: Vec2, bounds: Bounds) -> Self {
        Self {
            pos,
            vel: Vec2::ZERO,
            look_sign: 1,
            flip_timer: 0.0,
            focus_y: pos.y,
            vlook: 0.0,
            vhold: 0.0,
            fall_off: 0.0,
            trauma: 0.0,
            time: 0.0,
            bounds,
            bounds_from: bounds,
            bounds_blend: 1.0,
        }
    }

    /// Centre of the view (without shake).
    pub fn pos(&self) -> Vec2 {
        self.pos
    }

    pub fn look_sign(&self) -> i8 {
        self.look_sign
    }

    pub fn trauma(&self) -> f32 {
        self.trauma
    }

    pub fn bounds(&self) -> Bounds {
        Bounds::lerp(self.bounds_from, self.bounds, self.bounds_blend)
    }

    /// Jump straight to a spot (room entry): no smoothing, no blend.
    pub fn snap_to(&mut self, target: Vec2, bounds: Bounds, t: &CameraTuning, aspect: f32) {
        self.bounds = bounds;
        self.bounds_from = bounds;
        self.bounds_blend = 1.0;
        self.vel = Vec2::ZERO;
        self.focus_y = target.y;
        self.vlook = 0.0;
        self.vhold = 0.0;
        self.fall_off = 0.0;
        self.pos = clamp_to_bounds(target, half_view(t, aspect), &bounds);
    }

    /// Start blending toward a new room's bounds (adjacent room / zone change).
    pub fn set_bounds(&mut self, bounds: Bounds) {
        self.bounds_from = self.bounds();
        self.bounds = bounds;
        self.bounds_blend = 0.0;
    }

    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
    }

    /// Screen-space shake offset to add to the view centre.
    pub fn shake_offset(&self, t: &CameraTuning) -> Vec2 {
        if self.trauma <= 0.0 {
            return Vec2::ZERO;
        }
        let a = self.trauma * self.trauma * t.shake_max;
        // Cheap deterministic "noise": incommensurate sines.
        let s = self.time;
        Vec2::new(
            (s * 47.0).sin() * 0.6 + (s * 89.0 + 1.3).sin() * 0.4,
            (s * 53.0 + 2.1).sin() * 0.6 + (s * 97.0 + 0.4).sin() * 0.4,
        ) * a
    }
}

pub fn half_view(t: &CameraTuning, aspect: f32) -> Vec2 {
    let h = t.half_view_height();
    Vec2::new(h * aspect, h)
}

/// Keeps the visible footprint inside `b`; rooms smaller than the view are centred.
pub fn clamp_to_bounds(pos: Vec2, half: Vec2, b: &Bounds) -> Vec2 {
    let axis = |v: f32, lo: f32, hi: f32, half: f32| {
        if hi - lo <= 2.0 * half {
            (lo + hi) * 0.5
        } else {
            v.clamp(lo + half, hi - half)
        }
    };
    Vec2::new(
        axis(pos.x, b.min.x, b.max.x, half.x),
        axis(pos.y, b.min.y, b.max.y, half.y),
    )
}

pub fn camera_step(s: &mut CameraState, i: &CameraInput, t: &CameraTuning, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    s.time += dt;
    s.trauma = (s.trauma - t.trauma_decay * dt).max(0.0);
    if s.bounds_blend < 1.0 {
        s.bounds_blend = (s.bounds_blend + dt / (t.bounds_blend_ms * 0.001).max(1e-3)).min(1.0);
    }

    // ---- horizontal lookahead with hysteresis ----
    let moving = i.vel.x.abs() > 0.5 * i.run_speed.max(0.1);
    let dir = if i.vel.x > 0.0 { 1 } else { -1 };
    if moving && dir != s.look_sign {
        s.flip_timer += dt;
        if s.flip_timer >= t.lookahead_flip_ms * 0.001 {
            s.look_sign = dir;
            s.flip_timer = 0.0;
        }
    } else {
        s.flip_timer = 0.0;
    }
    let speed_ratio = (i.vel.x.abs() / i.run_speed.max(0.1)).clamp(0.0, 1.0);
    let lead = s.look_sign as f32 * t.lookahead * speed_ratio;

    // ---- vertical anchor with deadzone ----
    if i.grounded || i.wall_slide {
        s.focus_y = i.target.y;
    } else {
        let d = i.target.y - s.focus_y;
        if d > t.deadzone_half_y {
            s.focus_y = i.target.y - t.deadzone_half_y;
        } else if d < -t.deadzone_half_y {
            s.focus_y = i.target.y + t.deadzone_half_y;
        }
    }

    // ---- look up / down while standing ----
    let idle = i.grounded && i.vel.x.abs() < 0.5;
    let want = if idle && i.look_up != i.look_down {
        if i.look_up {
            1.0
        } else {
            -1.0
        }
    } else {
        0.0
    };
    if want != 0.0 {
        s.vhold += dt;
    } else {
        s.vhold = 0.0;
    }
    let vtarget = if s.vhold >= t.look_hold_ms * 0.001 {
        want * t.look_dist
    } else {
        0.0
    };
    s.vlook = ease_toward(s.vlook, vtarget, t.look_ease_ms * 0.001, dt);

    // ---- fall look ----
    let fall = (-i.vel.y - t.fall_look_start).max(0.0);
    let fall_target = -(fall / 16.0).min(1.0) * t.fall_look_max;
    s.fall_off = ease_toward(s.fall_off, fall_target, 0.25, dt);

    // ---- follow, then clamp to the (possibly blending) room bounds ----
    let half = half_view(t, i.aspect);
    let bounds = s.bounds();
    let goal = clamp_to_bounds(
        Vec2::new(i.target.x + lead, s.focus_y + s.vlook + s.fall_off),
        half,
        &bounds,
    );
    s.pos.x = smooth_damp(s.pos.x, goal.x, &mut s.vel.x, t.follow_x_ms * 0.001, dt);
    s.pos.y = smooth_damp(s.pos.y, goal.y, &mut s.vel.y, t.follow_y_ms * 0.001, dt);
    // Bounds can shrink faster than the smoothing catches up: never show outside.
    s.pos = clamp_to_bounds(s.pos, half, &bounds);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tune() -> CameraTuning {
        CameraTuning::default()
    }
    fn big() -> Bounds {
        Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(200.0, 60.0))
    }
    fn input(target: Vec2, vel: Vec2, grounded: bool) -> CameraInput {
        CameraInput {
            target,
            vel,
            grounded,
            wall_slide: false,
            look_up: false,
            look_down: false,
            run_speed: 9.0,
            aspect: 16.0 / 9.0,
        }
    }
    fn new_cam(at: Vec2) -> CameraState {
        let mut c = CameraState::new(at, big());
        c.snap_to(at, big(), &tune(), 16.0 / 9.0);
        c
    }

    #[test]
    fn smooth_damp_converges_without_overshoot() {
        let (mut x, mut v) = (0.0f32, 0.0f32);
        let mut last = 0.0;
        for _ in 0..600 {
            x = smooth_damp(x, 10.0, &mut v, 0.12, 1.0 / 60.0);
            assert!(x >= last - 1e-6, "monotone");
            assert!(x <= 10.0 + 1e-4, "no overshoot: {x}");
            last = x;
        }
        assert!((x - 10.0).abs() < 1e-3);
    }

    #[test]
    fn view_never_leaves_the_room() {
        let t = tune();
        let room = Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(64.0, 26.0));
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0, 32.0 / 9.0] {
            let half = half_view(&t, aspect);
            let mut c = CameraState::new(Vec2::new(10.0, 10.0), room);
            c.snap_to(Vec2::new(10.0, 10.0), room, &t, aspect);
            let mut rng = crate::rng::SimRng::new(5);
            let mut p = Vec2::new(10.0, 3.0);
            for _ in 0..3000 {
                p += Vec2::new(rng.range_f32(-0.4, 0.4), rng.range_f32(-0.3, 0.3));
                p = p.clamp(Vec2::new(-5.0, -5.0), Vec2::new(70.0, 30.0));
                let mut inp = input(
                    p,
                    Vec2::new(rng.range_f32(-9.0, 9.0), rng.range_f32(-20.0, 20.0)),
                    rng.chance(0.5),
                );
                inp.aspect = aspect;
                camera_step(&mut c, &inp, &t, 1.0 / 60.0);
                let q = c.pos();
                assert!(q.x.is_finite() && q.y.is_finite());
                assert!(
                    q.x - half.x >= room.min.x - 1e-3 && q.x + half.x <= room.max.x + 1e-3
                        || (room.max.x - room.min.x) <= 2.0 * half.x
                );
                assert!(
                    q.y - half.y >= room.min.y - 1e-3 && q.y + half.y <= room.max.y + 1e-3
                        || (room.max.y - room.min.y) <= 2.0 * half.y
                );
            }
        }
    }

    #[test]
    fn rooms_smaller_than_the_view_are_centred() {
        let t = tune();
        let tiny = Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 8.0));
        let mut c = CameraState::new(Vec2::ZERO, tiny);
        c.snap_to(Vec2::new(3.0, 2.0), tiny, &t, 16.0 / 9.0);
        camera_step(
            &mut c,
            &input(Vec2::new(3.0, 2.0), Vec2::ZERO, true),
            &t,
            1.0 / 60.0,
        );
        assert_eq!(c.pos(), Vec2::new(5.0, 4.0));
    }

    #[test]
    fn running_is_smooth_and_the_view_leads() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 10.0));
        let dt = 1.0 / 60.0;
        let mut px = 30.0;
        let mut last_x = c.pos().x;
        let mut last_v = 0.0;
        let mut max_jerk = 0.0f32;
        for step in 0..360 {
            px += 9.0 * dt;
            camera_step(
                &mut c,
                &input(Vec2::new(px, 10.0), Vec2::new(9.0, 0.0), true),
                &t,
                dt,
            );
            let v = (c.pos().x - last_x) / dt;
            if step > 60 {
                max_jerk = max_jerk.max((v - last_v).abs());
                assert!(v > 0.0, "never moves backwards while the player runs right");
            }
            last_x = c.pos().x;
            last_v = v;
        }
        assert!(
            (last_v - 9.0).abs() < 0.3,
            "camera speed matches the player: {last_v}"
        );
        assert!(
            max_jerk < 0.5,
            "steady run must not jitter, jerk {max_jerk}"
        );
        // A smoothed follower lags a constant-speed target by speed * smooth
        // time, so the net lead is lookahead minus that lag.
        let lag = 9.0 * t.follow_x_ms * 0.001;
        let lead = c.pos().x - px;
        assert!(
            (lead - (t.lookahead - lag)).abs() < 0.3,
            "net lead {lead}, expected ~{}",
            t.lookahead - lag
        );
        assert!(
            lead > 1.5,
            "the view must show clearly more of what is ahead: {lead}"
        );
    }

    /// The same motion sampled at 30 / 60 / 120 / 240 fps must give the same
    /// camera, both mid-run (where the lag matters) and after settling.
    #[test]
    fn follow_is_frame_rate_independent() {
        let t = tune();
        // Player path is analytic so every frame rate sees identical motion:
        // run right at 9 u/s for 1.5 s, then stand still.
        let player_x = |time: f32| 30.0 + 9.0 * time.min(1.5);
        let run = |fps: u32| {
            let mut c = new_cam(Vec2::new(30.0, 10.0));
            let dt = 1.0 / fps as f32;
            let mut at_1s = Vec2::ZERO;
            for step in 1..=(3 * fps) {
                let time = step as f32 * dt;
                let v = if time <= 1.5 { 9.0 } else { 0.0 };
                camera_step(
                    &mut c,
                    &input(Vec2::new(player_x(time), 10.0), Vec2::new(v, 0.0), true),
                    &t,
                    dt,
                );
                if step == fps {
                    at_1s = c.pos();
                }
            }
            (at_1s, c.pos())
        };
        let reference = run(120);
        for fps in [30, 60, 240] {
            let (mid, end) = run(fps);
            // Sampling the moving player once per frame biases the follower by
            // about half a frame of player travel (v * dt / 2); that is the only
            // frame-rate dependence allowed.
            let bound = 4.5 / fps as f32 + 4.5 / 120.0 + 0.02;
            assert!(
                (mid - reference.0).length() < bound,
                "{fps} fps mid-run {mid} vs {} (bound {bound})",
                reference.0
            );
            assert!(
                (end - reference.1).length() < 0.01,
                "{fps} fps settled {end} vs {}",
                reference.1
            );
        }
        assert!((reference.1.x - 43.5).abs() < 0.01, "settles on the player");
    }

    #[test]
    fn lookahead_ignores_quick_taps_but_flips_when_sustained() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 10.0));
        let dt = 1.0 / 60.0;
        let mut px = 30.0;
        for _ in 0..60 {
            px += 9.0 * dt;
            camera_step(
                &mut c,
                &input(Vec2::new(px, 10.0), Vec2::new(9.0, 0.0), true),
                &t,
                dt,
            );
        }
        assert_eq!(c.look_sign(), 1);
        // A 0.15 s tap the other way (< 0.25 s hysteresis).
        for _ in 0..9 {
            px -= 9.0 * dt;
            camera_step(
                &mut c,
                &input(Vec2::new(px, 10.0), Vec2::new(-9.0, 0.0), true),
                &t,
                dt,
            );
        }
        assert_eq!(c.look_sign(), 1, "a quick tap must not flip the lookahead");
        // Now keep going left.
        for _ in 0..40 {
            px -= 9.0 * dt;
            camera_step(
                &mut c,
                &input(Vec2::new(px, 10.0), Vec2::new(-9.0, 0.0), true),
                &t,
                dt,
            );
        }
        assert_eq!(c.look_sign(), -1, "sustained movement flips it");
    }

    #[test]
    fn a_normal_jump_does_not_move_the_camera_vertically() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 10.0));
        let dt = 1.0 / 120.0;
        for _ in 0..200 {
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, 10.0), Vec2::ZERO, true),
                &t,
                dt,
            );
        }
        let y0 = c.pos().y;
        // Hop 1.0 up (inside the +-1.25 deadzone) and back.
        for k in 0..120 {
            let y = 10.0 + (k as f32 / 120.0 * std::f32::consts::PI).sin();
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, y), Vec2::ZERO, false),
                &t,
                dt,
            );
            assert!(
                (c.pos().y - y0).abs() < 0.02,
                "camera bobbed: {}",
                c.pos().y - y0
            );
        }
    }

    #[test]
    fn leaving_the_deadzone_follows_and_landing_reanchors() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 10.0));
        let dt = 1.0 / 120.0;
        // Jump to +4: well outside the deadzone, so the camera follows partway.
        for _ in 0..200 {
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, 14.0), Vec2::ZERO, false),
                &t,
                dt,
            );
        }
        let expected = 14.0 - t.deadzone_half_y;
        assert!(
            (c.pos().y - expected).abs() < 0.1,
            "held at the deadzone edge: {} vs {expected}",
            c.pos().y
        );
        // Land on the higher ground: the camera settles on the player.
        for _ in 0..400 {
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, 14.0), Vec2::ZERO, true),
                &t,
                dt,
            );
        }
        assert!((c.pos().y - 14.0).abs() < 0.05);
    }

    #[test]
    fn holding_up_pans_after_the_delay_and_releasing_returns() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 10.0));
        let dt = 1.0 / 120.0;
        let mut inp = input(Vec2::new(30.0, 10.0), Vec2::ZERO, true);
        inp.look_up = true;
        for _ in 0..(0.4 / dt) as usize {
            camera_step(&mut c, &inp, &t, dt);
        }
        assert!(c.pos().y < 10.05, "no pan before the 500 ms hold");
        for _ in 0..(1.5 / dt) as usize {
            camera_step(&mut c, &inp, &t, dt);
        }
        assert!(
            (c.pos().y - (10.0 + t.look_dist)).abs() < 0.3,
            "panned up: {}",
            c.pos().y
        );
        inp.look_up = false;
        for _ in 0..(2.0 / dt) as usize {
            camera_step(&mut c, &inp, &t, dt);
        }
        assert!((c.pos().y - 10.0).abs() < 0.1, "returned");
        // Moving cancels the look (it only works while standing).
        inp.look_up = true;
        inp.vel = Vec2::new(9.0, 0.0);
        for _ in 0..(1.5 / dt) as usize {
            camera_step(&mut c, &inp, &t, dt);
        }
        assert!(c.pos().y < 10.3, "no look while running");
    }

    #[test]
    fn falling_fast_pulls_the_view_down_up_to_the_cap() {
        let t = tune();
        let mut c = new_cam(Vec2::new(30.0, 20.0));
        let dt = 1.0 / 120.0;
        // Slow fall: nothing.
        for _ in 0..240 {
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, 20.0), Vec2::new(0.0, -6.0), false),
                &t,
                dt,
            );
        }
        assert!((c.pos().y - 20.0).abs() < 0.05);
        // Terminal velocity: pulled down by (about) the cap, never more.
        for _ in 0..600 {
            camera_step(
                &mut c,
                &input(Vec2::new(30.0, 20.0), Vec2::new(0.0, -24.0), false),
                &t,
                dt,
            );
        }
        let off = 20.0 - c.pos().y;
        assert!(
            off > t.fall_look_max - 0.3 && off <= t.fall_look_max + 0.01,
            "fall look {off}"
        );
    }

    #[test]
    fn shake_is_bounded_decays_and_is_deterministic() {
        let t = tune();
        let run = || {
            let mut c = new_cam(Vec2::new(30.0, 10.0));
            c.add_trauma(1.0);
            let mut peak = 0.0f32;
            let mut trace = Vec::new();
            for _ in 0..240 {
                camera_step(
                    &mut c,
                    &input(Vec2::new(30.0, 10.0), Vec2::ZERO, true),
                    &t,
                    1.0 / 120.0,
                );
                let o = c.shake_offset(&t);
                peak = peak.max(o.length());
                trace.push(o);
            }
            (peak, c.trauma(), trace)
        };
        let (peak, end_trauma, a) = run();
        assert!(peak > 0.05, "visible shake at full trauma: {peak}");
        assert!(
            peak <= t.shake_max * std::f32::consts::SQRT_2 + 1e-3,
            "bounded: {peak}"
        );
        assert_eq!(end_trauma, 0.0, "1.0 trauma is gone after 2 s at 1.5/s");
        assert_eq!(run().2, a, "same input, same shake");
        let c = new_cam(Vec2::ZERO);
        assert_eq!(c.shake_offset(&t), Vec2::ZERO, "no trauma, no shake");
    }

    /// Locking the camera to a boss arena tightens the bounds: the view glides
    /// there over the blend time instead of popping.
    #[test]
    fn locking_to_an_arena_glides_over_the_blend_time() {
        let t = tune();
        let room = Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(64.0, 26.0));
        let arena = Bounds::new(Vec2::new(16.0, 0.0), Vec2::new(48.0, 26.0));
        let half = half_view(&t, 16.0 / 9.0);
        let mut c = CameraState::new(Vec2::new(55.0, 10.0), room);
        c.snap_to(Vec2::new(55.0, 10.0), room, &t, 16.0 / 9.0);
        let dt = 1.0 / 60.0;
        for _ in 0..120 {
            camera_step(
                &mut c,
                &input(Vec2::new(46.0, 10.0), Vec2::ZERO, true),
                &t,
                dt,
            );
        }
        let before = c.pos().x; // following the player at x = 46
        assert!(
            (before - 46.0).abs() < 0.1,
            "camera is on the player: {before}"
        );
        c.set_bounds(arena);
        let mut last = before;
        let mut biggest = 0.0f32;
        let mut frames = 0;
        while (c.pos().x - (48.0 - half.x)).abs() > 0.05 && frames < 300 {
            camera_step(
                &mut c,
                &input(Vec2::new(46.0, 10.0), Vec2::ZERO, true),
                &t,
                dt,
            );
            biggest = biggest.max((c.pos().x - last).abs());
            last = c.pos().x;
            frames += 1;
        }
        // The edge moves (64-48) = 16 u over 0.4 s = 40 u/s => ~0.67 u per frame.
        assert!(
            biggest < 0.75,
            "glides at the blend rate, no pop: {biggest}"
        );
        assert!(
            frames as f32 * dt >= 0.3,
            "takes about the blend time, took {frames} frames"
        );
        assert!(
            c.pos().x + half.x <= 48.0 + 1e-3,
            "view is inside the arena"
        );
    }
}
