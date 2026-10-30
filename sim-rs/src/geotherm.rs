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
/// (mW/m²).
pub fn temperature_at(t: &Tuning, heat_flow_mw: f64, depth_km: f64) -> f64 {
    let z = depth_km.max(0.0) * 1e3;
    let k = t.crust_conductivity.max(1e-9);
    let q = heat_flow_mw * 1e-3;
    let a = t.crust_heat_production * 1e-6;
    t.surface_temp + q * z / k - a * z * z / (2.0 * k)
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
    fn a_hotter_surface_flow_or_deeper_crust_is_hotter_below() {
        let t = Tuning::default();
        assert!(temperature_at(&t, 90.0, 35.0) > temperature_at(&t, 65.0, 35.0));
        assert!(temperature_at(&t, 65.0, 55.0) > temperature_at(&t, 65.0, 35.0));
    }
}
