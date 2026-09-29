//! The look of each area: one place that says what colour its stone is, how
//! its light falls, what its fires burn and how it is graded.

use bevy::prelude::*;
use hk_sim::world::room::Theme;

/// Everything that differs between areas.
#[derive(Clone, Copy, Debug)]
pub struct LookStyle {
    /// Sunlight-through-a-window key light.
    pub key: Color,
    pub key_lux: f32,
    /// A weak cool light from behind, to separate silhouettes from the wall.
    pub rim: Color,
    pub ambient: Color,
    pub ambient_brightness: f32,
    pub fog: Color,
    pub stone: Color,
    /// The lit lip along every walkable surface.
    pub cap: Color,
    /// The chamber wall behind the play lane.
    pub wall: Color,
    pub one_way: Color,
    /// How rough the stone is, as a multiplier on its map (wet stone is low).
    pub roughness: f32,
    /// Clearcoat on the stone: the wet sheen on damp and polished rock.
    pub wet: f32,
    /// Whether the lip along floors is moss rather than bare stone.
    pub moss: bool,
    /// Dust motes, fireflies.
    pub mote: Color,
    pub mote_emissive: LinearRgba,
    /// Braziers and lanterns: emissive flame and the light it throws.
    pub flame: LinearRgba,
    pub flame_light: Color,
    /// Colour grading: warmth (+ warm, - cool), saturation and contrast.
    pub temperature: f32,
    pub saturation: f32,
    pub contrast: f32,
}

