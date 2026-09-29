#![allow(dead_code)] // helpers are used progressively by the models
//! Animation maths for the models: pure functions from simulation state and a
//! clock to joint transforms, so all of it is unit-tested without a renderer.
//!
//! Model space: feet at the origin, +X is "forward" (the way the creature
//! faces), +Y up. Angles are radians about Z, counter-clockwise from +X, so
//! 0 points forward, 90 degrees up, 180 degrees back.

use std::f32::consts::{PI, TAU};

pub fn deg(d: f32) -> f32 {
    d.to_radians()
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Frame-rate independent exponential approach: moves `current` toward
/// `target` covering `1 - e^(-rate*dt)` of the gap.
pub fn damp(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    target + (current - target) * (-rate * dt).exp()
}

/// Shortest signed angle from `a` to `b`.
pub fn angle_diff(a: f32, b: f32) -> f32 {
    (b - a + PI).rem_euclid(TAU) - PI
}

/// A damped spring for things that trail behind (cape, dummies).
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    /// Semi-implicit step toward `target` with stiffness `k` and damping `c`.
    pub fn step(&mut self, target: f32, dt: f32, k: f32, c: f32) {
        // Sub-step so a long frame cannot blow the spring up.
        let n = ((dt / 0.008).ceil() as usize).clamp(1, 16);
        let h = dt / n as f32;
        for _ in 0..n {
            let a = k * (target - self.x) - c * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }
}

// ------------------------------------------------------------------- joints --

/// A joint's offset from its rest transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointXf {
    pub pos: [f32; 3],
    /// Rotation about Z, radians.
    pub rot: f32,
    pub scale: [f32; 3],
}

impl JointXf {
    pub const REST: JointXf = JointXf {
        pos: [0.0; 3],
        rot: 0.0,
        scale: [1.0; 3],
    };

    pub fn at(x: f32, y: f32) -> Self {
        Self {
            pos: [x, y, 0.0],
            ..Self::REST
        }
    }

    pub fn rot(rot: f32) -> Self {
        Self { rot, ..Self::REST }
    }

    pub fn with_pos(mut self, x: f32, y: f32) -> Self {
        self.pos = [x, y, 0.0];
        self
    }
}

// -------------------------------------------------------------------- swing --

/// Which way the nail is swung (mirrors `hk_sim::combat::AttackDir`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SwingDir {
    Forward,
    Up,
    Down,
}

/// Swing timing in sim ticks (from the combat tuning).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwingTiming {
    /// Ticks before the hitbox appears.
    pub startup: u32,
    /// Ticks the hitbox is live.
    pub active: u32,
    /// Ticks to bring the blade back to rest after the hit window.
    pub settle: u32,
}

impl SwingTiming {
    pub fn total(&self) -> f32 {
        (self.startup + self.active + self.settle) as f32
    }
}

/// Blade angle at rest: over the shoulder, like a sword slung on the back.
pub const REST_ANGLE_DEG: f32 = 140.0;

