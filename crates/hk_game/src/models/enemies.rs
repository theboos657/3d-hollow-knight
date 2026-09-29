//! The enemies and the training dummy, built from `rig::meshkit` geometry.
//!
//! * **Husk**: a charred, hunched shell with glowing cracks, a pale skull and
//!   long claws. It rears back to lunge.
//! * **Wisp**: a glass orb with a burning core, a little crown and five
//!   trailing tendrils. It squeezes small, then dives like a comet.
//! * **Shieldbearer**: a barrel of riveted iron behind a tower shield that
//!   stands on the guarded side; a glowing vent on the other side is the
//!   weak spot.
//! * **Spitter**: a warty pod on stub legs with a long stalk and a flared
//!   maw that tracks its target; its belly swells before it spits.
//! * **Dummy**: a straw-and-burlap training post with a target painted on it,
//!   in warm ochre so it never reads as an enemy.
//!
//! Each creature has two per-instance materials: `body` (washed with the tell
//! colour) and `glow` (cracks, eyes, core, weak spot), so what an enemy is
//! about to do stays readable exactly as before (`rig::creature::tell_glow`),
//! and the pose changes as well, so it is not colour alone.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::prelude::*;
use hk_sim::boss::{Boss, Pendulum};
use hk_sim::combat::{EnemyDied, Guard, Hit, Hurtbox, SimFrozen, Team};
use hk_sim::components::{Aabb, SimPos, Velocity};
use hk_sim::enemy::{Brain, EnemyKind, EnemyState};
use hk_sim::player::Player;
use hk_sim::tuning::{EnemyTuning, Tuning};
use hk_sim::SimTick;

use super::geo::enemy_meshes;
pub use super::geo::MeshList;
use crate::interp::Interpolated;
use crate::look::pbr::{Kind, Materials, EMISSIVE_FLOOR};
use crate::rig::creature::{
    creature_pose, dummy, ease_guard, species_glow, spitter, step_sway, tell_glow, wisp,
    CreatureIn, CreaturePose, Species, BODY_WASH, MAX_JOINTS,
};
use crate::rig::pose::{angle_diff, Spring};
use crate::rig::{joint, part, posed, ModelRoot, Rest};

pub struct EnemyModelsPlugin;

impl Plugin for EnemyModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Ledger>()
            .add_systems(Startup, build_enemy_assets)
            .add_systems(
                Update,
                (
                    spawn_enemy_models,
                    animate_creatures,
                    super::bosses::attach_boss_models,
                    super::bosses::animate_bosses,
                    // Shards first: the ledger still remembers what died last frame.
                    spawn_shards,
                    record_ledger,
                    fly_shards,
                )
                    .chain()
                    .after(crate::interp::RenderPrepSet),
            );
    }
}

// ------------------------------------------------------------------ assets --

#[derive(Resource)]
pub struct EnemyAssets {
    meshes: HashMap<&'static str, Handle<Mesh>>,
    mats: HashMap<&'static str, Handle<StandardMaterial>>,
}

/// Adds named materials to the asset map (used by the boss module too).
pub struct MatBuilder<'a> {
    mats: &'a mut Assets<StandardMaterial>,
    map: &'a mut HashMap<&'static str, Handle<StandardMaterial>>,
    pbr: &'a Materials,
}

impl MatBuilder<'_> {
    pub fn add(&mut self, k: &'static str, s: StandardMaterial) {
        self.map.insert(k, self.mats.add(s));
    }
    pub fn lit(&mut self, k: &'static str, c: Color, rough: f32, metallic: f32) {
        self.add(k, lit(c, rough, metallic));
    }
    pub fn glow(&mut self, k: &'static str, species: Species) {
        self.add(k, glow_material(species));
    }
    /// A surface with a full set of maps (`rough` scales the roughness map,
    /// `coat` is a clear wet or lacquered layer on top).
    pub fn skin(&mut self, k: &'static str, kind: Kind, tint: Color, rough: f32, coat: f32) {
        let m = skin_material(self.pbr, kind, tint, rough, coat);
        self.add(k, m);
    }
    /// The same, with a last adjustment (transmission, culling, ...).
    pub fn skin_with(
        &mut self,
        k: &'static str,
        kind: Kind,
        tint: Color,
        rough: f32,
        coat: f32,
        f: impl FnOnce(&mut StandardMaterial),
    ) {
        let mut m = skin_material(self.pbr, kind, tint, rough, coat);
        f(&mut m);
        self.add(k, m);
    }
}

impl EnemyAssets {
    pub(super) fn m(&self, k: &str) -> Handle<Mesh> {
        self.meshes
            .get(k)
            .unwrap_or_else(|| panic!("no enemy mesh `{k}`"))
            .clone()
    }
    pub(super) fn mat(&self, k: &str) -> Handle<StandardMaterial> {
        self.mats
            .get(k)
            .unwrap_or_else(|| panic!("no enemy material `{k}`"))
            .clone()
    }
}

