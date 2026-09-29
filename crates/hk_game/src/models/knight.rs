//! Nym, the deaf pilgrim: an oversized bone-white bell-helm with two swept
//! horns, an indigo cloak, a crimson three-piece cape that streams with motion,
//! boots, a hanging lantern, and the Needle, a slim sword that is always
//! visible (slung over the shoulder at rest) and swings with a bright
//! crescent trail timed to the real hitbox.
//!
//! Everything is authored in model space (feet at the origin, +X forward,
//! about 1.5 units tall to match the collision box) and posed by
//! `rig::pose::knight_pose`.

use bevy::prelude::*;
use hk_sim::combat::AttackDir;
use hk_sim::combat::{CombatState, Hit, HitKind, Invulnerable, SimFrozen, Soul, Team};
use hk_sim::components::{Aabb, Velocity};
use hk_sim::player::{Facing, Motor, Player, PlayerState};
use hk_sim::tuning::Tuning;
use hk_sim::SimTick;

use crate::rig::meshkit::{blade, cone, crescent, ellipsoid, lathe, limb, ribbon, tube, MeshData};
use crate::rig::pose::{
    angle_diff, deg, knight_joint as kj, knight_pose, lerp, run_cycle_rate, step_cape,
    swing_arc_deg, swing_pose, KnightIn, SwingDir, SwingTiming,
};
use crate::rig::{joint, part, posed, ModelRoot, Rest};

/// Where the shoulder (the sword's pivot) is in model space.
pub const SHOULDER: Vec3 = Vec3::new(0.08, 0.80, 0.08);
/// Radius of the slash crescent: the blade tip's distance from the shoulder.
pub const TRAIL_RADIUS: f32 = 2.3;
/// Ticks for the blade to settle back to rest after a swing.
pub const SETTLE_TICKS: u32 = 12;
const TRAIL_STEPS: usize = 8;

pub struct KnightPlugin;

impl Plugin for KnightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, build_knight_assets).add_systems(
            Update,
            (spawn_player_model, animate_knight)
                .chain()
                .after(crate::interp::RenderPrepSet),
        );
    }
}

// ------------------------------------------------------------------ assets --

pub struct KnightMats {
    pub bone: Handle<StandardMaterial>,
    pub cloak: Handle<StandardMaterial>,
    pub cape: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
    pub eye: Handle<StandardMaterial>,
    pub steel: Handle<StandardMaterial>,
    pub edge: Handle<StandardMaterial>,
    pub lantern: Handle<StandardMaterial>,
    pub trail: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub struct KnightAssets {
    pub mats: KnightMats,
    cloak: Handle<Mesh>,
    helm: Handle<Mesh>,
    visor: Handle<Mesh>,
    eyes: Handle<Mesh>,
    arm: Handle<Mesh>,
    hand: Handle<Mesh>,
    grip: Handle<Mesh>,
    guard: Handle<Mesh>,
    pommel: Handle<Mesh>,
    blade: Handle<Mesh>,
    edge: Handle<Mesh>,
    off_arm: Handle<Mesh>,
    leg: Handle<Mesh>,
    boot: Handle<Mesh>,
    cape: [Handle<Mesh>; 3],
    chain: Handle<Mesh>,
    cage: Handle<Mesh>,
    cage_cap: Handle<Mesh>,
    /// `[direction][step]`: the crescent revealed up to `step / TRAIL_STEPS`.
    trail: [[Handle<Mesh>; TRAIL_STEPS]; 3],
}

fn dir_index(d: SwingDir) -> usize {
    match d {
        SwingDir::Forward => 0,
        SwingDir::Up => 1,
        SwingDir::Down => 2,
    }
}

pub fn swing_dir(d: AttackDir) -> SwingDir {
    match d {
        AttackDir::Forward => SwingDir::Forward,
        AttackDir::Up => SwingDir::Up,
        AttackDir::Down => SwingDir::Down,
    }
}

fn mat(base: Color, rough: f32, metallic: f32, emissive: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: base,
        perceptual_roughness: rough,
        metallic,
        emissive,
        ..default()
    }
}

/// The knight's geometry, as plain data (also used by tests).
pub struct KnightMeshes {
    pub cloak: MeshData,
    pub helm: MeshData,
    pub visor: MeshData,
    pub eyes: MeshData,
    pub blade: MeshData,
    pub edge: MeshData,
    pub trail: Vec<Vec<MeshData>>,
}