/// (angle at the end of the coil, angle at the end of the sweep), degrees.
pub fn swing_arc_deg(dir: SwingDir) -> (f32, f32) {
    match dir {
        // A chop from up-and-forward down through the front.
        SwingDir::Forward => (60.0, -25.0),
        // A sweep over the head, from behind it to up-and-forward.
        SwingDir::Up => (160.0, 55.0),
        // Raise, then plunge straight down.
        SwingDir::Down => (100.0, -90.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingPose {
    /// Blade angle (radians, model space).
    pub angle: f32,
    /// Extra reach along the blade (the down-thrust).
    pub thrust: f32,
    /// Body lean into the swing (radians, positive = forward).
    pub lean: f32,
    /// How much of the crescent trail is revealed, 0..1 (its head position).
    pub trail_head: f32,
    /// Trail brightness, 0..1.
    pub trail_alpha: f32,
}

/// The blade during a swing, `t` ticks after it started (fractional ticks are
/// fine: the renderer interpolates). `None` once the blade is back at rest.
pub fn swing_pose(dir: SwingDir, t: f32, tm: SwingTiming) -> Option<SwingPose> {
    if t < 0.0 || t >= tm.total() {
        return None;
    }
    let (coil, end) = swing_arc_deg(dir);
    let hit_end = (tm.startup + tm.active) as f32;
    let st = tm.settle as f32;
    let rest = REST_ANGLE_DEG;
    // A short anticipation, then the strike. The blade is well into its sweep
    // by the tick the hitbox appears (`startup`), so on the frame of impact
    // (which freezes for hitstop) it is where the enemy is being hit.
    if t < COIL_TICKS {
        let k = smoothstep(t / COIL_TICKS);
        return Some(SwingPose {
            angle: deg(lerp(rest, coil, k)),
            thrust: 0.0,
            lean: -0.14 * k,
            trail_head: 0.0,
            trail_alpha: 0.0,
        });
    }
    if t < hit_end {
        let raw = ((t - COIL_TICKS) / (hit_end - COIL_TICKS)).clamp(0.0, 1.0);
        // The down-plunge is nearly instant, then the blade holds pointing down.
        let k = match dir {
            SwingDir::Down => ease_out_cubic(((t - COIL_TICKS) / 3.0).clamp(0.0, 1.0)),
            _ => ease_out_cubic(raw),
        };
        let thrust = match dir {
            SwingDir::Down => 0.15 * (PI * raw).sin(),
            _ => 0.0,
        };
        return Some(SwingPose {
            angle: deg(lerp(coil, end, k)),
            thrust,
            lean: -0.14 + 0.36 * k.max(0.4),
            trail_head: k,
            trail_alpha: 1.0,
        });
    }
    let k = (t - hit_end) / st.max(1.0);
    Some(SwingPose {
        angle: deg(lerp(end, rest, smoothstep(k))),
        thrust: 0.0,
        lean: lerp(0.1, 0.0, smoothstep(k)),
        trail_head: 1.0,
        trail_alpha: (1.0 - k * 1.7).clamp(0.0, 1.0),
    })
}

/// Ticks of anticipation before the strike.
pub const COIL_TICKS: f32 = 1.5;

// -------------------------------------------------------------------- knight --

/// Joint indices for the knight rig.
pub mod knight_joint {
    pub const BODY: usize = 0;
    pub const HEAD: usize = 1;
    pub const SWORD_ARM: usize = 2;
    pub const OFF_ARM: usize = 3;
    pub const LEG_FRONT: usize = 4;
    pub const LEG_BACK: usize = 5;
    pub const CAPE_1: usize = 6;
    pub const CAPE_2: usize = 7;
    pub const CAPE_3: usize = 8;
    pub const LANTERN: usize = 9;
    pub const COUNT: usize = 10;
}

/// What the knight is doing this frame, as far as the pose cares.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KnightIn {
    pub grounded: bool,
    /// Sim velocity, units per second, in world axes.
    pub vx: f32,
    pub vy: f32,
    /// +1 wall on the right, -1 left, 0 none (model-independent).
    pub wall_slide: bool,
    pub dashing: bool,
    pub focusing: bool,
    pub hurt: bool,
    pub dead: bool,
    /// Seconds (idle breathing, sword sway).
    pub clock: f32,
    /// Accumulated run-cycle phase, radians.
    pub run_phase: f32,
    /// The swing, if any.
    pub swing: Option<SwingPose>,
    /// Soul as 0..1 (the blade glows with it).
    pub soul: f32,
    /// Cape segment angles from the spring (radians, relative).
    pub cape: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnightPose {
    pub joints: [JointXf; knight_joint::COUNT],
    /// Squash and stretch of the whole model (x = z, y).
    pub squash: [f32; 2],
    /// Whole-model lean about the feet (radians, positive = forward).
    pub lean: f32,
    /// Vertical offset of the whole model (kneeling, collapsing).
    pub drop: f32,
    /// Extra reach along the blade.
    pub thrust: f32,
    /// Blade edge glow, 0..1.
    pub glow: f32,
}

impl KnightPose {
    pub fn blade_angle(&self) -> f32 {
        self.joints[knight_joint::SWORD_ARM].rot
    }
}

/// Run-cycle angular speed for a given ground speed (radians per second).
pub fn run_cycle_rate(speed: f32) -> f32 {
    // About 3.4 full cycles a second at the top speed of 9 units/s.
    TAU * (speed.abs() / 2.65)
}

pub fn knight_pose(i: &KnightIn) -> KnightPose {
    use knight_joint::*;
    let mut j = [JointXf::REST; COUNT];
    let speed = i.vx.abs();
    let breathe = (i.clock * TAU * 0.7).sin();
    let mut lean = 0.0;
    let mut drop = 0.0;
    let mut squash = [1.0, 1.0];
    let mut blade = deg(REST_ANGLE_DEG) + 0.04 * (i.clock * 1.3).sin();
    let mut thrust = 0.0;

    // ---- locomotion ----
    if i.dead {
        // Crumple forward and down.
        lean = 1.25;
        drop = -0.35;
        blade = deg(200.0);
        j[LEG_FRONT] = JointXf::rot(-0.5);
        j[LEG_BACK] = JointXf::rot(0.4);
    } else if i.focusing {
        // Kneel with the blade planted point-down.
        drop = -0.28;
        lean = 0.08;
        blade = deg(-100.0);
        j[LEG_FRONT] = JointXf::rot(-1.1);
        j[LEG_BACK] = JointXf::rot(0.9);
        j[HEAD] = JointXf::rot(-0.18);
    } else if i.hurt {
        lean = -0.35;
        blade = deg(120.0);
        j[HEAD] = JointXf::rot(0.25);
        j[LEG_FRONT] = JointXf::rot(0.5);
        j[LEG_BACK] = JointXf::rot(-0.3);
    } else if i.dashing {
        lean = 0.42;
        blade = deg(178.0);
        j[LEG_FRONT] = JointXf::rot(-0.9);
        j[LEG_BACK] = JointXf::rot(0.9);
        j[OFF_ARM] = JointXf::rot(deg(200.0) - deg(-90.0));
        squash = [0.92, 1.0];
    } else if i.wall_slide {
        lean = -0.12;
        j[LEG_FRONT] = JointXf::rot(-0.3);
        j[LEG_BACK] = JointXf::rot(-0.5);
        j[OFF_ARM] = JointXf::rot(-0.6);
    } else if !i.grounded {
        // Airborne: legs tucked while rising, trailing when falling.
        let rising = i.vy > 0.5;
        let k = (i.vy.abs() / 18.0).min(1.0);
        if rising {
            j[LEG_FRONT] = JointXf::rot(lerp(-0.5, -0.9, k));
            j[LEG_BACK] = JointXf::rot(lerp(0.4, 0.8, k));
            lean = 0.06;
        } else {
            j[LEG_FRONT] = JointXf::rot(-0.25);
            j[LEG_BACK] = JointXf::rot(0.5 * k);
            j[OFF_ARM] = JointXf::rot(0.5 * k);
        }
        let stretch = (i.vy.abs() / 40.0).min(0.2);
        squash = [1.0 - stretch * 0.5, 1.0 + stretch];
    } else if speed > 0.6 {
        // Running: legs and arm swing, body bobs twice per cycle.
        let ph = i.run_phase;
        let amp = 0.9 * (speed / 9.0).clamp(0.4, 1.0);
        j[LEG_FRONT] = JointXf::rot(amp * ph.sin());
        j[LEG_BACK] = JointXf::rot(-amp * ph.sin());
        j[OFF_ARM] = JointXf::rot(-0.7 * ph.sin());
        let bob = 0.04 * (ph * 2.0).cos().abs();
        j[BODY] = JointXf::at(0.0, bob);
        j[HEAD] = JointXf::at(0.0, bob * 0.5);
        lean = 0.14 * (speed / 9.0).min(1.0);
    } else {
        // Idle: breathing.
        j[BODY] = JointXf::at(0.0, 0.012 * breathe);
        j[HEAD] = JointXf::at(0.0, 0.02 * breathe);
        j[OFF_ARM] = JointXf::rot(0.05 * breathe);
        j[LEG_FRONT] = JointXf::rot(-0.06);
        j[LEG_BACK] = JointXf::rot(0.06);
    }

    // ---- the swing overrides the blade (and adds lean) ----
    if let Some(s) = i.swing {
        blade = s.angle;
        thrust = s.thrust;
        if !i.dead && !i.hurt {
            lean += s.lean;
        }
        // A lunge onto the front foot.
        if i.grounded && !i.dashing {
            j[LEG_FRONT] = JointXf::rot(-0.45 * s.lean.max(0.0));
            j[LEG_BACK] = JointXf::rot(0.35 * s.lean.max(0.0));
        }
    }

    // The sword arm points along the blade.
    j[SWORD_ARM] = JointXf::rot(blade);

    // Cape segments hang and stream according to the springs (see `step_cape`).
    for k in 0..3 {
        j[CAPE_1 + k] = JointXf::rot(i.cape[k]);
    }
    // The lantern swings with the body.
    j[LANTERN] = JointXf::rot(-0.15 * (i.vx / 9.0) + 0.05 * (i.clock * 2.1).sin());

    let glow = if i.soul >= 1.0 / 3.0 {
        0.55 + 0.45 * (i.clock * TAU * 1.4).sin().abs()
    } else {
        i.soul * 1.2
    };
    KnightPose {
        joints: j,
        squash,
        lean,
        drop,
        thrust,
        glow: glow.clamp(0.0, 1.0),
    }
}

/// Steps the three cape segments. The cape mesh extends backward (-X) from
/// the shoulder, so a positive angle means the tail hangs down and a negative
/// one means it is blown up. Standing it hangs, running it streams out
/// behind, falling it whips up, rising it drops. Later segments bend less and
/// lag behind the first.
pub fn step_cape(state: &mut [Spring; 3], speed: f32, vy: f32, dt: f32) {
    let run = (speed.abs() / 9.0).clamp(0.0, 1.0);
    let fall = (-vy / 18.0).clamp(0.0, 1.0);
    let rise = (vy / 18.0).clamp(0.0, 1.0);
    let a = 1.15 - 0.85 * run - 0.9 * fall + 0.25 * rise;
    let targets = [a, 0.35 * a, 0.3 * a];
    for k in 0..3 {
        state[k].step(targets[k], dt, 90.0 - 22.0 * k as f32, 9.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TM: SwingTiming = SwingTiming {
        startup: 4,
        active: 11,
        settle: 12,
    };

    #[test]
    fn swing_hits_its_table_endpoints() {
        for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
            let (coil, end) = swing_arc_deg(dir);
            let at_start = swing_pose(dir, 0.0, TM).unwrap();
            assert!(
                (at_start.angle - deg(REST_ANGLE_DEG)).abs() < 1e-4,
                "{dir:?} starts at rest"
            );
            // A short coil, then the strike begins.
            let at_coil = swing_pose(dir, COIL_TICKS, TM).unwrap();
            assert!((at_coil.angle - deg(coil)).abs() < 1e-4, "{dir:?} coil");
            // The sweep ends when the hitbox goes away (age 15).
            let at_end = swing_pose(dir, 15.0, TM).unwrap();
            assert!((at_end.angle - deg(end)).abs() < 1e-4, "{dir:?} sweep end");
            // And the blade is home again after the settle.
            assert!(swing_pose(dir, TM.total(), TM).is_none());
            let almost = swing_pose(dir, TM.total() - 0.01, TM).unwrap();
            assert!(
                (almost.angle - deg(REST_ANGLE_DEG)).abs() < 0.02,
                "{dir:?} settles to rest"
            );
        }
    }

    #[test]
    fn the_blade_is_well_into_its_strike_when_the_hitbox_appears() {
        // The hit lands at age `startup` and the frame freezes there for
        // hitstop, so the sword must already be through most of its arc.
        for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
            let (coil, end) = swing_arc_deg(dir);
            let at_hit = swing_pose(dir, TM.startup as f32, TM)
                .unwrap()
                .angle
                .to_degrees();
            let progress = (at_hit - coil) / (end - coil);
            assert!(
                progress > 0.4,
                "{dir:?} only {:.0}% through the arc at the hit",
                progress * 100.0
            );
        }
    }

    #[test]
    fn swing_is_continuous_and_never_teleports() {
        for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
            let mut prev = swing_pose(dir, 0.0, TM).unwrap().angle;
            let mut t = 0.05;
            while t < TM.total() {
                let a = swing_pose(dir, t, TM).unwrap().angle;
                // No more than ~70 degrees in a twentieth of a tick... i.e. the
                // fastest part (the down-plunge) is still a smooth ramp.
                assert!(
                    (a - prev).abs() < 0.5,
                    "{dir:?} jumped {} at t={t}",
                    (a - prev).abs()
                );
                prev = a;
                t += 0.05;
            }
        }
    }

    #[test]
    fn the_trail_reveals_during_the_sweep_and_fades_after() {
        for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
            let early = swing_pose(dir, 1.0, TM).unwrap();
            assert_eq!(early.trail_alpha, 0.0, "no trail during the coil");
            let mid = swing_pose(dir, 9.0, TM).unwrap();
            assert!(mid.trail_head > 0.2 && mid.trail_alpha == 1.0);
            let end = swing_pose(dir, 15.0, TM).unwrap();
            assert!((end.trail_head - 1.0).abs() < 1e-3);
            let fading = swing_pose(dir, 22.0, TM).unwrap();
            assert!(fading.trail_alpha < 1.0);
            let gone = swing_pose(dir, 26.9, TM).unwrap();
            assert_eq!(gone.trail_alpha, 0.0);
        }
    }

    #[test]
    fn the_down_plunge_is_vertical_for_most_of_the_hit_window() {
        // The pogo needs to read: pointing straight down by the third live tick.
        let p = swing_pose(SwingDir::Down, 7.0, TM).unwrap();
        assert!(
            (p.angle - deg(-90.0)).abs() < 0.05,
            "angle {}",
            p.angle.to_degrees()
        );
        let later = swing_pose(SwingDir::Down, 12.0, TM).unwrap();
        assert!((later.angle - deg(-90.0)).abs() < 0.01);
    }

    #[test]
    fn idle_breathing_is_periodic() {
        let a = knight_pose(&KnightIn {
            grounded: true,
            clock: 1.0,
            ..Default::default()
        });
        let period = 1.0 / 0.7;
        let b = knight_pose(&KnightIn {
            grounded: true,
            clock: 1.0 + period,
            ..Default::default()
        });
        for k in 0..knight_joint::COUNT {
            assert!(
                (a.joints[k].pos[1] - b.joints[k].pos[1]).abs() < 1e-3,
                "joint {k}"
            );
        }
    }

    #[test]
    fn the_run_cycle_swings_the_legs_in_opposition_and_repeats() {
        let run = |ph: f32| {
            knight_pose(&KnightIn {
                grounded: true,
                vx: 9.0,
                run_phase: ph,
                ..Default::default()
            })
        };
        let p = run(1.0);
        assert!(
            (p.joints[knight_joint::LEG_FRONT].rot + p.joints[knight_joint::LEG_BACK].rot).abs()
                < 1e-5
        );
        let q = run(1.0 + TAU);
        assert!(
            (p.joints[knight_joint::LEG_FRONT].rot - q.joints[knight_joint::LEG_FRONT].rot).abs()
                < 1e-4
        );
        assert!(run_cycle_rate(9.0) > run_cycle_rate(4.0));
    }

    #[test]
    fn states_have_distinct_silhouettes() {
        let base = KnightIn {
            grounded: true,
            ..Default::default()
        };
        let idle = knight_pose(&base);
        let dash = knight_pose(&KnightIn {
            dashing: true,
            ..base
        });
        let focus = knight_pose(&KnightIn {
            focusing: true,
            ..base
        });
        let dead = knight_pose(&KnightIn { dead: true, ..base });
        assert!(dash.lean > idle.lean + 0.3, "dash leans into it");
        assert!(focus.drop < -0.2, "focus kneels");
        assert!(dead.lean > 1.0 && dead.drop < 0.0, "death collapses");
        // The blade shows: dash trails it back, focus plants it down.
        assert!(dash.blade_angle() > deg(170.0));
        assert!(focus.blade_angle() < deg(-90.0));
    }

    #[test]
    fn a_swing_leans_the_body_and_moves_the_blade() {
        let base = KnightIn {
            grounded: true,
            ..Default::default()
        };
        let swing = swing_pose(SwingDir::Forward, 9.0, TM);
        let p = knight_pose(&KnightIn { swing, ..base });
        assert!(p.lean > 0.05);
        assert!((p.blade_angle() - swing.unwrap().angle).abs() < 1e-6);
    }

    #[test]
    fn the_blade_glows_with_soul() {
        let g = |soul: f32| {
            knight_pose(&KnightIn {
                grounded: true,
                soul,
                ..Default::default()
            })
            .glow
        };
        assert!(g(0.0) < 0.01);
        assert!(g(0.2) > 0.0 && g(0.2) < 0.4);
        assert!(g(0.5) >= 0.55, "castable: bright");
    }

    #[test]
    fn springs_settle_on_their_target_and_do_not_explode() {
        let mut s = Spring::default();
        for _ in 0..600 {
            s.step(1.0, 1.0 / 60.0, 80.0, 9.0);
        }
        assert!((s.x - 1.0).abs() < 1e-3);
        // One enormous frame stays bounded.
        let mut s = Spring::default();
        s.step(1.0, 0.5, 80.0, 9.0);
        assert!(s.x.abs() < 5.0);
    }

    #[test]
    fn cape_hangs_when_still_streams_out_when_running_and_whips_up_when_falling() {
        let settle = |speed: f32, vy: f32| {
            let mut c = [Spring::default(); 3];
            for _ in 0..300 {
                step_cape(&mut c, speed, vy, 1.0 / 60.0);
            }
            c
        };
        let still = settle(0.0, 0.0);
        let run = settle(9.0, 0.0);
        let fall = settle(0.0, -18.0);
        assert!(still[0].x > 0.9, "hangs down: {}", still[0].x);
        assert!(
            run[0].x < still[0].x - 0.6,
            "streams out behind when running"
        );
        assert!(fall[0].x < 0.4, "whips up when falling");
        assert!(still[1].x < still[0].x, "later segments bend less");
    }

    #[test]
    fn damp_and_angle_helpers() {
        assert!((damp(0.0, 1.0, 100.0, 1.0) - 1.0).abs() < 1e-4);
        assert!((damp(0.0, 1.0, 1.0, 0.0)).abs() < 1e-6);
        assert!((angle_diff(deg(350.0), deg(10.0)) - deg(20.0)).abs() < 1e-5);
    }
}