pub fn style(t: Theme) -> LookStyle {
    let c = Color::srgb;
    let e = LinearRgba::rgb;
    match t {
        Theme::Sandbox => LookStyle {
            key: c(0.85, 0.9, 1.0),
            key_lux: 5200.0,
            rim: c(0.5, 0.6, 1.0),
            ambient: c(0.45, 0.5, 0.7),
            ambient_brightness: 260.0,
            fog: c(0.02, 0.025, 0.045),
            stone: c(0.36, 0.40, 0.50),
            cap: c(0.62, 0.68, 0.78),
            wall: c(0.10, 0.115, 0.19),
            one_way: c(0.55, 0.46, 0.30),
            roughness: 0.95,
            wet: 0.0,
            moss: false,
            mote: c(0.2, 0.6, 0.9),
            mote_emissive: e(0.6, 2.4, 4.0),
            flame: e(1.0, 2.6, 4.0),
            flame_light: c(0.5, 0.75, 1.0),
            temperature: 0.0,
            saturation: 1.05,
            contrast: 1.05,
        },
        Theme::Ashen => LookStyle {
            key: c(1.0, 0.80, 0.58),
            key_lux: 5600.0,
            rim: c(0.55, 0.62, 0.9),
            ambient: c(0.48, 0.48, 0.68),
            ambient_brightness: 250.0,
            fog: c(0.05, 0.035, 0.03),
            stone: c(0.42, 0.37, 0.34),
            cap: c(0.72, 0.62, 0.50),
            wall: c(0.12, 0.105, 0.125),
            one_way: c(0.60, 0.45, 0.28),
            roughness: 1.0,
            wet: 0.0,
            moss: false,
            mote: c(0.9, 0.6, 0.3),
            mote_emissive: e(3.0, 1.6, 0.5),
            flame: e(3.6, 1.7, 0.45),
            flame_light: c(1.0, 0.62, 0.30),
            temperature: 0.06,
            saturation: 1.08,
            contrast: 1.08,
        },
        Theme::Warrens => LookStyle {
            key: c(0.75, 1.0, 0.78),
            key_lux: 5000.0,
            rim: c(0.5, 0.9, 0.9),
            ambient: c(0.38, 0.60, 0.62),
            ambient_brightness: 250.0,
            fog: c(0.01, 0.035, 0.022),
            stone: c(0.28, 0.42, 0.33),
            cap: c(0.34, 0.56, 0.32),
            wall: c(0.045, 0.10, 0.08),
            one_way: c(0.50, 0.42, 0.24),
            roughness: 0.9,
            wet: 0.12,
            moss: true,
            mote: c(0.4, 0.9, 0.5),
            mote_emissive: e(1.0, 3.0, 1.2),
            flame: e(1.0, 3.2, 1.2),
            flame_light: c(0.5, 1.0, 0.55),
            temperature: -0.03,
            saturation: 1.15,
            contrast: 1.06,
        },
        Theme::Cistern => LookStyle {
            key: c(0.72, 0.92, 1.0),
            key_lux: 5200.0,
            rim: c(0.6, 0.8, 1.0),
            ambient: c(0.42, 0.62, 0.78),
            ambient_brightness: 270.0,
            fog: c(0.01, 0.035, 0.06),
            stone: c(0.28, 0.40, 0.50),
            cap: c(0.46, 0.66, 0.76),
            wall: c(0.04, 0.085, 0.14),
            one_way: c(0.42, 0.44, 0.38),
            roughness: 0.62,
            wet: 0.35,
            moss: false,
            mote: c(0.3, 0.8, 0.9),
            mote_emissive: e(0.6, 2.8, 3.4),
            flame: e(0.6, 2.6, 3.4),
            flame_light: c(0.45, 0.85, 1.0),
            temperature: -0.08,
            saturation: 1.12,
            contrast: 1.06,
        },
        Theme::Spire => LookStyle {
            key: c(1.0, 0.88, 0.72),
            key_lux: 5400.0,
            rim: c(0.7, 0.6, 1.0),
            ambient: c(0.58, 0.52, 0.78),
            ambient_brightness: 260.0,
            fog: c(0.04, 0.025, 0.06),
            stone: c(0.40, 0.35, 0.50),
            cap: c(0.80, 0.70, 0.88),
            wall: c(0.085, 0.06, 0.14),
            one_way: c(0.58, 0.46, 0.30),
            roughness: 0.72,
            wet: 0.2,
            moss: false,
            mote: c(0.95, 0.8, 0.45),
            mote_emissive: e(3.0, 2.2, 0.8),
            flame: e(3.4, 2.4, 0.8),
            flame_light: c(1.0, 0.82, 0.45),
            temperature: 0.02,
            saturation: 1.12,
            contrast: 1.08,
        },
        Theme::Throne => LookStyle {
            key: c(1.0, 0.62, 0.52),
            key_lux: 5200.0,
            rim: c(0.8, 0.4, 0.5),
            ambient: c(0.66, 0.40, 0.56),
            ambient_brightness: 240.0,
            fog: c(0.06, 0.012, 0.018),
            stone: c(0.44, 0.22, 0.24),
            cap: c(0.62, 0.34, 0.30),
            wall: c(0.11, 0.04, 0.06),
            one_way: c(0.55, 0.32, 0.24),
            roughness: 0.95,
            wet: 0.0,
            moss: false,
            mote: c(0.95, 0.35, 0.3),
            mote_emissive: e(3.5, 0.6, 0.5),
            flame: e(3.8, 0.9, 0.4),
            flame_light: c(1.0, 0.45, 0.30),
            temperature: 0.08,
            saturation: 1.15,
            contrast: 1.1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEMES: [Theme; 6] = [
        Theme::Sandbox,
        Theme::Ashen,
        Theme::Warrens,
        Theme::Cistern,
        Theme::Spire,
        Theme::Throne,
    ];

    fn luma(c: Color) -> f32 {
        let l = c.to_linear();
        0.2126 * l.red + 0.7152 * l.green + 0.0722 * l.blue
    }

    #[test]
    fn every_area_has_a_readable_look() {
        for t in THEMES {
            let s = style(t);
            // The lip is lighter than the stone, which is lighter than the wall:
            // that ordering is what makes ledges readable.
            assert!(luma(s.cap) > luma(s.stone) * 1.3, "{t:?}: lip vs stone");
            assert!(luma(s.stone) > luma(s.wall) * 1.5, "{t:?}: stone vs wall");
            assert!(s.key_lux > 1000.0 && s.ambient_brightness > 100.0);
            assert!((0.8..1.3).contains(&s.saturation) && (0.9..1.3).contains(&s.contrast));
        }
    }

    #[test]
    fn wet_areas_shine_and_dry_ones_do_not() {
        let (cistern, ashen) = (style(Theme::Cistern), style(Theme::Ashen));
        assert!(cistern.wet > 0.2 && cistern.roughness < ashen.roughness);
        assert_eq!(ashen.wet, 0.0);
        for t in THEMES {
            let s = style(t);
            assert!((0.4..=1.0).contains(&s.roughness) && (0.0..=0.6).contains(&s.wet));
        }
        assert!(style(Theme::Warrens).moss && !style(Theme::Throne).moss);
    }

    #[test]
    fn the_areas_are_told_apart_by_colour() {
        // Their stone colours must differ, so you know where you are at a glance.
        for (i, a) in THEMES.iter().enumerate().skip(1) {
            for b in THEMES.iter().skip(i + 1) {
                let (x, y) = (style(*a).stone.to_srgba(), style(*b).stone.to_srgba());
                let d = (x.red - y.red).abs() + (x.green - y.green).abs() + (x.blue - y.blue).abs();
                assert!(d > 0.12, "{a:?} and {b:?} look too alike");
            }
        }
    }
}