pub fn knight_meshes() -> KnightMeshes {
    // Cloak: a hooded bell of cloth, flared at the hem, with a ragged edge and
    // a darker base. Local to the BODY joint (hip at the origin).
    let cloak = lathe(
        &[
            (0.34, -0.36),
            (0.36, -0.33),
            (0.31, -0.20),
            (0.24, -0.02),
            (0.20, 0.20),
            (0.22, 0.34),
            (0.14, 0.42),
            (0.0, 0.44),
        ],
        20,
    )
    .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.86)))
    .jitter(11, 0.012)
    .recolor(|p| {
        let k = ((p.y + 0.36) / 0.8).clamp(0.0, 1.0);
        let shade = 0.45 + 0.55 * k;
        [shade, shade, shade * 1.05, 1.0]
    });

    // Helm: an inverted bell, widest at the rim, with two swept-back horns.
    // Local to the HEAD joint (neck at the origin).
    let mut helm = lathe(
        &[
            (0.30, 0.0),
            (0.36, 0.06),
            (0.36, 0.22),
            (0.31, 0.38),
            (0.21, 0.52),
            (0.08, 0.60),
            (0.0, 0.62),
        ],
        24,
    )
    .transformed(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.92)));
    for side in [-1.0f32, 1.0] {
        let horn = tube(
            &[
                Vec3::new(0.02, 0.50, 0.11 * side),
                Vec3::new(-0.06, 0.64, 0.17 * side),
                Vec3::new(-0.20, 0.76, 0.20 * side),
                Vec3::new(-0.32, 0.79, 0.19 * side),
            ],
            |t| 0.038 * (1.0 - t * 0.85),
            8,
        );
        helm.merge(&horn);
    }
    // The dark visor and the two small eyes.
    let visor = ellipsoid(0.06, 0.13, 0.20, 8, 12)
        .transformed(Mat4::from_translation(Vec3::new(0.30, 0.26, 0.0)));
    let mut eyes = ellipsoid(0.035, 0.05, 0.05, 6, 8)
        .transformed(Mat4::from_translation(Vec3::new(0.355, 0.28, 0.075)));
    eyes.merge(
        &ellipsoid(0.035, 0.05, 0.05, 6, 8)
            .transformed(Mat4::from_translation(Vec3::new(0.355, 0.28, -0.075))),
    );

    // The Needle, along +X from the shoulder joint.
    let blade_mesh = blade(1.75, 0.13, 0.04, 0.82)
        .transformed(Mat4::from_translation(Vec3::new(0.52, 0.0, 0.0)));
    let edge = blade(1.66, 0.03, 0.058, 0.85)
        .transformed(Mat4::from_translation(Vec3::new(0.55, 0.0, 0.0)));

    // Trail crescents: for each direction, revealed in TRAIL_STEPS steps.
    let mut trail = Vec::new();
    for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
        let (coil, end) = swing_arc_deg(dir);
        let mut steps = Vec::new();
        for k in 1..=TRAIL_STEPS {
            let head = lerp(coil, end, k as f32 / TRAIL_STEPS as f32);
            steps.push(crescent(
                TRAIL_RADIUS,
                0.06,
                0.95,
                deg(coil),
                deg(head),
                6 + 2 * k,
            ));
        }
        trail.push(steps);
    }
    KnightMeshes {
        cloak,
        helm,
        visor,
        eyes,
        blade: blade_mesh,
        edge,
        trail,
    }
}

