//! What the rock now at the surface has been through. Erosion exposes rock
//! that once lay `exhumed` km down, where the weight above it set its
//! pressure and the geotherm its temperature; the hottest and the most
//! squeezed it has been are kept, and the metamorphic facies is read off that
//! peak.
//!
//! The temperature uses the heat flow at the time of exhumation, not at the
//! time of burial. Heat flow changes over tens of Myr, so this is close for
//! one orogen and drifts for rock exhumed long after it was buried.

use crate::cell_data::{CellData, LANE_PEAK_PRESSURE, LANE_PEAK_TEMP};
use crate::geotherm::temperature_below;
use crate::tuning::Tuning;

/// Pressure (kbar) per km of rock of density 1 g/cm³: `rho g z`, with
/// g = 9.81 m/s² and 1 kbar = 10⁸ Pa.
const KBAR_PER_KM_PER_DENSITY: f64 = 0.0981;

/// Pressure (kbar) `depth_km` down through crust of `density` (g/cm³).
pub fn pressure_at(density: f64, depth_km: f64) -> f64 {
    KBAR_PER_KM_PER_DENSITY * density.max(0.0) * depth_km.max(0.0)
}

/// Raise the peaks of cell `i` to what it feels `depth_km` down, if that is
/// more, over a mantle at `mantle_c` °C.
pub fn raise_peaks(
    data: &mut CellData,
    i: usize,
    heat_flow_mw: f64,
    depth_km: f64,
    mantle_c: f64,
    t: &Tuning,
) {
    let temp = temperature_below(t, heat_flow_mw, depth_km, mantle_c);
    let pressure = pressure_at(data.density[i] as f64, depth_km);
    let slot = &mut data.lanes.lane_mut(LANE_PEAK_TEMP)[i];
    *slot = slot.max(temp as f32);
    let slot = &mut data.lanes.lane_mut(LANE_PEAK_PRESSURE)[i];
    *slot = slot.max(pressure as f32);
}

/// Metamorphic facies, numbered as they are in the `facies_ptr` view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Facies {
    /// Never buried deep enough to change.
    None = 0,
    Zeolite = 1,
    Greenschist = 2,
    Amphibolite = 3,
    Granulite = 4,
    Blueschist = 5,
    Eclogite = 6,
}

/// How many facies there are, `None` included.
pub const FACIES: usize = 7;

/// The facies of rock whose peak was `temp` °C and `pressure` kbar. The
/// boundaries are simplified from the petrogenetic grids in Spear (1993),
/// *Metamorphic Phase Equilibria and Pressure-Temperature-Time Paths*, and
/// Winter (2010), *Principles of Igneous and Metamorphic Petrology*, ch. 25.
pub fn facies(t: &Tuning, temp: f64, pressure: f64) -> Facies {
    if temp < t.facies_zeolite_temp {
        return Facies::None;
    }
    // High pressure at low temperature: the cold geotherm of a subduction zone.
    if pressure >= t.facies_blueschist_pressure && temp < t.facies_blueschist_max_temp {
        return Facies::Blueschist;
    }
    if pressure >= t.facies_eclogite_pressure {
        return Facies::Eclogite;
    }
    if temp >= t.facies_granulite_temp {
        Facies::Granulite
    } else if temp >= t.facies_amphibolite_temp {
        Facies::Amphibolite
    } else if temp >= t.facies_greenschist_temp {
        Facies::Greenschist
    } else {
        Facies::Zeolite
    }
}

/// The facies of every cell, from its peaks.
pub fn classify_facies(data: &CellData, t: &Tuning, out: &mut [u8]) {
    let temps = data.lanes.lane(LANE_PEAK_TEMP);
    let pressures = data.lanes.lane(LANE_PEAK_PRESSURE);
    crate::par::fill(out, |i| facies(t, temps[i] as f64, pressures[i] as f64) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facies_change_at_their_boundaries() {
        let t = Tuning::default();
        let at = |temp: f64, pressure: f64| facies(&t, temp, pressure);
        assert_eq!(at(t.facies_zeolite_temp - 1.0, 2.0), Facies::None);
        assert_eq!(at(t.facies_zeolite_temp, 2.0), Facies::Zeolite);
        assert_eq!(at(t.facies_greenschist_temp - 1.0, 2.0), Facies::Zeolite);
        assert_eq!(at(t.facies_greenschist_temp, 2.0), Facies::Greenschist);
        assert_eq!(at(t.facies_amphibolite_temp, 2.0), Facies::Amphibolite);
        assert_eq!(at(t.facies_granulite_temp - 1.0, 2.0), Facies::Amphibolite);
        assert_eq!(at(t.facies_granulite_temp, 2.0), Facies::Granulite);
        // Pressure at a cold geotherm is blueschist; hot, eclogite.
        let p = t.facies_blueschist_pressure;
        assert_eq!(at(300.0, p), Facies::Blueschist);
        assert_eq!(at(300.0, p - 0.1), Facies::Greenschist);
        assert_eq!(at(t.facies_blueschist_max_temp, p), Facies::Greenschist);
        assert_eq!(at(600.0, t.facies_eclogite_pressure), Facies::Eclogite);
        assert_eq!(at(600.0, t.facies_eclogite_pressure - 0.1), Facies::Amphibolite);
    }

    #[test]
    fn a_kilometre_of_crust_weighs_about_a_quarter_of_a_kilobar() {
        let p = pressure_at(2.8, 1.0);
        assert!((0.25..0.30).contains(&p), "{p} kbar per km");
    }
}
