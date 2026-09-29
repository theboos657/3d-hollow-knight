//! Graphics tiers. What each costs and buys:
//!
//! * **Low**: no shadows, FXAA. For weak or integrated GPUs.
//! * **Medium**: key-light shadows (2048), SMAA, parallax-mapped stone.
//! * **High**: bigger shadows (4096), sharper SMAA, ambient occlusion and
//!   image-based light (reflections and ambient colour from a generated
//!   environment map).
//! * **Ultra** (default): 8192 soft shadows, temporal anti-aliasing with
//!   sharpening, top-quality ambient occlusion, volumetric haze that lights
//!   fires and the lantern as glowing halos, depth of field, a touch of lens
//!   fringing and film grain.
//!
//! The choice comes from the options menu (`Settings::quality`) or the
//! `--quality low|medium|high|ultra` flag, which wins for that run. Every
//! feature is an independent flag in [`Plan`], so a tier is just a table.

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::light::{
    DirectionalLightShadowMap, ShadowFilteringMethod, VolumetricFog, VolumetricLight,
};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};

use super::{KeyLight, NoHalo};
use crate::scene::MainCamera;
use crate::settings::{Quality, Settings};

/// `--quality` on the command line.
#[derive(Resource, Default)]
pub struct QualityOverride(pub Option<Quality>);