pub fn build_assets(
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) -> KnightAssets {
    let m = knight_meshes();
    let none = LinearRgba::BLACK;
    let mats = KnightMats {
        bone: mats.add(mat(
            Color::srgb(0.93, 0.92, 0.85),
            0.5,
            0.0,
            LinearRgba::rgb(0.05, 0.05, 0.05),
        )),
        cloak: mats.add(mat(
            Color::srgb(0.20, 0.18, 0.42),
            0.9,
            0.0,
            LinearRgba::rgb(0.01, 0.01, 0.04),
        )),
        cape: mats.add(StandardMaterial {
            cull_mode: None,
            ..mat(
                Color::srgb(0.70, 0.10, 0.16),
                0.85,
                0.0,
                LinearRgba::rgb(0.12, 0.01, 0.02),
            )
        }),
        dark: mats.add(mat(Color::srgb(0.06, 0.06, 0.10), 0.6, 0.0, none)),
        eye: mats.add(mat(
            Color::srgb(0.05, 0.05, 0.05),
            0.4,
            0.0,
            LinearRgba::rgb(3.2, 2.6, 0.9),
        )),
        steel: mats.add(mat(
            Color::srgb(0.84, 0.88, 0.95),
            0.28,
            0.75,
            LinearRgba::rgb(0.02, 0.03, 0.05),
        )),
        edge: mats.add(mat(
            Color::srgb(0.9, 0.95, 1.0),
            0.3,
            0.0,
            LinearRgba::rgb(0.4, 0.6, 0.9),
        )),
        lantern: mats.add(mat(
            Color::srgb(0.9, 0.6, 0.2),
            0.5,
            0.0,
            LinearRgba::rgb(3.0, 1.6, 0.4),
        )),
        trail: mats.add(StandardMaterial {
            base_color: Color::srgba(0.75, 0.93, 1.0, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        }),
    };
    let mut add = |d: MeshData| meshes.add(d.to_mesh());

    let cape_seg = |len: f32, w0: f32, w1: f32| {
        ribbon(
            &[
                Vec3::ZERO,
                Vec3::new(-len * 0.5, -0.01, 0.0),
                Vec3::new(-len, 0.0, 0.0),
            ],
            move |t| lerp(w0, w1, t),
            Vec3::Z,
        )
    };
    let trail_meshes: Vec<Vec<Handle<Mesh>>> = m
        .trail
        .into_iter()
        .map(|steps| steps.into_iter().map(&mut add).collect())
        .collect();
    let mut trail: [[Handle<Mesh>; TRAIL_STEPS]; 3] = Default::default();
    for (d, steps) in trail_meshes.into_iter().enumerate() {
        for (k, h) in steps.into_iter().enumerate() {
            trail[d][k] = h;
        }
    }
    KnightAssets {
        mats,
        cloak: add(m.cloak),
        helm: add(m.helm),
        visor: add(m.visor),
        eyes: add(m.eyes),
        arm: add(limb(Vec3::ZERO, Vec3::new(0.26, 0.0, 0.0), 0.055, 0.045, 8)),
        hand: add(ellipsoid(0.06, 0.06, 0.06, 6, 10)
            .transformed(Mat4::from_translation(Vec3::new(0.28, 0.0, 0.0)))),
        grip: add(limb(
            Vec3::new(0.30, 0.0, 0.0),
            Vec3::new(0.48, 0.0, 0.0),
            0.028,
            0.028,
            8,
        )),
        guard: add(ellipsoid(0.03, 0.17, 0.06, 6, 10)
            .transformed(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)))),
        pommel: add(ellipsoid(0.045, 0.045, 0.045, 6, 10)
            .transformed(Mat4::from_translation(Vec3::new(0.24, 0.0, 0.0)))),
        blade: add(m.blade),
        edge: add(m.edge),
        off_arm: add(limb(Vec3::ZERO, Vec3::new(0.30, 0.0, 0.0), 0.05, 0.04, 8)),
        leg: add(limb(Vec3::ZERO, Vec3::new(0.0, -0.34, 0.0), 0.07, 0.055, 8)),
        boot: add(ellipsoid(0.11, 0.06, 0.075, 6, 10)
            .transformed(Mat4::from_translation(Vec3::new(0.03, -0.38, 0.0)))),
        cape: [
            add(cape_seg(0.28, 0.32, 0.29)),
            add(cape_seg(0.26, 0.29, 0.24)),
            add(cape_seg(0.24, 0.24, 0.02)),
        ],
        chain: add(limb(
            Vec3::ZERO,
            Vec3::new(0.0, -0.10, 0.0),
            0.008,
            0.008,
            6,
        )),
        cage: add(ellipsoid(0.055, 0.075, 0.055, 6, 10)
            .transformed(Mat4::from_translation(Vec3::new(0.0, -0.17, 0.0)))),
        cage_cap: add(
            cone(0.05, 0.06, 8).transformed(Mat4::from_translation(Vec3::new(0.0, -0.11, 0.0)))
        ),
        trail,
    }
}

// --------------------------------------------------------------------- rig --

/// The entities of one knight.
#[derive(Component)]
pub struct KnightRig {
    pub model_root: Entity,
    pub squash: Entity,
    pub facing: Entity,
    pub lean: Entity,
    pub joints: [Entity; kj::COUNT],
    pub trail: Entity,
}

