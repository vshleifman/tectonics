//! How hot the crust is at depth: the steady conductive profile through crust
//! that makes some heat of its own,
//!
//! `T(z) = T0 + q z / k - A z^2 / (2 k)`
//!
//! with `q` the heat flow at the surface, `k` the conductivity and `A` the
//! heat the rock produces by radioactive decay per unit volume (Turcotte and
//! Schubert, *Geodynamics*, section 4-12). The heat flow a cell loses is
//! already tracked (`PlanetHeat::heat_flow`), so the same number that cools the
//! planet sets how hot its crust is.

use crate::tuning::Tuning;

/// Temperature (°C) at `depth_km` beneath a surface losing `heat_flow_mw`
/// (mW/m²). Below `q / A` the rock above would make all the heat that leaves
/// the surface, so none comes up from beneath and the profile is flat; it
/// never cools with depth.
pub fn temperature_at(t: &Tuning, heat_flow_mw: f64, depth_km: f64) -> f64 {
    let k = t.crust_conductivity.max(1e-9);
    let q = heat_flow_mw.max(0.0) * 1e-3;
    let a = t.crust_heat_production * 1e-6;
    let mut z = depth_km.max(0.0) * 1e3;
    if a > 0.0 {
        z = z.min(q / a);
    }
    t.surface_temp + q * z / k - a * z * z / (2.0 * k)
}

/// [`temperature_at`], no hotter than the mantle (`mantle_c`, °C): where the
/// conductive profile reaches it, that is the base of the lithosphere, and
/// beneath it the mantle convects and its temperature holds. A brief spike in
/// heat flow does not cook rock buried deep to thousands of degrees.
pub fn temperature_below(t: &Tuning, heat_flow_mw: f64, depth_km: f64, mantle_c: f64) -> f64 {
    temperature_at(t, heat_flow_mw, depth_km).min(mantle_c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_of_ordinary_continent_is_warm_not_molten() {
        let t = Tuning::default();
        let moho = temperature_at(&t, 65.0, 35.0);
        assert!((550.0..750.0).contains(&moho), "Moho at {moho:.0} C");
    }

    #[test]
    fn the_surface_is_at_surface_temperature() {
        let t = Tuning::default();
        assert_eq!(temperature_at(&t, 65.0, 0.0), t.surface_temp);
    }

    #[test]
    fn it_never_cools_with_depth_nor_passes_the_mantle() {
        let t = Tuning::default();
        let turn = 40.0 / t.crust_heat_production;
        let top = temperature_at(&t, 40.0, turn);
        assert_eq!(temperature_at(&t, 40.0, turn + 30.0), top);
        assert!(temperature_at(&t, 40.0, turn - 5.0) < top);
        // A ridge's heat flow, far down: held at the mantle's temperature.
        assert_eq!(temperature_below(&t, 400.0, 60.0, 1330.0), 1330.0);
        assert_eq!(temperature_below(&t, 65.0, 35.0, 1330.0), temperature_at(&t, 65.0, 35.0));
    }

    #[test]
    fn a_hotter_surface_flow_or_deeper_crust_is_hotter_below() {
        let t = Tuning::default();
        assert!(temperature_at(&t, 90.0, 35.0) > temperature_at(&t, 65.0, 35.0));
        assert!(temperature_at(&t, 65.0, 55.0) > temperature_at(&t, 65.0, 35.0));
    }
}
