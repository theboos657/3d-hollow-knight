//! Graphics tiers. What each costs and buys:
//!
//! * **Low**: no shadows, FXAA. For weak or integrated GPUs.
//! * **Medium** (default): key-light shadows (2048), SMAA.
//! * **High**: bigger shadows (4096), sharper SMAA and screen-space ambient
//!   occlusion.
//!
//! The choice comes from the options menu (`Settings::quality`) or the
//! `--quality low|medium|high` flag, which wins for that run.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::light::DirectionalLightShadowMap;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::prelude::*;

use super::KeyLight;
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
    pub aa: Aa,
    pub ssao: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aa {
    Fxaa,
    Smaa(SmaaLevel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmaaLevel {
    Medium,
    High,
}

pub fn plan(q: Quality) -> Plan {
    match q {
        Quality::Low => Plan {
            shadows: false,
            shadow_map: 1024,
            aa: Aa::Fxaa,
            ssao: false,
        },
        Quality::Medium => Plan {
            shadows: true,
            shadow_map: 2048,
            aa: Aa::Smaa(SmaaLevel::Medium),
            ssao: false,
        },
        Quality::High => Plan {
            shadows: true,
            shadow_map: 4096,
            aa: Aa::Smaa(SmaaLevel::High),
            ssao: true,
        },
    }
}

pub fn apply_quality(
    settings: Res<Settings>,
    forced: Res<QualityOverride>,
    mut commands: Commands,
    cam: Query<Entity, With<MainCamera>>,
    mut key: Query<&mut DirectionalLight, With<KeyLight>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    fresh: Query<(), Added<MainCamera>>,
) {
    if !settings.is_changed() && !forced.is_changed() && fresh.is_empty() {
        return;
    }
    let q = forced.0.unwrap_or(settings.quality);
    let p = plan(q);
    shadow_map.size = p.shadow_map as usize;
    for mut l in &mut key {
        l.shadows_enabled = p.shadows;
    }
    for e in &cam {
        // Multisampling is off for every tier: SMAA/FXAA replace it (and SSAO
        // needs it off).
        let mut c = commands.entity(e);
        c.insert(Msaa::Off);
        match p.aa {
            Aa::Fxaa => {
                c.remove::<Smaa>().insert(Fxaa::default());
            }
            Aa::Smaa(level) => {
                c.remove::<Fxaa>().insert(Smaa {
                    preset: match level {
                        SmaaLevel::Medium => SmaaPreset::Medium,
                        SmaaLevel::High => SmaaPreset::High,
                    },
                });
            }
        }
        if p.ssao {
            c.insert(ScreenSpaceAmbientOcclusion::default());
        } else {
            c.remove::<ScreenSpaceAmbientOcclusion>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tier_buys_more_than_the_last() {
        let (l, m, h) = (
            plan(Quality::Low),
            plan(Quality::Medium),
            plan(Quality::High),
        );
        assert!(!l.shadows && m.shadows && h.shadows);
        assert!(l.shadow_map < m.shadow_map && m.shadow_map < h.shadow_map);
        assert!(!l.ssao && !m.ssao && h.ssao);
        assert_eq!(l.aa, Aa::Fxaa);
        assert!(matches!(m.aa, Aa::Smaa(SmaaLevel::Medium)));
        assert!(matches!(h.aa, Aa::Smaa(SmaaLevel::High)));
    }

    #[test]
    fn the_default_tier_is_medium() {
        assert_eq!(Quality::default(), Quality::Medium);
        assert_eq!(Settings::default().quality, Quality::Medium);
    }
}