fn lit(base: Color, rough: f32, metallic: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: base,
        perceptual_roughness: rough,
        metallic,
        ..default()
    }
}

/// A creature surface from a generated map set. Creatures are small, so there
/// is no parallax (the normal map does the work).
pub fn skin_material(
    pbr: &Materials,
    kind: Kind,
    tint: Color,
    rough: f32,
    coat: f32,
) -> StandardMaterial {
    let mut m = pbr.get(kind).material();
    m.base_color = tint;
    m.perceptual_roughness = rough;
    m.clearcoat = coat;
    m.clearcoat_perceptual_roughness = 0.2;
    m.depth_map = None;
    m
}

/// A material for glowing parts: dark base, emissive driven per instance.
fn glow_material(species: Species) -> StandardMaterial {
    let g = species_glow(species);
    StandardMaterial {
        base_color: Color::srgb(0.10, 0.06, 0.04),
        perceptual_roughness: 0.5,
        emissive: LinearRgba::rgb(g[0], g[1], g[2]),
        ..default()
    }
}

/// How much brighter than the flat wash a body's own emissive colour must be:
/// its glow map is at least [`EMISSIVE_FLOOR`] everywhere, so scaling by the
/// reciprocal gives the same even wash the flat bodies had, with the veins and
/// seams (up to five times brighter) glowing through it.
fn body_emissive_scale(species: Species) -> f32 {
    match species {
        Species::Husk
        | Species::Shieldbearer
        | Species::Spitter
        | Species::Matron
        | Species::Bellwarden => 1.0 / EMISSIVE_FLOOR,
        _ => 1.0,
    }
}

pub fn build_enemy_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pbr: Res<Materials>,
) {
    commands.insert_resource(make_assets(&mut meshes, &mut mats, &pbr));
}

pub fn make_assets(
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    pbr: &Materials,
) -> EnemyAssets {
    let mesh_map = enemy_meshes()
        .into_iter()
        .map(|(k, d)| (k, meshes.add(d.to_mesh_pbr())))
        .collect();
    let mut m: HashMap<&'static str, Handle<StandardMaterial>> = HashMap::new();
    let mut b = MatBuilder {
        mats,
        map: &mut m,
        pbr,
    };
    let c = Color::srgb;
    // Husk: charred chitin, a bone skull, leathery limbs.
    b.skin("husk_shell", Kind::Chitin, c(0.9, 0.84, 0.80), 0.9, 0.55);
    b.glow("husk_glow", Species::Husk);
    b.skin("husk_flesh", Kind::Leather, c(0.55, 0.34, 0.27), 0.9, 0.0);
    b.skin("husk_bone", Kind::Bone, c(1.0, 0.96, 0.88), 1.0, 0.12);
    b.skin("husk_char", Kind::Chitin, c(0.45, 0.40, 0.38), 1.0, 0.0);
    b.lit("husk_dark", c(0.02, 0.015, 0.015), 0.9, 0.0);
    // Wisp: real glass around a burning core, ghost-bone horns, silky threads.
    b.add(
        "wisp_glass",
        StandardMaterial {
            base_color: c(0.90, 0.82, 1.0),
            perceptual_roughness: 0.05,
            reflectance: 0.6,
            specular_transmission: 0.9,
            ior: 1.35,
            thickness: 0.45,
            attenuation_color: c(0.48, 0.26, 0.95),
            attenuation_distance: 0.32,
            ..default()
        },
    );
    b.glow("wisp_glow", Species::Wisp);
    b.skin("wisp_bone", Kind::Bone, c(0.55, 0.42, 0.85), 0.55, 0.4);
    b.skin_with(
        "wisp_flesh",
        Kind::Flesh,
        c(0.50, 0.34, 0.86),
        0.5,
        0.3,
        |m| m.diffuse_transmission = 0.5,
    );
    // Shieldbearer: worn plate, dark iron, bronze trim, leather and cloth.
    b.skin("shield_barrel", Kind::Steel, c(0.62, 0.78, 0.82), 1.0, 0.0);
    b.glow("shield_glow", Species::Shieldbearer);
    b.skin("shield_iron", Kind::Iron, c(0.70, 0.76, 0.80), 1.0, 0.0);
    b.skin("shield_face", Kind::Steel, c(0.95, 1.0, 1.0), 1.8, 0.0);
    b.skin("shield_bronze", Kind::Bronze, c(1.0, 0.88, 0.70), 1.0, 0.0);
    b.skin(
        "shield_leather",
        Kind::Leather,
        c(0.95, 0.72, 0.55),
        0.9,
        0.0,
    );
    b.skin_with(
        "shield_cloth",
        Kind::Cloth,
        c(0.85, 0.16, 0.12),
        0.95,
        0.0,
        |m| {
            m.cull_mode = None;
            m.double_sided = true;
        },
    );
    // Spitter: wet, glistening flesh that light passes a little way into.
    b.skin_with(
        "spitter_pod",
        Kind::Flesh,
        c(0.42, 0.72, 0.30),
        0.55,
        0.75,
        |m| m.diffuse_transmission = 0.3,
    );
    b.glow("spitter_glow", Species::Spitter);
    b.skin("spitter_stalk", Kind::Flesh, c(0.70, 0.92, 0.52), 0.6, 0.6);
    b.skin("spitter_leg", Kind::Chitin, c(0.55, 0.75, 0.42), 0.9, 0.2);
    b.skin("spitter_tooth", Kind::Bone, c(0.95, 0.92, 0.75), 0.9, 0.1);
    // Death shards: the creature's own material, still glowing a little.
    let shard = |c: Color, e: LinearRgba| StandardMaterial {
        emissive: e,
        ..lit(c, 0.7, 0.0)
    };
    b.add(
        "shard_husk",
        shard(c(0.22, 0.14, 0.11), LinearRgba::rgb(0.9, 0.3, 0.06)),
    );
    b.add(
        "shard_wisp",
        shard(c(0.55, 0.42, 0.95), LinearRgba::rgb(0.6, 0.35, 1.4)),
    );
    b.add(
        "shard_shield",
        shard(c(0.30, 0.42, 0.46), LinearRgba::rgb(0.02, 0.06, 0.07)),
    );
    b.add(
        "shard_spitter",
        shard(c(0.30, 0.50, 0.20), LinearRgba::rgb(0.2, 0.7, 0.1)),
    );
    // Dummy: warm ochre burlap and timber, distinct from every enemy.
    b.skin("dummy_burlap", Kind::Burlap, c(1.0, 0.86, 0.66), 1.0, 0.0);
    b.skin("dummy_wood", Kind::Wood, c(0.85, 0.70, 0.55), 0.95, 0.0);
    b.skin(
        "dummy_straw",
        Kind::Burlap,
        Color::linear_rgb(1.6, 1.25, 0.55),
        1.0,
        0.0,
    );
    b.skin("dummy_red", Kind::Burlap, c(1.0, 0.16, 0.10), 0.9, 0.0);
    b.skin(
        "dummy_cream",
        Kind::Burlap,
        Color::linear_rgb(1.5, 1.4, 1.15),
        0.9,
        0.0,
    );
    b.skin("dummy_rope", Kind::Burlap, c(0.62, 0.46, 0.28), 1.0, 0.0);
    b.lit("dummy_dark", c(0.10, 0.07, 0.05), 0.9, 0.0);
    super::bosses::boss_materials(&mut b);
    EnemyAssets {
        meshes: mesh_map,
        mats: m,
    }
}