/// Per-knight animation state (things that accumulate over frames).
#[derive(Component)]
pub struct KnightAnim {
    pub clock: f32,
    pub run_phase: f32,
    pub cape: [crate::rig::pose::Spring; 3],
    pub yaw: f32,
    pub prev_grounded: bool,
    pub prev_vy: f32,
    pub land_timer: f32,
    /// The direction and facing of the swing in progress (or just finished).
    pub swing: Option<(SwingDir, i8)>,
    pub flash: f32,
    // Last values pushed to shared materials (to avoid touching them needlessly).
    last_glow: f32,
    last_tint: LinearRgba,
    last_trail_alpha: f32,
    last_trail: Option<(usize, usize)>,
}

impl KnightAssets {
    /// The cloak and helm meshes (for the dash afterimages).
    pub fn cloak_mesh(&self) -> Handle<Mesh> {
        self.cloak.clone()
    }
    pub fn helm_mesh(&self) -> Handle<Mesh> {
        self.helm.clone()
    }
}

impl Default for KnightAnim {
    fn default() -> Self {
        Self {
            clock: 0.0,
            run_phase: 0.0,
            cape: Default::default(),
            yaw: -0.30,
            prev_grounded: true,
            prev_vy: 0.0,
            land_timer: 0.0,
            swing: None,
            flash: 0.0,
            last_glow: -1.0,
            last_tint: LinearRgba::rgb(-1.0, 0.0, 0.0),
            last_trail_alpha: -1.0,
            last_trail: None,
        }
    }
}

/// Builds the knight's entity hierarchy under `anchor`.
pub fn spawn_knight(
    commands: &mut Commands,
    a: &KnightAssets,
    anchor: Entity,
    half_y: f32,
) -> KnightRig {
    let m = &a.mats;
    let model_root = commands
        .spawn((
            ModelRoot,
            Transform::from_xyz(0.0, -half_y, 0.0),
            Visibility::default(),
        ))
        .id();
    commands.entity(anchor).add_child(model_root);
    let squash = commands
        .spawn((Transform::default(), Visibility::default()))
        .id();
    commands.entity(model_root).add_child(squash);
    let facing = commands
        .spawn((Transform::default(), Visibility::default()))
        .id();
    commands.entity(squash).add_child(facing);
    let lean = commands
        .spawn((Transform::default(), Visibility::default()))
        .id();
    commands.entity(facing).add_child(lean);

    let t = |x: f32, y: f32, z: f32| Transform::from_xyz(x, y, z);
    let mut joints = [Entity::PLACEHOLDER; kj::COUNT];

    // Body and everything hung from it.
    let body = joint(commands, lean, t(0.0, 0.45, 0.0));
    joints[kj::BODY] = body;
    part(
        commands,
        body,
        a.cloak.clone(),
        m.cloak.clone(),
        Transform::IDENTITY,
    );

    let head = joint(commands, body, t(0.0, 0.47, 0.0));
    joints[kj::HEAD] = head;
    part(
        commands,
        head,
        a.helm.clone(),
        m.bone.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        head,
        a.visor.clone(),
        m.dark.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        head,
        a.eyes.clone(),
        m.eye.clone(),
        Transform::IDENTITY,
    );

    let sword = joint(commands, body, t(0.08, 0.35, 0.05));
    joints[kj::SWORD_ARM] = sword;
    part(
        commands,
        sword,
        a.arm.clone(),
        m.cloak.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.hand.clone(),
        m.dark.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.grip.clone(),
        m.dark.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.guard.clone(),
        m.steel.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.pommel.clone(),
        m.bone.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.blade.clone(),
        m.steel.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        sword,
        a.edge.clone(),
        m.edge.clone(),
        Transform::IDENTITY,
    );

    let off = joint(
        commands,
        body,
        t(-0.04, 0.35, -0.05).with_rotation(Quat::from_rotation_z(deg(-80.0))),
    );
    joints[kj::OFF_ARM] = off;
    part(
        commands,
        off,
        a.off_arm.clone(),
        m.cloak.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        off,
        a.hand.clone(),
        m.dark.clone(),
        Transform::from_xyz(0.02, 0.0, 0.0),
    );

    let cape1 = joint(commands, body, t(-0.16, 0.40, -0.03));
    let cape2 = joint(commands, cape1, t(-0.28, 0.0, 0.0));
    let cape3 = joint(commands, cape2, t(-0.26, 0.0, 0.0));
    joints[kj::CAPE_1] = cape1;
    joints[kj::CAPE_2] = cape2;
    joints[kj::CAPE_3] = cape3;
    for (j, mesh) in [cape1, cape2, cape3].into_iter().zip(a.cape.iter()) {
        part(
            commands,
            j,
            mesh.clone(),
            m.cape.clone(),
            Transform::IDENTITY,
        );
    }

    let lantern = joint(commands, body, t(-0.12, 0.02, 0.13));
    joints[kj::LANTERN] = lantern;
    part(
        commands,
        lantern,
        a.chain.clone(),
        m.dark.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        lantern,
        a.cage_cap.clone(),
        m.dark.clone(),
        Transform::IDENTITY,
    );
    part(
        commands,
        lantern,
        a.cage.clone(),
        m.lantern.clone(),
        Transform::IDENTITY,
    );

    // Legs hang from the hip (children of the lean node, so the body can bob).
    for (idx, x, z) in [(kj::LEG_FRONT, 0.08, 0.07), (kj::LEG_BACK, -0.08, -0.07)] {
        let leg = joint(commands, lean, t(x, 0.42, z));
        joints[idx] = leg;
        part(
            commands,
            leg,
            a.leg.clone(),
            m.cloak.clone(),
            Transform::IDENTITY,
        );
        part(
            commands,
            leg,
            a.boot.clone(),
            m.dark.clone(),
            Transform::IDENTITY,
        );
    }

    // The slash trail, centred on the shoulder.
    let trail = commands
        .spawn((
            Mesh3d(a.trail[0][0].clone()),
            MeshMaterial3d(m.trail.clone()),
            Transform::from_translation(SHOULDER + Vec3::new(0.0, 0.0, 0.05)),
            Visibility::Hidden,
        ))
        .id();
    commands.entity(lean).add_child(trail);

    KnightRig {
        model_root,
        squash,
        facing,
        lean,
        joints,
        trail,
    }
}

