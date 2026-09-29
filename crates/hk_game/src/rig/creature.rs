#![allow(dead_code)] // wired up by models/enemies.rs
//! Pose maths for the enemies and the training dummy: pure functions from the
//! simulation's state (the enemy `Brain`, its velocity) and a clock to joint
//! transforms, so every tell is unit-tested without a renderer.
//!
//! The colour language is unchanged (amber windup, red attack, blue recover,
//! white stagger), but each state now also has its own silhouette, so a tell
//! never depends on colour alone: a Husk rears back before it lunges, a Wisp
//! squeezes small before it dives, a Shieldbearer draws its shield in before
//! it bashes, a Spitter's belly swells before it spits.
//!
//! Model space: feet at the origin, +X forward, +Y up. A positive joint
//! rotation swings something that hangs down toward +X (forward); a positive
//! `lean` tips the whole body forward.

use hk_sim::enemy::EnemyState;

use super::pose::{damp, ease_in_out, ease_out_cubic, lerp, JointXf};

/// Joints per creature (unused slots stay at rest).
pub const MAX_JOINTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Species {
    Husk,
    Wisp,
    Shieldbearer,
    Spitter,
    Dummy,
}

impl From<hk_sim::enemy::EnemyKind> for Species {
    fn from(k: hk_sim::enemy::EnemyKind) -> Self {
        use hk_sim::enemy::EnemyKind as K;
        match k {
            K::Husk => Species::Husk,
            K::Wisp => Species::Wisp,
            K::Shieldbearer => Species::Shieldbearer,
            K::Spitter => Species::Spitter,
        }
    }
}

pub mod husk {
    pub const BODY: usize = 0;
    pub const HEAD: usize = 1;
    pub const ARM_FRONT: usize = 2;
    pub const ARM_BACK: usize = 3;
    pub const LEG_FRONT: usize = 4;
    pub const LEG_BACK: usize = 5;
}

pub mod wisp {
    pub const ORB: usize = 0;
    pub const CORE: usize = 1;
    /// Five tendrils: `TENDRIL + 0..5`.
    pub const TENDRIL: usize = 2;
    pub const TENDRILS: usize = 5;
}

pub mod shield {
    pub const BODY: usize = 0;
    pub const HEAD: usize = 1;
    pub const SHIELD: usize = 2;
    pub const WEAK: usize = 3;
    pub const LEG_FRONT: usize = 4;
    pub const LEG_BACK: usize = 5;
    /// How far the shield stands from the body's centre, on the guarded side.
    pub const SHIELD_X: f32 = 0.62;
    /// And the glowing weak spot, on the other side.
    pub const WEAK_X: f32 = 0.50;
}

pub mod spitter {
    pub const BODY: usize = 0;
    pub const MAW: usize = 1;
    pub const BELLY: usize = 2;
    pub const LEG_FRONT: usize = 3;
    pub const LEG_BACK: usize = 4;
}

pub mod dummy {
    pub const POST: usize = 0;
    pub const HEAD: usize = 1;
    pub const ARMS: usize = 2;
}

/// What a creature is doing this frame.
#[derive(Clone, Copy, Debug)]
pub struct CreatureIn {
    pub species: Species,
    pub state: EnemyState,
    /// Sim ticks spent in the state (fractional, for smoothness).
    pub t: f32,
    /// Planned length of the state in ticks; 0 when open-ended.
    pub len: f32,
    /// Seconds, always running (breathing, tendril sway).
    pub clock: f32,
    /// Walk-cycle phase in radians.
    pub walk: f32,
    /// Speed along the facing direction (negative when backing away).
    pub vx: f32,
    pub vy: f32,
    /// Aim angle in model space (0 forward, 90 degrees up).
    pub aim: f32,
    /// Where a Shieldbearer's shield is: +1 in front, -1 behind.
    pub guard: f32,
    /// 1 right after being hit, fading to 0.
    pub hit: f32,
    /// The dummy's sway angle.
    pub sway: f32,
}