// --------------------------------------------------------------------- rig --

/// The entities of one creature.
#[derive(Component)]
pub struct CreatureRig {
    pub species: Species,
    pub squash: Entity,
    pub facing: Entity,
    pub lean: Entity,
    pub joints: [Entity; MAX_JOINTS],
    /// Per-instance materials: the body (washed by the tell) and the glow.
    pub body: Handle<StandardMaterial>,
    pub glow: Handle<StandardMaterial>,
    /// What the body's emissive wash is multiplied by (see [`body_emissive_scale`]).
    pub emissive_scale: f32,
}

/// Per-creature animation state.
#[derive(Component)]
pub struct CreatureAnim {
    pub clock: f32,
    pub walk: f32,
    pub yaw: f32,
    pub hit: f32,
    pub sway: Spring,
    pub guard: f32,
    last_glow: [f32; 3],
    last_body: [f32; 3],
}

impl CreatureAnim {
    pub fn new(face: i8, guard: f32) -> Self {
        Self {
            clock: 0.0,
            walk: 0.0,
            yaw: facing_yaw(face),
            hit: 0.0,
            sway: Spring::default(),
            guard,
            last_glow: [-1.0; 3],
            last_body: [-1.0; 3],
        }
    }
}

fn facing_yaw(face: i8) -> f32 {
    if face >= 0 {
        -0.30
    } else {
        PI + 0.30
    }
}

fn clone_mat(
    mats: &mut Assets<StandardMaterial>,
    of: &Handle<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let m = mats.get(of).cloned().unwrap_or_default();
    mats.add(m)
}