/// Swing timing from the combat tuning.
pub fn swing_timing(tuning: &Tuning) -> SwingTiming {
    let c = &tuning.combat;
    SwingTiming {
        startup: c.nail_startup_ticks(),
        active: c.nail_active_ticks(),
        settle: SETTLE_TICKS,
    }
}

// ----------------------------------------------------------------- systems --

pub fn build_knight_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let a = build_assets(&mut meshes, &mut mats);
    commands.insert_resource(a);
}

/// Gives every new player its model, and the lantern that lights the way.
pub fn spawn_player_model(
    mut commands: Commands,
    assets: Res<KnightAssets>,
    q: Query<(Entity, &Aabb, &hk_sim::components::SimPos), Added<Player>>,
) {
    for (e, aabb, pos) in &q {
        // Transform and Visibility first, so children never see a bare parent.
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Visibility::default(),
            crate::interp::Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
        commands.entity(e).with_children(|p| {
            p.spawn((
                PointLight {
                    intensity: 900_000.0,
                    range: 24.0,
                    color: Color::srgb(1.0, 0.85, 0.6),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.6, 2.5),
            ));
        });
        let rig = spawn_knight(&mut commands, &assets, e, aabb.half.y);
        commands.entity(e).insert((rig, KnightAnim::default()));
    }
}

/// Everything the pose needs, gathered from the sim, plus the swing timing.
struct Sampled {
    input: KnightIn,
    facing: i8,
}