impl Default for CreatureIn {
    fn default() -> Self {
        Self {
            species: Species::Husk,
            state: EnemyState::Idle,
            t: 0.0,
            len: 0.0,
            clock: 0.0,
            walk: 0.0,
            vx: 0.0,
            vy: 0.0,
            aim: 0.0,
            guard: 1.0,
            hit: 0.0,
            sway: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CreaturePose {
    pub joints: [JointXf; MAX_JOINTS],
    /// Tip forward about the feet (radians, positive = forward).
    pub lean: f32,
    /// Vertical drop of the whole body (crouch, collapse).
    pub drop: f32,
    /// Squash and stretch about the feet.
    pub squash: [f32; 2],
}

impl CreaturePose {
    fn rest() -> Self {
        Self {
            joints: [JointXf::REST; MAX_JOINTS],
            lean: 0.0,
            drop: 0.0,
            squash: [1.0, 1.0],
        }
    }

    /// How different two poses look: used to check that every tell has a
    /// silhouette of its own, not just a colour.
    pub fn distance(&self, other: &CreaturePose) -> f32 {
        let mut d = (self.lean - other.lean).abs() * 2.0
            + (self.drop - other.drop).abs() * 4.0
            + (self.squash[0] - other.squash[0]).abs() * 3.0
            + (self.squash[1] - other.squash[1]).abs() * 3.0;
        for (p, q) in self.joints.iter().zip(&other.joints) {
            d += (p.rot - q.rot).abs() * 0.6;
            d += ((p.pos[0] - q.pos[0]).abs() + (p.pos[1] - q.pos[1]).abs()) * 2.0;
            d += (p.scale[0] - q.scale[0]).abs() + (p.scale[1] - q.scale[1]).abs();
        }
        d
    }
}

fn progress(i: &CreatureIn) -> f32 {
    if i.len <= 0.0 {
        1.0
    } else {
        (i.t / i.len).clamp(0.0, 1.0)
    }
}

pub fn creature_pose(i: &CreatureIn) -> CreaturePose {
    let mut p = match i.species {
        Species::Husk => husk_pose(i),
        Species::Wisp => wisp_pose(i),
        Species::Shieldbearer => shield_pose(i),
        Species::Spitter => spitter_pose(i),
        Species::Dummy => dummy_pose(i),
    };
    // A fresh hit always kicks the body back a little, whatever it is doing.
    if i.species != Species::Dummy && i.species != Species::Wisp {
        p.lean -= 0.22 * i.hit;
        p.squash[0] += 0.06 * i.hit;
        p.squash[1] -= 0.06 * i.hit;
    }
    p
}

// -------------------------------------------------------------------- husk --

fn husk_pose(i: &CreatureIn) -> CreaturePose {
    use husk::*;
    let mut p = CreaturePose::rest();
    let (c, pr) = (i.clock, progress(i));
    let breathe = (c * 2.2).sin();
    p.joints[BODY] = JointXf::at(0.0, 0.012 * breathe);
    p.joints[HEAD] = JointXf::rot(0.05 * (c * 1.3).sin());
    p.joints[ARM_FRONT] = JointXf::rot(0.12 + 0.04 * breathe);
    p.joints[ARM_BACK] = JointXf::rot(0.05 - 0.04 * breathe);

    let amp = (i.vx.abs() / 4.5).clamp(0.3, 1.0);
    let walking = i.vx.abs() > 0.3;
    let s = i.walk.sin();
    match i.state {
        EnemyState::Idle | EnemyState::Chase => {
            let chasing = i.state == EnemyState::Chase;
            if walking {
                p.joints[LEG_FRONT] = JointXf::rot(0.7 * amp * s);
                p.joints[LEG_BACK] = JointXf::rot(-0.7 * amp * s);
                p.joints[ARM_FRONT] =
                    JointXf::rot(if chasing { 0.7 } else { 0.12 } - 0.5 * amp * s);
                p.joints[ARM_BACK] = JointXf::rot(if chasing { 0.5 } else { 0.05 } + 0.5 * amp * s);
                p.joints[BODY] = JointXf::at(0.0, 0.05 * (i.walk * 2.0).cos().abs());
            }
            p.lean = if chasing { 0.24 } else { 0.10 };
            if chasing {
                p.joints[HEAD] = JointXf::rot(-0.18);
            }
        }
        EnemyState::Notice => {
            // Rears up and throws its arms wide: "!".
            let e = ease_out_cubic(pr);
            p.lean = -0.30 * e;
            p.joints[HEAD] = JointXf::rot(0.5 * e);
            p.joints[ARM_FRONT] = JointXf::rot(lerp(0.12, 2.3, e));
            p.joints[ARM_BACK] = JointXf::rot(lerp(0.05, 1.9, e));
            p.squash = [1.0 - 0.04 * e, 1.0 + 0.05 * e];
        }
        EnemyState::Windup => {
            // Coils: crouches, leans back, draws both claws behind it.
            let e = ease_in_out(pr);
            p.lean = -0.50 * e;
            p.drop = -0.13 * e;
            p.joints[HEAD] = JointXf::rot(0.3 * e);
            p.joints[ARM_FRONT] = JointXf::rot(lerp(0.12, -1.7, e));
            p.joints[ARM_BACK] = JointXf::rot(lerp(0.05, -1.3, e));
            p.joints[LEG_FRONT] = JointXf::rot(0.5 * e);
            p.joints[LEG_BACK] = JointXf::rot(-0.4 * e);
            p.joints[BODY] = JointXf::at((i.clock * 80.0).sin() * 0.012 * e, 0.0);
            p.squash = [1.0 + 0.08 * e, 1.0 - 0.10 * e];
        }
        EnemyState::Attack => {
            // The lunge: stretched flat out, claws first.
            p.lean = 0.60;
            p.drop = -0.06;
            p.joints[HEAD] = JointXf::rot(-0.15);
            p.joints[ARM_FRONT] = JointXf::rot(1.5);
            p.joints[ARM_BACK] = JointXf::rot(1.2);
            p.joints[LEG_FRONT] = JointXf::rot(0.9 * s);
            p.joints[LEG_BACK] = JointXf::rot(-0.9 * s);
            p.squash = [0.93, 1.07];
        }
        EnemyState::Recover => {
            let e = 1.0 - ease_out_cubic(pr);
            p.lean = 0.35 * e;
            p.drop = -0.10 * e;
            p.joints[HEAD] = JointXf::rot(-0.5 * e);
            p.joints[ARM_FRONT] = JointXf::rot(lerp(0.12, 0.3, e));
            p.squash = [1.0 + 0.04 * e, 1.0 - 0.04 * e];
        }
        EnemyState::Stagger => {
            p.lean = -0.40;
            p.joints[HEAD] = JointXf::rot(-0.5);
            p.joints[ARM_FRONT] = JointXf::rot(-1.1);
            p.joints[ARM_BACK] = JointXf::rot(1.2);
            p.squash = [1.06, 0.94];
        }
    }
    p
}

// -------------------------------------------------------------------- wisp --

fn wisp_pose(i: &CreatureIn) -> CreaturePose {
    use wisp::*;
    let mut p = CreaturePose::rest();
    let (c, pr) = (i.clock, progress(i));
    let drag = (-i.vx * 0.06).clamp(-0.6, 0.6);
    let sway = |k: usize, speed: f32, amount: f32| {
        amount * (c * speed + k as f32 * 1.3).sin() + drag * (0.5 + 0.12 * k as f32)
    };
    for k in 0..TENDRILS {
        p.joints[TENDRIL + k] = JointXf::rot(sway(k, 2.6, 0.30));
    }
    p.joints[CORE] = JointXf {
        scale: [1.0 + 0.12 * (c * 4.0).sin(); 3],
        ..JointXf::REST
    };
    match i.state {
        EnemyState::Idle => {}
        EnemyState::Chase => {
            p.joints[ORB] = JointXf::rot(-0.18 * i.vx.signum() * (i.vx.abs() / 3.5).min(1.0));
            for k in 0..TENDRILS {
                p.joints[TENDRIL + k] = JointXf::rot(sway(k, 3.4, 0.22) - 0.45);
            }
        }
        EnemyState::Notice => {
            // The orb pops bright and swells: "!".
            let e = ease_out_cubic(pr);
            let s = 1.0 + 0.25 * (1.0 - e) * (pr * std::f32::consts::PI).sin().max(0.4);
            p.joints[ORB].scale = [s; 3];
            p.joints[CORE].scale = [1.0 + 0.7 * e; 3];
        }
        EnemyState::Windup => {
            // Squeezes small and tight, tendrils lifted, trembling.
            let e = ease_in_out(pr);
            let shake = (c * 70.0).sin() * 0.02 * e;
            p.joints[ORB] = JointXf {
                pos: [shake, (c * 63.0).cos() * 0.02 * e, 0.0],
                rot: 0.0,
                scale: [1.0 - 0.22 * e; 3],
            };
            p.joints[CORE].scale = [1.0 + 0.9 * e; 3];
            for k in 0..TENDRILS {
                let side = k as f32 - 2.0;
                p.joints[TENDRIL + k] = JointXf::rot(side * 0.32 * e + sway(k, 9.0, 0.10 * e));
            }
        }
        EnemyState::Attack => {
            // The dive: a comet, tendrils streaming straight back.
            p.joints[ORB] = JointXf {
                pos: [0.0; 3],
                rot: i.aim,
                scale: [1.40, 0.72, 0.72],
            };
            p.joints[CORE].scale = [1.5; 3];
            for k in 0..TENDRILS {
                p.joints[TENDRIL + k] = JointXf::rot(-1.5 + 0.10 * (k as f32 - 2.0));
            }
        }
        EnemyState::Recover => {
            // Dazed and sagging.
            let e = 1.0 - ease_out_cubic(pr);
            p.joints[ORB].scale = [1.0 + 0.06 * e, 1.0 - 0.10 * e, 1.0];
            p.joints[CORE].scale = [0.7; 3];
            for k in 0..TENDRILS {
                p.joints[TENDRIL + k] =
                    JointXf::rot(sway(k, 1.6, 0.16) + 0.25 * (k as f32 - 2.0) * e);
            }
        }
        EnemyState::Stagger => {
            p.joints[ORB] = JointXf {
                pos: [0.0; 3],
                rot: 0.45 * (c * 40.0).sin(),
                scale: [0.9, 1.1, 1.0],
            };
            for k in 0..TENDRILS {
                p.joints[TENDRIL + k] = JointXf::rot(1.0 * (k as f32 - 2.0) * 0.5);
            }
        }
    }
    // The wisp is a floating orb: a hit shoves it, it does not lean.
    p.joints[ORB].scale[0] *= 1.0 - 0.10 * i.hit;
    p.joints[ORB].scale[1] *= 1.0 + 0.10 * i.hit;
    p
}

// ------------------------------------------------------------- shieldbearer --

fn shield_pose(i: &CreatureIn) -> CreaturePose {
    use shield::*;
    let mut p = CreaturePose::rest();
    let (c, pr) = (i.clock, progress(i));
    let g = i.guard.clamp(-1.0, 1.0);
    let breathe = (c * 1.6).sin();
    // The shield stands on whichever side is guarded; the weak spot glows on
    // the other. Both joints rest at the body's centre and are placed here.
    let shield_at = |dx: f32, dy: f32, rot: f32| JointXf {
        pos: [SHIELD_X * g + dx, dy + 0.012 * breathe, 0.0],
        rot,
        scale: [1.0; 3],
    };
    p.joints[SHIELD] = shield_at(0.0, 0.0, 0.0);
    p.joints[WEAK] = JointXf::at(-WEAK_X * g, 0.0);
    p.joints[HEAD] = JointXf::rot(0.03 * (c * 1.1).sin());
    p.joints[BODY] = JointXf::at(0.0, 0.010 * breathe);

    let amp = (i.vx.abs() / 2.5).clamp(0.3, 1.0);
    let walking = i.vx.abs() > 0.3;
    let s = i.walk.sin();
    match i.state {
        EnemyState::Idle | EnemyState::Chase => {
            if walking {
                p.joints[LEG_FRONT] = JointXf::rot(0.5 * amp * s);
                p.joints[LEG_BACK] = JointXf::rot(-0.5 * amp * s);
                // Heavy stomps: the body sinks on every step.
                p.joints[BODY] = JointXf::at(0.0, -0.05 * (i.walk * 2.0).cos().abs());
                p.joints[SHIELD] = shield_at(0.0, 0.03 * (i.walk * 2.0).sin(), 0.0);
            }
            p.lean = if i.state == EnemyState::Chase {
                0.10
            } else {
                0.03
            };
        }
        EnemyState::Notice => {
            let e = ease_out_cubic(pr);
            p.joints[SHIELD] = shield_at(0.0, 0.10 * e, 0.0);
            p.squash = [1.0 - 0.03 * e, 1.0 + 0.05 * e];
            p.joints[HEAD] = JointXf::rot(0.25 * e);
        }
        EnemyState::Windup => {
            // Draws the shield in and back, hunkering behind it.
            let e = ease_in_out(pr);
            p.joints[SHIELD] = shield_at(-0.34 * e, 0.0, 0.28 * e);
            p.lean = -0.22 * e;
            p.drop = -0.07 * e;
            p.joints[LEG_FRONT] = JointXf::rot(0.4 * e);
            p.joints[LEG_BACK] = JointXf::rot(-0.3 * e);
            p.joints[BODY] = JointXf::at((c * 70.0).sin() * 0.010 * e, 0.0);
        }
        EnemyState::Attack => {
            // The bash: shield first, body behind it.
            p.joints[SHIELD] = shield_at(0.42, 0.0, -0.10);
            p.lean = 0.34;
            p.drop = -0.04;
            p.joints[LEG_FRONT] = JointXf::rot(0.6 * s);
            p.joints[LEG_BACK] = JointXf::rot(-0.6 * s);
            p.squash = [0.94, 1.05];
        }
        EnemyState::Recover => {
            // Sagging and open: the back weak spot is the target.
            let e = 1.0 - ease_out_cubic(pr);
            p.joints[SHIELD] = shield_at(0.10 * e, -0.16 * e, -0.22 * e);
            p.lean = 0.20 * e;
            p.drop = -0.06 * e;
            p.joints[HEAD] = JointXf::rot(-0.3 * e);
        }
        EnemyState::Stagger => {
            p.joints[SHIELD] = shield_at(-0.14, -0.05, 0.15);
            p.lean = -0.25;
            p.squash = [1.04, 0.96];
        }
    }
    p
}

// ------------------------------------------------------------------ spitter --

fn spitter_pose(i: &CreatureIn) -> CreaturePose {
    use spitter::*;
    let mut p = CreaturePose::rest();
    let (c, pr) = (i.clock, progress(i));
    let breathe = (c * 2.0).sin();
    let aim = i.aim.clamp(-1.2, 1.2);
    p.joints[BODY] = JointXf {
        pos: [0.0, 0.0, 0.0],
        rot: 0.0,
        scale: [1.0, 1.0 + 0.03 * breathe, 1.0],
    };
    p.joints[MAW] = JointXf::rot(0.12 + 0.10 * (c * 1.4).sin());
    p.joints[BELLY] = JointXf {
        scale: [1.0 + 0.05 * breathe; 3],
        ..JointXf::REST
    };
    let amp = (i.vx.abs() / 3.5).clamp(0.3, 1.0);
    let s = i.walk.sin();
    if i.vx.abs() > 0.3 {
        p.joints[LEG_FRONT] = JointXf::rot(0.6 * amp * s);
        p.joints[LEG_BACK] = JointXf::rot(-0.6 * amp * s);
        p.joints[BODY].pos[1] = 0.03 * (i.walk * 2.0).cos().abs();
    }
    match i.state {
        EnemyState::Idle => {}
        EnemyState::Chase => {
            p.joints[MAW] = JointXf::rot(lerp(0.12, aim, 0.5));
        }
        EnemyState::Notice => {
            let e = ease_out_cubic(pr);
            p.joints[MAW] = JointXf::rot(lerp(0.12, 0.9, e));
            p.squash = [1.0 - 0.05 * e, 1.0 + 0.08 * e];
        }
        EnemyState::Windup => {
            // The belly swells, the maw locks on: something is coming out.
            let e = ease_in_out(pr);
            p.joints[MAW] = JointXf::rot(lerp(0.12, aim, ease_out_cubic(pr * 2.0)));
            p.joints[BELLY].scale = [1.0 + 0.75 * e; 3];
            p.squash = [1.0 + 0.14 * e, 1.0 - 0.10 * e];
            p.joints[BODY].pos[0] = (c * 75.0).sin() * 0.012 * e;
            p.lean = -0.10 * e;
        }
        EnemyState::Attack => {
            // Fired: kicked back, belly still full.
            p.joints[MAW] = JointXf::rot(aim).with_pos(-0.08, 0.0);
            p.joints[BELLY].scale = [1.5; 3];
            p.lean = -0.30;
            p.squash = [0.94, 1.06];
        }
        EnemyState::Recover => {
            // Spent: the belly is slack, the maw droops.
            let e = 1.0 - ease_out_cubic(pr);
            p.joints[MAW] = JointXf::rot(lerp(-0.55, 0.12, 1.0 - e));
            p.joints[BELLY].scale = [0.70 + 0.3 * (1.0 - e); 3];
            p.drop = -0.05 * e;
            p.squash = [1.05, 0.95];
        }
        EnemyState::Stagger => {
            p.joints[MAW] = JointXf::rot(-0.6);
            p.lean = -0.25;
            p.squash = [1.06, 0.94];
        }
    }
    p
}

// -------------------------------------------------------------------- dummy --

fn dummy_pose(i: &CreatureIn) -> CreaturePose {
    use dummy::*;
    let mut p = CreaturePose::rest();
    // The post pivots about its base; the head lags and the straw arms
    // flap, so a hit rings through the whole thing.
    p.joints[POST] = JointXf::rot(i.sway);
    p.joints[HEAD] = JointXf::rot(-i.sway * 0.7);
    p.joints[ARMS] = JointXf::rot(-i.sway * 0.35);
    p
}

/// Advances the dummy's sway spring (a hit kicks `v`; it rings down).
pub fn step_sway(sway: &mut super::pose::Spring, dt: f32) {
    sway.step(0.0, dt, 90.0, 5.5);
}

/// Eases the shield toward the side the sim says is guarded.
pub fn ease_guard(current: f32, target: f32, dt: f32) -> f32 {
    damp(current, target, 14.0, dt)
}

// ---------------------------------------------------------------- the tells --

/// The glow colour that tells you what an enemy is about to do (linear RGB,
/// HDR). `tick` is the global sim tick (the windup flashes on it).
///
/// Idle and Chase glow softly in the species' own colour; the rest are the
/// game's tell language and never change: yellow "!", flashing amber windup,
/// red attack, blue recover (the punish window), white stagger.
pub fn tell_glow(species: Species, state: EnemyState, tick: u64) -> [f32; 3] {
    let base = species_glow(species);
    match state {
        EnemyState::Idle => [base[0] * 0.9, base[1] * 0.9, base[2] * 0.9],
        EnemyState::Chase => [base[0] * 1.8, base[1] * 1.8, base[2] * 1.8],
        EnemyState::Notice => [2.0, 1.8, 0.2],
        EnemyState::Windup if (tick / 4) & 1 == 0 => [4.0, 2.4, 0.4],
        EnemyState::Windup => [1.6, 0.8, 0.1],
        EnemyState::Attack => [4.0, 0.3, 0.3],
        EnemyState::Recover => [0.1, 0.35, 1.4],
        EnemyState::Stagger => [2.0, 2.0, 2.0],
    }
}

/// The species' resting glow (cracks, eyes, core), linear RGB.
pub fn species_glow(species: Species) -> [f32; 3] {
    match species {
        Species::Husk => [0.55, 0.16, 0.05],
        Species::Wisp => [0.30, 0.16, 0.55],
        Species::Shieldbearer => [0.08, 0.30, 0.34],
        Species::Spitter => [0.14, 0.34, 0.06],
        Species::Dummy => [0.0, 0.0, 0.0],
    }
}

/// How strongly the tell colour washes the creature's whole body (the parts
/// that are not the glow), so the state still reads when the small glowing
/// parts are a few pixels wide.
pub const BODY_WASH: f32 = 0.15;

#[cfg(test)]
mod tests {
    use super::*;
    use EnemyState::*;

    const SPECIES: [Species; 4] = [
        Species::Husk,
        Species::Wisp,
        Species::Shieldbearer,
        Species::Spitter,
    ];
    const STATES: [EnemyState; 8] = [Idle, Notice, Chase, Windup, Attack, Recover, Stagger, Idle];

    fn at(species: Species, state: EnemyState, t: f32, len: f32) -> CreatureIn {
        CreatureIn {
            species,
            state,
            t,
            len,
            clock: 0.7,
            aim: 0.4,
            ..Default::default()
        }
    }

    fn finite(p: &CreaturePose) -> bool {
        p.lean.is_finite()
            && p.drop.is_finite()
            && p.squash.iter().all(|v| v.is_finite() && *v > 0.2)
            && p.joints.iter().all(|j| {
                j.pos.iter().all(|v| v.is_finite())
                    && j.rot.is_finite()
                    && j.scale.iter().all(|v| v.is_finite() && *v > 0.05)
            })
    }

    #[test]
    fn every_pose_is_finite_over_the_whole_state_machine() {
        for sp in SPECIES.into_iter().chain([Species::Dummy]) {
            for st in STATES {
                for k in 0..=20 {
                    let mut i = at(sp, st, k as f32 * 3.0, 40.0);
                    i.walk = k as f32;
                    i.vx = if k % 2 == 0 { 4.0 } else { -3.0 };
                    i.hit = k as f32 / 20.0;
                    i.sway = 0.2;
                    assert!(finite(&creature_pose(&i)), "{sp:?} {st:?} at {k}");
                }
            }
        }
    }

    #[test]
    fn the_windup_is_a_shape_you_can_see_not_just_a_colour() {
        // Standing still vs the end of the windup must differ clearly for every
        // enemy, so the tell survives being colour-blind or far away.
        for sp in SPECIES {
            let idle = creature_pose(&at(sp, Idle, 0.0, 0.0));
            let wind = creature_pose(&at(sp, Windup, 40.0, 40.0));
            let d = wind.distance(&idle);
            assert!(d > 0.6, "{sp:?}: windup only {d} away from idle");
        }
    }

    #[test]
    fn windup_grows_toward_the_attack() {
        for sp in SPECIES {
            let idle = creature_pose(&at(sp, Idle, 0.0, 0.0));
            let d = |t: f32| creature_pose(&at(sp, Windup, t, 40.0)).distance(&idle);
            assert!(d(0.0) < d(20.0) && d(20.0) < d(40.0), "{sp:?} monotone");
        }
    }

    #[test]
    fn each_phase_has_its_own_silhouette() {
        // Windup, attack and recover must not look alike.
        for sp in SPECIES {
            let w = creature_pose(&at(sp, Windup, 40.0, 40.0));
            let a = creature_pose(&at(sp, Attack, 5.0, 20.0));
            let r = creature_pose(&at(sp, Recover, 0.0, 40.0));
            assert!(w.distance(&a) > 0.5, "{sp:?} windup vs attack");
            assert!(a.distance(&r) > 0.3, "{sp:?} attack vs recover");
        }
    }

    #[test]
    fn the_husk_rears_back_then_lunges_forward() {
        let w = creature_pose(&at(Species::Husk, Windup, 40.0, 40.0));
        let a = creature_pose(&at(Species::Husk, Attack, 5.0, 20.0));
        assert!(w.lean < -0.3, "windup leans back, got {}", w.lean);
        assert!(a.lean > 0.4, "lunge leans forward, got {}", a.lean);
        assert!(w.drop < -0.08, "windup crouches");
        assert!(a.squash[0] < 1.0 && a.squash[1] > 1.0, "lunge stretches");
    }

    #[test]
    fn the_wisp_dives_along_its_aim_and_squeezes_first() {
        let mut i = at(Species::Wisp, Attack, 5.0, 24.0);
        i.aim = -0.9;
        let p = creature_pose(&i);
        assert_eq!(p.joints[wisp::ORB].rot, -0.9, "points down the dive");
        assert!(p.joints[wisp::ORB].scale[0] > 1.3, "stretched along it");
        let w = creature_pose(&at(Species::Wisp, Windup, 24.0, 24.0));
        assert!(w.joints[wisp::ORB].scale[0] < 0.85, "squeezed small");
        assert!(w.joints[wisp::CORE].scale[0] > 1.6, "core blazing");
    }

    #[test]
    fn the_shield_stays_on_the_guarded_side_and_the_weak_spot_opposite() {
        use shield::*;
        for g in [1.0f32, -1.0] {
            let mut i = at(Species::Shieldbearer, Idle, 0.0, 0.0);
            i.guard = g;
            let p = creature_pose(&i);
            assert!((p.joints[SHIELD].pos[0] - SHIELD_X * g).abs() < 1e-4);
            assert!((p.joints[WEAK].pos[0] + WEAK_X * g).abs() < 1e-4);
        }
        let mut bash = at(Species::Shieldbearer, Attack, 3.0, 18.0);
        let mut wind = at(Species::Shieldbearer, Windup, 30.0, 30.0);
        bash.guard = 1.0;
        wind.guard = 1.0;
        let (b, w) = (creature_pose(&bash), creature_pose(&wind));
        assert!(
            b.joints[SHIELD].pos[0] > SHIELD_X + 0.3,
            "shield thrust out"
        );
        assert!(w.joints[SHIELD].pos[0] < SHIELD_X - 0.25, "shield drawn in");
    }

    #[test]
    fn the_spitter_swells_before_it_spits() {
        let idle = creature_pose(&at(Species::Spitter, Idle, 0.0, 0.0));
        let mid = creature_pose(&at(Species::Spitter, Windup, 30.0, 60.0));
        let end = creature_pose(&at(Species::Spitter, Windup, 60.0, 60.0));
        let b = |p: &CreaturePose| p.joints[spitter::BELLY].scale[0];
        assert!(b(&idle) < 1.1 && b(&mid) > b(&idle) && b(&end) > 1.5);
        // The maw follows the aim, within its neck's reach.
        let mut i = at(Species::Spitter, Windup, 60.0, 60.0);
        i.aim = 3.0;
        let far = creature_pose(&i);
        assert!(far.joints[spitter::MAW].rot <= 1.2 + 1e-4);
        i.aim = 0.5;
        assert!((creature_pose(&i).joints[spitter::MAW].rot - 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_walk_cycle_repeats() {
        for sp in [Species::Husk, Species::Shieldbearer, Species::Spitter] {
            let mut i = at(sp, Idle, 0.0, 0.0);
            i.vx = 3.0;
            i.walk = 1.1;
            let a = creature_pose(&i);
            i.walk = 1.1 + std::f32::consts::TAU;
            let b = creature_pose(&i);
            for (p, q) in a.joints.iter().zip(&b.joints) {
                assert!((p.rot - q.rot).abs() < 1e-3 && (p.pos[1] - q.pos[1]).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn a_hit_kicks_the_body_and_fades() {
        let calm = creature_pose(&at(Species::Husk, Idle, 0.0, 0.0));
        let mut i = at(Species::Husk, Idle, 0.0, 0.0);
        i.hit = 1.0;
        let hit = creature_pose(&i);
        assert!(hit.lean < calm.lean - 0.15);
    }

    #[test]
    fn the_dummy_sways_and_rings_down() {
        let mut i = at(Species::Dummy, Idle, 0.0, 0.0);
        i.sway = 0.3;
        let p = creature_pose(&i);
        assert!(p.joints[dummy::POST].rot > 0.29);
        assert!(p.joints[dummy::HEAD].rot < 0.0, "the head lags the post");
        let mut s = super::super::pose::Spring { x: 0.0, v: 6.0 };
        let mut peak: f32 = 0.0;
        for _ in 0..300 {
            step_sway(&mut s, 1.0 / 60.0);
            peak = peak.max(s.x.abs());
        }
        assert!(
            peak > 0.05 && peak < 0.6,
            "kicked, but not violently: {peak}"
        );
        assert!(s.x.abs() < 0.005 && s.v.abs() < 0.05, "and it settles");
    }

    #[test]
    fn the_guard_eases_but_arrives() {
        let mut g = 1.0;
        for _ in 0..120 {
            g = ease_guard(g, -1.0, 1.0 / 60.0);
        }
        assert!((g + 1.0).abs() < 0.01);
    }

    /// The tell colours are the game's language: pinned exactly (they were the
    /// `enemy_fx` table before the models existed) so a restyle cannot
    /// change what an amber or a red means.
    #[test]
    fn the_tell_colours_are_the_original_language() {
        let sp = Species::Husk;
        assert_eq!(tell_glow(sp, Notice, 0), [2.0, 1.8, 0.2]);
        assert_eq!(tell_glow(sp, Windup, 0), [4.0, 2.4, 0.4]);
        assert_eq!(tell_glow(sp, Windup, 4), [1.6, 0.8, 0.1]);
        assert_eq!(tell_glow(sp, Windup, 8), [4.0, 2.4, 0.4]);
        assert_eq!(tell_glow(sp, Attack, 0), [4.0, 0.3, 0.3]);
        assert_eq!(tell_glow(sp, Recover, 0), [0.1, 0.35, 1.4]);
        assert_eq!(tell_glow(sp, Stagger, 0), [2.0, 2.0, 2.0]);
        // Every species shares the same tells; only the resting glow differs.
        for other in SPECIES {
            for st in [Notice, Windup, Attack, Recover, Stagger] {
                assert_eq!(tell_glow(other, st, 5), tell_glow(sp, st, 5));
            }
        }
        // Resting glows are dimmer than any tell.
        for other in SPECIES {
            for st in [Idle, Chase] {
                let g = tell_glow(other, st, 0);
                assert!(g.iter().all(|v| *v < 1.2), "{other:?} {st:?} is subtle");
            }
        }
    }
}