/// What a tier turns on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub shadows: bool,
    pub shadow_map: u32,
    /// Shadow edges are softened over time (needs temporal AA to resolve).
    pub soft_shadows: bool,
    pub aa: Aa,
    /// Sharpening after temporal AA, which softens a little.
    pub sharpen: bool,
    pub ssao: Ssao,
    /// Parallax-mapped stone (relief that shifts as you move past it).
    pub parallax: bool,
    /// Image-based light from a generated environment map.
    pub ibl: bool,
    /// Volumetric haze, with lights glowing in it.
    pub volumetric: bool,
    pub dof: bool,
    /// Colour fringing toward the frame's edges.
    pub aberration: bool,
    pub grain: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aa {
    Fxaa,
    Smaa(SmaaLevel),
    /// Temporal anti-aliasing.
    Taa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmaaLevel {
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ssao {
    Off,
    High,
    Ultra,
}

pub fn plan(q: Quality) -> Plan {
    let off = Plan {
        shadows: false,
        shadow_map: 1024,
        soft_shadows: false,
        aa: Aa::Fxaa,
        sharpen: false,
        ssao: Ssao::Off,
        parallax: false,
        ibl: false,
        volumetric: false,
        dof: false,
        aberration: false,
        grain: false,
    };
    match q {
        Quality::Low => off,
        Quality::Medium => Plan {
            shadows: true,
            shadow_map: 2048,
            aa: Aa::Smaa(SmaaLevel::Medium),
            parallax: true,
            ..off
        },
        Quality::High => Plan {
            shadows: true,
            shadow_map: 4096,
            aa: Aa::Smaa(SmaaLevel::High),
            ssao: Ssao::High,
            parallax: true,
            ibl: true,
            ..off
        },
        Quality::Ultra => Plan {
            shadows: true,
            shadow_map: 8192,
            soft_shadows: true,
            aa: Aa::Taa,
            sharpen: true,
            ssao: Ssao::Ultra,
            parallax: true,
            ibl: true,
            volumetric: true,
            dof: true,
            aberration: true,
            grain: true,
        },
    }
}

/// The tier in force: the `--quality` flag wins over the options menu.
pub fn current(settings: &Settings, forced: &QualityOverride) -> Quality {
    forced.0.unwrap_or(settings.quality)
}

/// The plan for the tier in force, kept up to date.
#[derive(Resource, Clone, Copy)]
pub struct CurrentPlan(pub Plan);

impl Default for CurrentPlan {
    fn default() -> Self {
        CurrentPlan(plan(Quality::default()))
    }
}

fn resolve_plan(
    settings: Res<Settings>,
    forced: Res<QualityOverride>,
    mut now: ResMut<CurrentPlan>,
) {
    let want = plan(current(&settings, &forced));
    if now.0 != want {
        now.0 = want;
    }
}

/// Puts on the camera and lights exactly what the plan asks for, and takes
/// off what it no longer asks for.
pub fn apply_plan(
    plan: Res<CurrentPlan>,
    mut commands: Commands,
    cam: Query<Entity, With<MainCamera>>,
    fresh: Query<(), Added<MainCamera>>,
    mut key: Query<(Entity, &mut DirectionalLight), With<KeyLight>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
) {
    if !plan.is_changed() && fresh.is_empty() {
        return;
    }
    let p = plan.0;
    shadow_map.size = p.shadow_map as usize;
    for (e, mut l) in &mut key {
        l.shadows_enabled = p.shadows;
        // Light shafts need the shadow map, so the volumetric flag only
        // sticks where there is one.
        if p.volumetric && p.shadows {
            commands.entity(e).insert(VolumetricLight);
        } else {
            commands.entity(e).remove::<VolumetricLight>();
        }
    }
    for e in &cam {
        let mut c = commands.entity(e);
        // Multisampling is off for every tier: the post-process AA replaces it
        // (and SSAO and TAA need it off).
        c.insert(Msaa::Off);
        c.remove::<(Fxaa, Smaa, TemporalAntiAliasing, TemporalJitter, MipBias)>();
        c.remove::<ContrastAdaptiveSharpening>();
        match p.aa {
            Aa::Fxaa => {
                c.insert(Fxaa::default());
            }
            Aa::Smaa(level) => {
                c.insert(Smaa {
                    preset: match level {
                        SmaaLevel::Medium => SmaaPreset::Medium,
                        SmaaLevel::High => SmaaPreset::High,
                    },
                });
            }
            Aa::Taa => {
                c.insert(TemporalAntiAliasing::default());
            }
        }
        if p.sharpen {
            c.insert(ContrastAdaptiveSharpening {
                enabled: true,
                sharpening_strength: 0.45,
                denoise: false,
            });
        }
        c.insert(if p.soft_shadows {
            ShadowFilteringMethod::Temporal
        } else {
            ShadowFilteringMethod::Gaussian
        });
        match p.ssao {
            Ssao::Off => {
                c.remove::<ScreenSpaceAmbientOcclusion>();
            }
            level => {
                c.insert(ScreenSpaceAmbientOcclusion {
                    quality_level: if level == Ssao::Ultra {
                        ScreenSpaceAmbientOcclusionQualityLevel::Ultra
                    } else {
                        ScreenSpaceAmbientOcclusionQualityLevel::High
                    },
                    constant_object_thickness: 0.4,
                });
            }
        }
        if p.volumetric {
            c.insert(VolumetricFog {
                // The environment light already supplies ambient colour; the
                // haze only needs a whisper of it.
                ambient_color: Color::srgb(0.6, 0.65, 0.8),
                ambient_intensity: 0.0,
                jitter: 0.5,
                step_count: 48,
            });
        } else {
            c.remove::<VolumetricFog>();
        }
        if p.dof {
            c.insert(depth_of_field(crate::scene::CAM_DIST));
        } else {
            c.remove::<DepthOfField>();
        }
        if p.aberration {
            c.insert(ChromaticAberration {
                color_lut: None,
                intensity: 0.005,
                max_samples: 8,
            });
        } else {
            c.remove::<ChromaticAberration>();
        }
    }
}

/// A lens focused on the play lane, `focus` units from the camera. The values
/// are chosen by eye: the world is a few dozen units across, so a physical
/// lens would blur nothing; this one lets the far wall and the near tufts
/// soften while the knight stays sharp.
pub fn depth_of_field(focus: f32) -> DepthOfField {
    DepthOfField {
        mode: DepthOfFieldMode::Bokeh,
        focal_distance: focus,
        sensor_height: 0.45,
        aperture_f_stops: 1.4,
        max_circle_of_confusion_diameter: 12.0,
        max_depth: 90.0,
    }
}

/// Keeps the lens focused on the play lane as the camera moves in and out.
fn focus_lens(mut cam: Query<(&Transform, &mut DepthOfField), With<MainCamera>>) {
    for (t, mut dof) in &mut cam {
        // The play lane is the z = 0 plane.
        let focus = t.translation.z.max(4.0);
        if (dof.focal_distance - focus).abs() > 1e-3 {
            dof.focal_distance = focus;
        }
    }
}

/// Every light in the scene glows in the haze on tiers that have it (fires,
/// the lantern, bolts); tiers that do not have it strip the glow again.
fn halo_lights(
    plan: Res<CurrentPlan>,
    mut commands: Commands,
    plain: Query<Entity, (With<PointLight>, Without<VolumetricLight>, Without<NoHalo>)>,
    haloed: Query<Entity, (With<PointLight>, With<VolumetricLight>)>,
) {
    if plan.0.volumetric {
        for e in &plain {
            commands.entity(e).insert(VolumetricLight);
        }
    } else if plan.is_changed() {
        for e in &haloed {
            commands.entity(e).remove::<VolumetricLight>();
        }
    }
}

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QualityOverride>()
            .init_resource::<CurrentPlan>()
            .add_systems(
                Update,
                (resolve_plan, apply_plan, focus_lens, halo_lights).chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tier_buys_more_than_the_last() {
        let tiers = Quality::ALL.map(plan);
        for pair in tiers.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            // Never a feature that a higher tier drops.
            assert!(hi.shadows >= lo.shadows);
            assert!(hi.shadow_map >= lo.shadow_map);
            assert!(hi.soft_shadows >= lo.soft_shadows);
            assert!(hi.ssao >= lo.ssao);
            assert!(hi.parallax >= lo.parallax);
            assert!(hi.ibl >= lo.ibl);
            assert!(hi.volumetric >= lo.volumetric);
            assert!(hi.dof >= lo.dof);
            assert!(hi.aberration >= lo.aberration);
            assert!(hi.grain >= lo.grain);
            assert!(hi != lo, "adjacent tiers differ");
        }
        let [low, medium, high, ultra] = tiers;
        assert!(!low.shadows && medium.shadows);
        assert!(low.shadow_map < medium.shadow_map && medium.shadow_map < high.shadow_map);
        assert!(high.shadow_map < ultra.shadow_map);
        assert_eq!(low.ssao, Ssao::Off);
        assert_eq!(medium.ssao, Ssao::Off);
        assert_eq!(high.ssao, Ssao::High);
        assert_eq!(ultra.ssao, Ssao::Ultra);
        assert!(!low.parallax && medium.parallax);
        assert_eq!(low.aa, Aa::Fxaa);
        assert!(matches!(medium.aa, Aa::Smaa(SmaaLevel::Medium)));
        assert!(matches!(high.aa, Aa::Smaa(SmaaLevel::High)));
        assert_eq!(ultra.aa, Aa::Taa);
        assert!(!high.volumetric && ultra.volumetric);
    }

    #[test]
    fn soft_shadows_and_sharpening_only_come_with_temporal_aa() {
        for q in Quality::ALL {
            let p = plan(q);
            if p.soft_shadows || p.sharpen {
                assert_eq!(p.aa, Aa::Taa, "{q:?}: temporal shadows need TAA to resolve");
            }
        }
    }

    #[test]
    fn the_default_tier_is_ultra() {
        assert_eq!(Quality::default(), Quality::Ultra);
        assert_eq!(Settings::default().quality, Quality::Ultra);
        assert_eq!(CurrentPlan::default().0, plan(Quality::Ultra));
    }

    #[test]
    fn the_flag_beats_the_menu() {
        let s = Settings {
            quality: Quality::Low,
            ..Settings::default()
        };
        assert_eq!(current(&s, &QualityOverride(None)), Quality::Low);
        assert_eq!(
            current(&s, &QualityOverride(Some(Quality::High))),
            Quality::High
        );
    }
}