/// Builds the hierarchy of one creature under `anchor` (whose sim position
/// is the centre of the creature's box, `half_y` above its feet).
pub fn spawn_creature(
    commands: &mut Commands,
    mats: &mut Assets<StandardMaterial>,
    a: &EnemyAssets,
    anchor: Entity,
    species: Species,
    half_y: f32,
) -> CreatureRig {
    let (body_key, glow_key) = match species {
        Species::Husk => ("husk_shell", "husk_glow"),
        Species::Wisp => ("wisp_glass", "wisp_glow"),
        Species::Shieldbearer => ("shield_barrel", "shield_glow"),
        Species::Spitter => ("spitter_pod", "spitter_glow"),
        Species::Dummy => ("dummy_burlap", "husk_glow"),
        Species::Matron | Species::Bellwarden => super::bosses::body_glow_keys(species),
    };
    let body = clone_mat(mats, &a.mat(body_key));
    let glow = clone_mat(mats, &a.mat(glow_key));

    let model_root = commands
        .spawn((
            ModelRoot,
            Transform::from_xyz(0.0, -half_y, 0.0),
            Visibility::default(),
        ))
        .id();
    commands.entity(anchor).add_child(model_root);
    let node = |commands: &mut Commands, parent: Entity| {
        let e = commands
            .spawn((Transform::default(), Visibility::default()))
            .id();
        commands.entity(parent).add_child(e);
        e
    };
    let squash = node(commands, model_root);
    let facing = node(commands, squash);
    let lean = node(commands, facing);

    let joints = match species {
        Species::Husk => build_husk(commands, a, lean, &body, &glow),
        Species::Wisp => build_wisp(commands, a, lean, &body, &glow),
        Species::Shieldbearer => build_shield(commands, a, lean, &body, &glow),
        Species::Spitter => build_spitter(commands, a, lean, &body, &glow),
        Species::Dummy => build_dummy(commands, a, lean, &body),
        Species::Matron => super::bosses::build_matron(commands, a, lean, &body, &glow),
        Species::Bellwarden => super::bosses::build_warden(commands, a, lean, &body, &glow),
    };
    CreatureRig {
        species,
        squash,
        facing,
        lean,
        joints,
        body,
        glow,
        emissive_scale: body_emissive_scale(species),
    }
}

type Mat = Handle<StandardMaterial>;