#[allow(clippy::too_many_arguments)]
pub fn animate_knight(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    frozen: Res<SimFrozen>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    assets: Res<KnightAssets>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut hits: MessageReader<Hit>,
    mut players: Query<
        (
            &Motor,
            &PlayerState,
            &Velocity,
            &Facing,
            &CombatState,
            &Soul,
            Has<Invulnerable>,
            &KnightRig,
            &mut KnightAnim,
        ),
        With<Player>,
    >,
    mut transforms: Query<(&mut Transform, Option<&Rest>), Without<Player>>,
    mut visibility: Query<&mut Visibility, Without<Player>>,
    mut trail_mesh: Query<&mut Mesh3d, Without<Player>>,
) {
    let Ok((motor, state, vel, facing, cs, soul, invulnerable, rig, mut anim)) =
        players.single_mut()
    else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    let live = !frozen.0;
    if live {
        anim.clock += dt;
    }
    for h in hits.read() {
        if h.victim_team != Team::Player && h.kind == HitKind::Nail {
            anim.flash = 1.0;
        }
    }
    anim.flash = (anim.flash - dt * 7.0).max(0.0);

    // ---- the swing ----
    if let Some(a) = cs.attack {
        anim.swing = Some((swing_dir(a.dir), a.facing));
    }
    let timing = swing_timing(&tuning);
    let cooldown = tuning.combat.nail_cooldown_ticks();
    let hurt = cs.stun > 0 || cs.dead;
    let swing = match anim.swing {
        Some((dir, _)) if cs.attack_cooldown > 0 && !hurt => {
            let t = cooldown.saturating_sub(cs.attack_cooldown) as f32;
            // The sim state is the end of the last tick; the frame shows a
            // moment between the last two. Frozen (hitstop): hold the impact.
            let alpha = if frozen.0 {
                1.0
            } else {
                fixed.overstep_fraction()
            };
            swing_pose(dir, (t - 1.0 + alpha).max(0.0), timing)
        }
        _ => None,
    };
    if swing.is_none() && cs.attack.is_none() {
        anim.swing = None;
    }
    let face = match (swing.is_some(), anim.swing) {
        (true, Some((_, f))) => f,
        _ => facing.0,
    };

    // ---- locomotion state ----
    let speed = vel.x.abs();
    if live && motor.grounded && speed > 0.6 {
        anim.run_phase += run_cycle_rate(speed) * dt;
    }
    if live {
        step_cape(&mut anim.cape, speed, vel.y, dt);
    }
    if motor.grounded && !anim.prev_grounded && anim.prev_vy < -4.0 {
        anim.land_timer = 0.12;
    }
    anim.prev_grounded = motor.grounded;
    anim.prev_vy = vel.y;

    let s = Sampled {
        input: KnightIn {
            grounded: motor.grounded,
            vx: vel.x,
            vy: vel.y,
            wall_slide: *state == PlayerState::WallSlide,
            dashing: *state == PlayerState::Dash,
            focusing: cs.focusing,
            hurt: *state == PlayerState::Hurt,
            dead: cs.dead,
            clock: anim.clock,
            run_phase: anim.run_phase,
            swing,
            soul: soul.value as f32 / soul.max.max(1) as f32,
            cape: [anim.cape[0].x, anim.cape[1].x, anim.cape[2].x],
        },
        facing: face,
    };
    let pose = knight_pose(&s.input);

    // ---- apply to the rig ----
    let flicker = invulnerable && (tick.0 / 6) & 1 == 0;
    let mut out = Out {
        pose: &pose,
        facing: s.facing,
        swing,
        dir: anim.swing.map(|(d, _)| d),
        flicker,
        state_tint: match *state {
            PlayerState::Focus => LinearRgba::rgb(0.10, 0.55, 0.28),
            PlayerState::Dash => LinearRgba::rgb(0.12, 0.40, 0.70),
            PlayerState::Hurt => LinearRgba::rgb(1.6, 0.12, 0.12),
            PlayerState::WallSlide => LinearRgba::rgb(0.30, 0.24, 0.05),
            _ => LinearRgba::rgb(0.05, 0.05, 0.05),
        },
        flash: anim.flash,
    };
    apply(
        &mut out,
        rig,
        &mut anim,
        &assets,
        &mut mats,
        &mut transforms,
        &mut visibility,
        &mut trail_mesh,
        dt,
    );
}

/// What to show this frame (from the sim, or synthetic in the viewer).
pub struct Out<'a> {
    pub pose: &'a crate::rig::pose::KnightPose,
    pub facing: i8,
    pub swing: Option<crate::rig::pose::SwingPose>,
    pub dir: Option<SwingDir>,
    pub flicker: bool,
    pub state_tint: LinearRgba,
    pub flash: f32,
}

/// Puts `out` on the rig: joints, facing, lean, squash, flicker, trail, glow.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    out: &mut Out,
    rig: &KnightRig,
    anim: &mut KnightAnim,
    assets: &KnightAssets,
    mats: &mut Assets<StandardMaterial>,
    transforms: &mut Query<(&mut Transform, Option<&Rest>), Without<Player>>,
    visibility: &mut Query<&mut Visibility, Without<Player>>,
    trail_mesh: &mut Query<&mut Mesh3d, Without<Player>>,
    dt: f32,
) {
    let pose = out.pose;
    for (k, e) in rig.joints.iter().enumerate() {
        if let Ok((mut t, Some(rest))) = transforms.get_mut(*e) {
            *t = posed(&rest.0, &pose.joints[k]);
        }
    }
    // Facing: yaw toward the camera a little, so the model has depth.
    let target_yaw = if out.facing >= 0 {
        -0.30
    } else {
        std::f32::consts::PI + 0.30
    };
    let step = angle_diff(anim.yaw, target_yaw);
    anim.yaw += step * (1.0 - (-26.0 * dt).exp());
    if let Ok((mut t, _)) = transforms.get_mut(rig.facing) {
        t.rotation = Quat::from_rotation_y(anim.yaw);
    }
    if let Ok((mut t, _)) = transforms.get_mut(rig.lean) {
        t.rotation = Quat::from_rotation_z(-pose.lean);
        t.translation = Vec3::new(0.0, pose.drop, 0.0);
    }
    // Squash and stretch about the feet.
    let (mut sx, mut sy) = (pose.squash[0], pose.squash[1]);
    if anim.land_timer > 0.0 {
        let k = anim.land_timer / 0.12;
        sx *= 1.0 + 0.25 * k;
        sy *= 1.0 - 0.25 * k;
        anim.land_timer = (anim.land_timer - dt).max(0.0);
    }
    if let Ok((mut t, _)) = transforms.get_mut(rig.squash) {
        t.scale = Vec3::new(sx, sy, sx);
    }
    // I-frame flicker hides the model (not the lantern light).
    if let Ok(mut v) = visibility.get_mut(rig.model_root) {
        let want = if out.flicker {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *v != want {
            *v = want;
        }
    }

    // ---- the trail ----
    let show = out
        .swing
        .filter(|s| s.trail_alpha > 0.01 && s.trail_head > 0.02);
    if let Ok(mut v) = visibility.get_mut(rig.trail) {
        let want = if show.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
    if let (Some(sp), Some(dir)) = (show, out.dir) {
        let d = dir_index(dir);
        let step = ((sp.trail_head * TRAIL_STEPS as f32).ceil() as usize).clamp(1, TRAIL_STEPS) - 1;
        if anim.last_trail != Some((d, step)) {
            if let Ok(mut m) = trail_mesh.get_mut(rig.trail) {
                m.0 = assets.trail[d][step].clone();
            }
            anim.last_trail = Some((d, step));
        }
        if (anim.last_trail_alpha - sp.trail_alpha).abs() > 0.01 {
            if let Some(m) = mats.get_mut(&assets.mats.trail) {
                m.base_color = Color::srgba(0.75, 0.93, 1.0, sp.trail_alpha);
            }
            anim.last_trail_alpha = sp.trail_alpha;
        }
    }

    // ---- glow and state tint ----
    let glow = (pose.glow + out.flash * 1.5).min(2.5);
    if (anim.last_glow - glow).abs() > 0.01 {
        if let Some(m) = mats.get_mut(&assets.mats.edge) {
            m.emissive = LinearRgba::rgb(1.3, 2.0, 3.0) * (0.25 + 1.6 * glow);
        }
        anim.last_glow = glow;
    }
    if anim.last_tint != out.state_tint {
        if let Some(m) = mats.get_mut(&assets.mats.bone) {
            m.emissive = out.state_tint;
        }
        anim.last_tint = out.state_tint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::pose::SwingTiming;
    use hk_sim::combat::Hitbox;
    use hk_sim::components::SimPos;
    use hk_sim::input::Action;
    use hk_sim::player::{spawn_player, Abilities};
    use hk_sim::testing::Harness;
    use hk_sim::world::TileGrid;

    /// Does the segment `a`-`b` touch the box (centre `c`, half extents `h`)?
    fn segment_hits_box(a: Vec2, b: Vec2, c: Vec2, h: Vec2) -> bool {
        let (lo, hi) = (c - h, c + h);
        let d = b - a;
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for axis in 0..2 {
            let (p, dd, l, u) = if axis == 0 {
                (a.x, d.x, lo.x, hi.x)
            } else {
                (a.y, d.y, lo.y, hi.y)
            };
            if dd.abs() < 1e-6 {
                if p < l || p > u {
                    return false;
                }
            } else {
                let (mut ta, mut tb) = ((l - p) / dd, (u - p) / dd);
                if ta > tb {
                    std::mem::swap(&mut ta, &mut tb);
                }
                t0 = t0.max(ta);
                t1 = t1.min(tb);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }

    #[test]
    fn segment_box_helper_is_right() {
        let c = Vec2::new(2.0, 0.0);
        let h = Vec2::new(0.5, 0.5);
        assert!(segment_hits_box(Vec2::ZERO, Vec2::new(3.0, 0.0), c, h));
        assert!(!segment_hits_box(Vec2::ZERO, Vec2::new(3.0, 2.0), c, h));
        assert!(segment_hits_box(
            Vec2::new(2.0, -2.0),
            Vec2::new(2.0, 2.0),
            c,
            h
        ));
        assert!(!segment_hits_box(
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
            c,
            h
        ));
    }

    /// The sword drawn on screen must be where the hitbox is while it can hurt
    /// something. For each direction: swing for real in the simulation, and at
    /// every tick the hitbox exists check that the blade (from the model's
    /// own pose maths) passes through it, allowing 0.3 units of slack.
    #[test]
    fn the_drawn_blade_is_inside_the_real_hitbox_for_every_live_tick() {
        // A tall room, floor top at y = 2, so a Down swing has air to happen in.
        let mut rows: Vec<&str> = vec![".............................."; 30];
        rows.extend(["##############################"; 2]);
        for dir in [SwingDir::Forward, SwingDir::Up, SwingDir::Down] {
            for facing in [1i8, -1] {
                let mut h = Harness::new();
                h.world_mut().insert_resource(TileGrid::from_ascii(&rows));
                let start_y = if dir == SwingDir::Down {
                    24.0
                } else {
                    2.0 + 0.75 + 0.001
                };
                let p = spawn_player(
                    h.world_mut(),
                    Vec2::new(10.0, start_y),
                    Abilities::default(),
                );
                h.tick_n(3);
                if facing < 0 {
                    h.press(Action::Left);
                    h.tick_n(2);
                    h.release(Action::Left);
                    h.tick_n(if dir == SwingDir::Down { 2 } else { 30 });
                }
                let tuning = h.world().resource::<Tuning>().clone();
                let timing: SwingTiming = swing_timing(&tuning);
                let cooldown = tuning.combat.nail_cooldown_ticks();
                match dir {
                    SwingDir::Up => h.press(Action::Up),
                    SwingDir::Down => h.press(Action::Down),
                    SwingDir::Forward => {}
                }
                h.press(Action::Attack);
                let mut checked = 0;
                for _ in 0..40 {
                    h.tick();
                    let cs = h.world().get::<CombatState>(p).unwrap();
                    let sim_dir = cs.attack.map(|a| swing_dir(a.dir));
                    let boxes: Vec<(Vec2, Vec2)> = h
                        .world_mut()
                        .query::<(&Hitbox, &SimPos)>()
                        .iter(h.world())
                        .filter(|(hb, _)| hb.kind == HitKind::Nail)
                        .map(|(hb, pos)| (pos.0, hb.half))
                        .collect();
                    let cs = h.world().get::<CombatState>(p).unwrap();
                    let t = cooldown.saturating_sub(cs.attack_cooldown) as f32;
                    let centre = h.world().get::<SimPos>(p).unwrap().0;
                    let f = h.world().get::<Facing>(p).unwrap().0 as f32;
                    for (bc, half) in boxes {
                        assert_eq!(
                            sim_dir,
                            Some(dir),
                            "the sim swung the direction we asked for"
                        );
                        let pose = swing_pose(dir, t, timing).expect("swinging");
                        // World position of the shoulder and the blade direction.
                        let shoulder = centre + Vec2::new(SHOULDER.x * f, SHOULDER.y - 0.75);
                        let ang = pose.angle;
                        let d = Vec2::new(f * ang.cos(), ang.sin());
                        let (from, to) = (shoulder + d * 0.52, shoulder + d * (2.27 + pose.thrust));
                        assert!(
                            segment_hits_box(from, to, bc, half + Vec2::splat(0.3)),
                            "{dir:?} facing {facing} at tick {t}: blade {from:?}->{to:?} misses the hitbox {bc:?} +- {half:?}"
                        );
                        checked += 1;
                    }
                }
                assert!(checked >= 8, "{dir:?}: only {checked} live ticks checked");
            }
        }
    }
}