fn t(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

fn build_husk(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use crate::rig::creature::husk::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.55, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("husk_shell"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("husk_belly"),
        a.mat("husk_flesh"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("husk_spines"),
        a.mat("husk_char"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("husk_cracks"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let head = joint(c, body, t(0.34, 0.06, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("husk_skull"),
        a.mat("husk_bone"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("husk_jaw"),
        a.mat("husk_flesh"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("husk_sockets"),
        a.mat("husk_dark"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("husk_eyes"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(ARM_FRONT, 0.20, 0.30), (ARM_BACK, 0.08, -0.30)] {
        let arm = joint(c, body, t(x, -0.02, z));
        j[idx] = arm;
        part(
            c,
            arm,
            a.m("husk_arm"),
            a.mat("husk_flesh"),
            Transform::IDENTITY,
        );
        part(
            c,
            arm,
            a.m("husk_claws"),
            a.mat("husk_bone"),
            Transform::IDENTITY,
        );
    }
    for (idx, x, z) in [(LEG_FRONT, 0.14, 0.16), (LEG_BACK, -0.14, -0.16)] {
        let leg = joint(c, lean, t(x, 0.30, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("husk_leg"),
            a.mat("husk_flesh"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("husk_foot"),
            a.mat("husk_char"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_wisp(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let orb = joint(c, lean, t(0.0, 0.45, 0.0));
    j[wisp::ORB] = orb;
    part(c, orb, a.m("wisp_orb"), body_m.clone(), Transform::IDENTITY);
    part(
        c,
        orb,
        a.m("wisp_halo"),
        a.mat("wisp_flesh"),
        Transform::IDENTITY,
    );
    part(
        c,
        orb,
        a.m("wisp_crown"),
        a.mat("wisp_bone"),
        Transform::IDENTITY,
    );
    part(
        c,
        orb,
        a.m("wisp_veins"),
        glow_m.clone(),
        Transform::IDENTITY,
    );
    let core = joint(c, orb, Transform::IDENTITY);
    j[wisp::CORE] = core;
    part(
        c,
        core,
        a.m("wisp_core"),
        glow_m.clone(),
        Transform::IDENTITY,
    );
    let xs = [-0.20, -0.10, 0.0, 0.10, 0.20];
    let zs = [0.10, -0.12, 0.14, -0.10, 0.08];
    let lens = [1.0, 1.25, 1.45, 1.2, 0.95];
    for k in 0..wisp::TENDRILS {
        let e = joint(
            c,
            orb,
            t(xs[k], -0.30, zs[k]).with_scale(Vec3::new(1.0, lens[k], 1.0)),
        );
        j[wisp::TENDRIL + k] = e;
        part(
            c,
            e,
            a.m("wisp_tendril"),
            a.mat("wisp_flesh"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_shield(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use crate::rig::creature::shield::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.85, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("shield_barrel"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_bands"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_rivets"),
        a.mat("shield_bronze"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_belt"),
        a.mat("shield_leather"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_buckle"),
        a.mat("shield_bronze"),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("shield_pauldrons"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );

    let head = joint(c, body, t(0.05, 0.44, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("shield_helm"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("shield_plume"),
        a.mat("shield_cloth"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("shield_visor"),
        a.mat("husk_dark"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("shield_eyes"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    // Placed by the pose (it slides to whichever side is guarded).
    let sh = joint(c, body, t(0.0, 0.0, 0.0));
    j[SHIELD] = sh;
    part(
        c,
        sh,
        a.m("shield_rim"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );
    part(
        c,
        sh,
        a.m("shield_sigil"),
        a.mat("shield_bronze"),
        Transform::IDENTITY,
    );
    part(
        c,
        sh,
        a.m("shield_face"),
        a.mat("shield_face"),
        Transform::IDENTITY,
    );
    part(
        c,
        sh,
        a.m("shield_trim"),
        a.mat("shield_iron"),
        Transform::IDENTITY,
    );

    let weak = joint(c, body, t(0.0, 0.0, 0.0));
    j[WEAK] = weak;
    part(
        c,
        weak,
        a.m("shield_vent"),
        glow_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        weak,
        a.m("shield_grille"),
        a.mat("husk_dark"),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(LEG_FRONT, 0.16, 0.20), (LEG_BACK, -0.16, -0.20)] {
        let leg = joint(c, lean, t(x, 0.42, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("shield_leg"),
            a.mat("shield_iron"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("shield_boot"),
            a.mat("husk_char"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_spitter(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
    glow_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use spitter::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    let body = joint(c, lean, t(0.0, 0.45, 0.0));
    j[BODY] = body;
    part(
        c,
        body,
        a.m("spitter_pod"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        body,
        a.m("spitter_spots"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let maw = joint(c, body, t(0.15, 0.30, 0.0));
    j[MAW] = maw;
    // The neck and maw are authored around the joint at the stalk's root.
    part(
        c,
        maw,
        a.m("spitter_neck"),
        a.mat("spitter_stalk"),
        Transform::IDENTITY,
    );
    part(
        c,
        maw,
        a.m("spitter_maw"),
        a.mat("spitter_stalk"),
        Transform::IDENTITY,
    );
    part(
        c,
        maw,
        a.m("spitter_teeth"),
        a.mat("spitter_tooth"),
        Transform::IDENTITY,
    );
    part(
        c,
        maw,
        a.m("spitter_mouth"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    let belly = joint(c, body, t(0.30, -0.06, 0.0));
    j[BELLY] = belly;
    part(
        c,
        belly,
        a.m("spitter_belly"),
        glow_m.clone(),
        Transform::IDENTITY,
    );

    for (idx, x, z) in [(LEG_FRONT, 0.16, 0.16), (LEG_BACK, -0.16, -0.16)] {
        let leg = joint(c, lean, t(x, 0.24, z));
        j[idx] = leg;
        part(
            c,
            leg,
            a.m("spitter_leg"),
            a.mat("spitter_leg"),
            Transform::IDENTITY,
        );
        part(
            c,
            leg,
            a.m("spitter_toe"),
            a.mat("spitter_leg"),
            Transform::IDENTITY,
        );
    }
    j
}

fn build_dummy(
    c: &mut Commands,
    a: &EnemyAssets,
    lean: Entity,
    body_m: &Mat,
) -> [Entity; MAX_JOINTS] {
    use dummy::*;
    let mut j = [Entity::PLACEHOLDER; MAX_JOINTS];
    // The whole post pivots about its base.
    let post = joint(c, lean, Transform::IDENTITY);
    j[POST] = post;
    part(
        c,
        post,
        a.m("dummy_post"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_base"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_rope"),
        a.mat("dummy_rope"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_target_cream"),
        a.mat("dummy_cream"),
        Transform::IDENTITY,
    );
    part(
        c,
        post,
        a.m("dummy_target_red"),
        a.mat("dummy_red"),
        Transform::IDENTITY,
    );

    let arms = joint(c, post, t(0.0, 0.76, 0.0));
    j[ARMS] = arms;
    part(
        c,
        arms,
        a.m("dummy_bar"),
        a.mat("dummy_wood"),
        Transform::IDENTITY,
    );
    part(
        c,
        arms,
        a.m("dummy_lash"),
        a.mat("dummy_rope"),
        Transform::IDENTITY,
    );
    part(
        c,
        arms,
        a.m("dummy_straw"),
        a.mat("dummy_straw"),
        Transform::IDENTITY,
    );

    let head = joint(c, post, t(0.0, 0.98, 0.0));
    j[HEAD] = head;
    part(
        c,
        head,
        a.m("dummy_head"),
        body_m.clone(),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("dummy_face"),
        a.mat("dummy_dark"),
        Transform::IDENTITY,
    );
    part(
        c,
        head,
        a.m("dummy_tuft"),
        a.mat("dummy_straw"),
        Transform::IDENTITY,
    );
    j
}

// ----------------------------------------------------------------- systems --

/// Gives every new enemy and dummy its model.
#[allow(clippy::type_complexity)]
pub fn spawn_enemy_models(
    mut commands: Commands,
    assets: Option<Res<EnemyAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    enemies: Query<(Entity, &Brain, &Aabb, &SimPos, Option<&Guard>), Added<Brain>>,
    dummies: Query<
        (Entity, &Hurtbox, &Aabb, &SimPos),
        (
            Added<Hurtbox>,
            Without<Brain>,
            Without<Boss>,
            Without<Pendulum>,
            Without<Player>,
        ),
    >,
    grid: Option<Res<hk_sim::world::grid::TileGrid>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(assets) = assets else {
        return;
    };
    let prep = |commands: &mut Commands, e: Entity, pos: &SimPos| {
        // Transform and Visibility first, so children never see a bare parent.
        commands.entity(e).insert((
            Transform::from_xyz(pos.0.x, pos.0.y, 0.0),
            Visibility::default(),
            Interpolated {
                z: 0.0,
                offset: Vec2::ZERO,
            },
        ));
    };
    for (e, brain, aabb, pos, guard) in &enemies {
        prep(&mut commands, e, pos);
        let rig = spawn_creature(
            &mut commands,
            &mut mats,
            &assets,
            e,
            brain.kind.into(),
            aabb.half.y,
        );
        let g = guard.map_or(1.0, |g| (g.facing * brain.facing) as f32);
        commands
            .entity(e)
            .insert((rig, CreatureAnim::new(brain.facing, g)));
    }
    for (e, hurt, aabb, pos) in &dummies {
        if hurt.team != Team::Enemy {
            continue;
        }
        prep(&mut commands, e, pos);
        let rig = spawn_creature(
            &mut commands,
            &mut mats,
            &assets,
            e,
            Species::Dummy,
            aabb.half.y,
        );
        // A dummy with no ground under it (a pogo target over a pit) hangs from
        // a chain, so it never looks like it is floating.
        let feet = pos.0.y - aabb.half.y;
        let hanging = grid.as_ref().is_some_and(|g| {
            let (i, j) = (pos.0.x.floor() as i32, (feet - 0.15).floor() as i32);
            !matches!(
                g.get(i, j),
                hk_sim::world::grid::Tile::Solid | hk_sim::world::grid::Tile::OneWay
            )
        });
        if hanging {
            let chain = crate::look::kits::chain(0.0, 1.3, 60.0, 0.0);
            let chain_e = commands
                .spawn((
                    Mesh3d(meshes.add(chain.to_mesh())),
                    MeshMaterial3d(assets.mat("dummy_dark")),
                    Transform::IDENTITY,
                    Visibility::default(),
                ))
                .id();
            commands.entity(rig.lean).add_child(chain_e);
        }
        commands.entity(e).insert((rig, CreatureAnim::new(1, 1.0)));
    }
}

/// The planned length in ticks of the current state (0 when open-ended).
pub fn state_len(kind: EnemyKind, state: EnemyState, t: &EnemyTuning) -> f32 {
    let n = match (kind, state) {
        (EnemyKind::Husk, EnemyState::Notice) => t.husk.notice_ticks(),
        (EnemyKind::Husk, EnemyState::Windup) => t.husk.windup_ticks(),
        (EnemyKind::Husk, EnemyState::Attack) => t.husk.lunge_ticks(),
        (EnemyKind::Husk, EnemyState::Recover) => t.husk.recover_ticks(),
        (EnemyKind::Wisp, EnemyState::Notice) => t.wisp.notice_ticks(),
        (EnemyKind::Wisp, EnemyState::Windup) => t.wisp.windup_ticks(),
        (EnemyKind::Wisp, EnemyState::Attack) => t.wisp.dive_ticks(),
        (EnemyKind::Wisp, EnemyState::Recover) => t.wisp.recover_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Notice) => t.shield.notice_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Windup) => t.shield.windup_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Attack) => t.shield.bash_ticks(),
        (EnemyKind::Shieldbearer, EnemyState::Recover) => t.shield.recover_ticks(),
        (EnemyKind::Spitter, EnemyState::Notice) => t.spitter.notice_ticks(),
        (EnemyKind::Spitter, EnemyState::Windup) => t.spitter.windup_ticks(),
        (EnemyKind::Spitter, EnemyState::Recover) => t.spitter.recover_ticks(),
        _ => 0,
    };
    n as f32
}

fn sign(v: f32, fallback: i8) -> i8 {
    if v > 0.2 {
        1
    } else if v < -0.2 {
        -1
    } else {
        fallback
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_creatures(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    frozen: Res<SimFrozen>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut hits: MessageReader<Hit>,
    mut creatures: Query<
        (
            Entity,
            Option<&Brain>,
            Option<&Guard>,
            &Velocity,
            &CreatureRig,
            &mut CreatureAnim,
        ),
        Without<hk_sim::boss::BossBrain>,
    >,
    mut transforms: Query<(&mut Transform, Option<&Rest>), Without<CreatureRig>>,
) {
    let dt = time.delta_secs().min(0.05);
    let live = !frozen.0;
    let alpha = if live { fixed.overstep_fraction() } else { 1.0 };
    let hit_events: Vec<Hit> = hits.read().copied().collect();
    for (e, brain, guard, vel, rig, mut anim) in &mut creatures {
        if live {
            anim.clock += dt;
        }
        for h in hit_events.iter().filter(|h| h.victim == e) {
            anim.hit = 1.0;
            if rig.species == Species::Dummy {
                anim.sway.v += h.dir as f32 * 6.0;
            }
        }
        anim.hit = (anim.hit - dt * 4.5).max(0.0);
        if rig.species == Species::Dummy && live {
            step_sway(&mut anim.sway, dt);
        }

        let (state, timer, facing, aim_v) = brain
            .map(|b| (b.state, b.timer, b.facing, b.aim))
            .unwrap_or((EnemyState::Idle, 0, 1, Vec2::X));
        // Wisps and Spitters turn to face what they are about to attack.
        let attacking = matches!(state, EnemyState::Windup | EnemyState::Attack);
        let face = match rig.species {
            Species::Wisp | Species::Spitter if attacking => sign(aim_v.x, facing),
            _ => facing,
        };
        let fwd = face as f32;
        let vx = vel.x * fwd;
        if live && vx.abs() > 0.3 {
            anim.walk += (5.0 + vx.abs() * 2.2).min(34.0) * dt;
        }
        if let Some(g) = guard {
            let target = (g.facing * facing) as f32;
            anim.guard = ease_guard(anim.guard, target, dt);
        }
        let len = brain
            .map(|b| state_len(b.kind, state, &tuning.enemies))
            .unwrap_or(0.0);
        let input = CreatureIn {
            species: rig.species,
            state,
            t: (timer as f32 - 1.0 + alpha).max(0.0),
            len,
            clock: anim.clock,
            walk: anim.walk,
            vx,
            aim: aim_v.y.atan2((aim_v.x * fwd).max(0.05)),
            guard: anim.guard,
            hit: anim.hit,
            sway: anim.sway.x,
            ..Default::default()
        };
        let pose = creature_pose(&input);
        let glow = if rig.species == Species::Dummy {
            [0.0; 3]
        } else {
            tell_glow(rig.species, state, tick.0)
        };
        apply_creature(
            &pose,
            face,
            glow,
            anim.hit,
            rig,
            &mut anim,
            &mut mats,
            &mut transforms,
            dt,
        );
    }
}

/// Puts a pose on a creature's rig: joints, facing, lean, squash and the two
/// tell materials. Shared by the game and the viewer.
#[allow(clippy::too_many_arguments)]
pub fn apply_creature(
    pose: &CreaturePose,
    face: i8,
    glow: [f32; 3],
    hit: f32,
    rig: &CreatureRig,
    anim: &mut CreatureAnim,
    mats: &mut Assets<StandardMaterial>,
    transforms: &mut Query<(&mut Transform, Option<&Rest>), Without<CreatureRig>>,
    dt: f32,
) {
    for (k, e) in rig.joints.iter().enumerate() {
        if *e == Entity::PLACEHOLDER {
            continue;
        }
        if let Ok((mut tr, Some(rest))) = transforms.get_mut(*e) {
            *tr = posed(&rest.0, &pose.joints[k]);
        }
    }
    let target = facing_yaw(face);
    anim.yaw += angle_diff(anim.yaw, target) * (1.0 - (-22.0 * dt).exp());
    if let Ok((mut tr, _)) = transforms.get_mut(rig.facing) {
        tr.rotation = Quat::from_rotation_y(anim.yaw);
    }
    if let Ok((mut tr, _)) = transforms.get_mut(rig.lean) {
        tr.rotation = Quat::from_rotation_z(-pose.lean);
        tr.translation = Vec3::new(0.0, pose.drop, 0.0);
    }
    if let Ok((mut tr, _)) = transforms.get_mut(rig.squash) {
        tr.scale = Vec3::new(pose.squash[0], pose.squash[1], pose.squash[0]);
    }

    // A fresh hit flashes the whole body white for a moment.
    let flash = hit * hit;
    let glow_e = [
        glow[0] + flash * 2.5,
        glow[1] + flash * 2.5,
        glow[2] + flash * 2.5,
    ];
    let k = rig.emissive_scale;
    let body_e = [
        (glow[0] * BODY_WASH + flash * 0.8) * k,
        (glow[1] * BODY_WASH + flash * 0.8) * k,
        (glow[2] * BODY_WASH + flash * 0.8) * k,
    ];
    let differs = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).any(|(x, y)| (x - y).abs() > 0.01);
    if differs(anim.last_glow, glow_e) {
        if let Some(m) = mats.get_mut(&rig.glow) {
            m.emissive = LinearRgba::rgb(glow_e[0], glow_e[1], glow_e[2]);
        }
        anim.last_glow = glow_e;
    }
    if differs(anim.last_body, body_e) {
        if let Some(m) = mats.get_mut(&rig.body) {
            m.emissive = LinearRgba::rgb(body_e[0], body_e[1], body_e[2]);
        }
        anim.last_body = body_e;
    }
}

// ------------------------------------------------------------ death shards --

/// Which species each live creature is, so a death (reported a frame after the
/// entity is gone) can still be drawn as the right splinters.
#[derive(Resource, Default)]
pub struct Ledger(HashMap<Entity, Species>);

fn record_ledger(mut ledger: ResMut<Ledger>, q: Query<(Entity, &CreatureRig)>) {
    ledger.0.retain(|e, _| q.contains(*e));
    for (e, rig) in &q {
        ledger.0.insert(e, rig.species);
    }
}

#[derive(Component)]
struct Shard {
    vel: Vec3,
    spin: Vec3,
    life: f32,
    max: f32,
    size: f32,
}

fn spawn_shards(
    mut commands: Commands,
    assets: Option<Res<EnemyAssets>>,
    mut ledger: ResMut<Ledger>,
    mut died: MessageReader<EnemyDied>,
    mut seed: Local<u32>,
) {
    let Some(assets) = assets else {
        died.clear();
        return;
    };
    let mut rand = || {
        // xorshift: purely visual randomness.
        *seed = (*seed).max(0x9E37_79B9);
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for d in died.read() {
        let Some(species) = ledger.0.remove(&d.entity) else {
            continue;
        };
        let key = match species {
            Species::Husk => "shard_husk",
            Species::Wisp => "shard_wisp",
            Species::Shieldbearer => "shard_shield",
            Species::Spitter => "shard_spitter",
            Species::Dummy | Species::Matron | Species::Bellwarden => continue,
        };
        for _ in 0..11 {
            let ang = rand() * std::f32::consts::TAU;
            let speed = 3.0 + rand() * 6.0;
            let life = 0.55 + rand() * 0.5;
            let size = 0.7 + rand() * 0.9;
            commands.spawn((
                Shard {
                    vel: Vec3::new(
                        ang.cos() * speed,
                        ang.sin().abs() * speed + 2.5,
                        (rand() - 0.5) * 3.0,
                    ),
                    spin: Vec3::new(rand() - 0.5, rand() - 0.5, rand() - 0.5) * 16.0,
                    life,
                    max: life,
                    size,
                },
                Mesh3d(assets.m("shard")),
                MeshMaterial3d(assets.mat(key)),
                Transform::from_xyz(d.pos.x, d.pos.y, 0.2)
                    .with_rotation(Quat::from_euler(
                        EulerRot::XYZ,
                        rand() * 6.0,
                        rand() * 6.0,
                        rand() * 6.0,
                    ))
                    .with_scale(Vec3::splat(size)),
            ));
        }
    }
}

fn fly_shards(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Shard, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut s, mut t) in &mut q {
        s.life -= dt;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        s.vel.y -= 26.0 * dt;
        t.translation += s.vel * dt;
        let spin = s.spin * dt;
        t.rotation = Quat::from_euler(EulerRot::XYZ, spin.x, spin.y, spin.z) * t.rotation;
        // Shrink away over the last part of their life.
        t.scale = Vec3::splat(s.size * (s.life / s.max * 2.0).min(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_creature_mesh_is_well_formed() {
        for (name, m) in enemy_meshes() {
            m.validate()
                .unwrap_or_else(|e| panic!("mesh `{name}` is malformed: {e}"));
        }
    }

    #[test]
    fn the_creatures_fit_the_boxes_they_are_hit_in() {
        // Sizes are the (half width, half height) of each creature's hurtbox in
        // `assets/tuning/enemies.ron`; a model may overhang a little (spines,
        // claws, tendrils) but must not be visibly bigger than what you hit.
        let meshes: HashMap<_, _> = enemy_meshes().into_iter().collect();
        let width = |names: &[&str]| {
            names
                .iter()
                .map(|n| {
                    let (lo, hi) = meshes[n].bounds();
                    lo.x.abs().max(hi.x.abs()).max(lo.z.abs()).max(hi.z.abs())
                })
                .fold(0.0f32, f32::max)
        };
        // The dominant body part of each: shell, orb, barrel, pod.
        assert!(width(&["husk_shell"]) < 0.5 * 1.35);
        assert!(width(&["wisp_orb"]) < 0.45 * 1.2);
        assert!(width(&["shield_barrel"]) < 0.55 * 1.2);
        assert!(width(&["spitter_pod"]) < 0.5 * 1.2);
    }

    #[test]
    fn every_state_has_a_planned_length_where_it_should() {
        let t = Tuning::default().enemies;
        for kind in [
            EnemyKind::Husk,
            EnemyKind::Wisp,
            EnemyKind::Shieldbearer,
            EnemyKind::Spitter,
        ] {
            assert!(state_len(kind, EnemyState::Windup, &t) > 10.0);
            assert!(state_len(kind, EnemyState::Recover, &t) > 10.0);
            assert_eq!(state_len(kind, EnemyState::Chase, &t), 0.0);
        }
    }
}
