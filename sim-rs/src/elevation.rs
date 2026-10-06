//! Relief from plate interaction, mirroring `elevation.ts`: boundary forces
//! nudge elevation, then every cell relaxes toward its isostatic baseline.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT, NONE};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::clamp01;
use crate::tuning::Tuning;

/// How far cooling lithosphere of this age has subsided from the ridge crest
/// toward the abyssal plain, in [0, 1]: as the square root of age, as a
/// cooling half-space does, until the plate has cooled through at
/// `seafloor_flattening_age` and the floor flattens out.
pub fn subsidence(t: &Tuning, age: f64) -> f64 {
    (age.max(0.0) / t.seafloor_flattening_age.max(1e-6)).sqrt().min(1.0)
}

/// Depth of the sea floor from cooling alone.
pub fn thermal_elevation(t: &Tuning, age: f64) -> f64 {
    t.ridge_elevation + (t.abyss_elevation - t.ridge_elevation) * subsidence(t, age)
}

/// Isostatic baseline elevation of a cell from its crust alone, by Airy
/// isostasy: a column of crust floats on the mantle by how much lighter than
/// it it is, on top of the height its lithosphere has from cooling. Measured
/// against a column of ordinary ocean crust, so that ocean floor of the
/// standard thickness sits on the cooling curve. Ocean and continent differ
/// only in their density and in how much of the cooling curve they follow, so
/// crust that changes type changes height only by its change of density.
pub fn baseline_elevation(t: &Tuning, crust_type: u8, age: f64, thickness: f64) -> f64 {
    let buoyancy = |h: f64, density: f64| h * (t.mantle_density - density) / t.mantle_density;
    let reference = buoyancy(t.oceanic_thickness, t.oceanic_density_young);
    let (density, thermal) = if crust_type == CONTINENTAL {
        let cooled = t.abyss_elevation;
        let young = thermal_elevation(t, age) - cooled;
        (t.continental_density, cooled + young * t.continent_thermal_share)
    } else {
        (t.oceanic_density_young, thermal_elevation(t, age))
    };
    thermal + buoyancy(thickness, density) - reference
}

pub fn apply_elevation(
    arc_cells: &[i32],
    data: &mut CellData,
    dt_myr: f64,
    boundaries: &BoundaryField,
    t: &Tuning,
) {
    let bump = |e: &mut f32, x: f64| *e = (*e as f64 + x) as f32;
    for a in 0..boundaries.arc_kind.len() {
        let kind = boundaries.arc_kind[a];
        if kind == NONE {
            continue;
        }
        let (ca, cb) = (arc_cells[a * 2], arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (ca, cb) = (ca as usize, cb as usize);
        let drive = (boundaries.arc_closing[a] as f64).abs() * dt_myr;
        if kind == CONVERGENT {
            if data.crust_type[ca] == CONTINENTAL && data.crust_type[cb] == CONTINENTAL {
                let up = t.collision_uplift * drive;
                bump(&mut data.elevation[ca], up);
                bump(&mut data.elevation[cb], up);
            } else {
                let a_denser = data.density[ca] >= data.density[cb];
                let (trench, arc) = if a_denser { (ca, cb) } else { (cb, ca) };
                bump(&mut data.elevation[trench], -t.trench_deepen * drive);
                bump(&mut data.elevation[arc], t.arc_uplift * drive);
            }
        } else if kind == DIVERGENT {
            let pull = t.ridge_pull * dt_myr;
            for c in [ca, cb] {
                let e = data.elevation[c] as f64;
                bump(&mut data.elevation[c], (t.ridge_elevation - e) * pull);
            }
        }
    }

    let relax = clamp01(t.relax_rate * dt_myr);
    for i in 0..data.elevation.len() {
        let base = baseline_elevation(
            t,
            data.crust_type[i],
            data.age[i] as f64,
            data.thickness[i] as f64,
        );
        let e = data.elevation[i] as f64;
        let e = (e + (base - e) * relax).clamp(t.elevation_min, t.elevation_max);
        data.elevation[i] = e as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::OCEANIC;

    #[test]
    fn the_sea_floor_sinks_as_the_square_root_of_its_age_then_flattens() {
        let t = Tuning::default();
        let depth = |age: f64| baseline_elevation(&t, OCEANIC, age, t.oceanic_thickness);
        let relief = t.abyss_elevation - t.ridge_elevation;
        assert_eq!(depth(0.0), t.ridge_elevation);
        let quarter = t.seafloor_flattening_age / 4.0;
        assert!((depth(quarter) - (t.ridge_elevation + 0.5 * relief)).abs() < 1e-9);
        assert_eq!(depth(t.seafloor_flattening_age), t.abyss_elevation);
        assert_eq!(depth(3.0 * t.seafloor_flattening_age), t.abyss_elevation);
        // Most of the sinking is done young.
        assert!(depth(25.0) < -4.0, "25 Myr floor at {:.2} km", depth(25.0));
    }

    #[test]
    fn continents_float_by_their_thickness_and_thin_continent_is_drowned() {
        let t = Tuning::default();
        let old = 1000.0;
        let land = |h: f64| baseline_elevation(&t, CONTINENTAL, old, h);
        assert!((0.0..1.0).contains(&land(40.0)), "40 km continent at {:.2} km", land(40.0));
        assert!(land(30.0) < 0.0, "30 km continent above the sea at {:.2} km", land(30.0));
        assert!(land(60.0) > 3.0, "60 km of crust barely raised: {:.2} km", land(60.0));
        assert!(land(60.0) > land(40.0) && land(40.0) > land(20.0));
        assert!(baseline_elevation(&t, CONTINENTAL, 0.0, 40.0) > land(40.0), "young continent no higher");
    }

    #[test]
    fn crust_turning_continental_changes_height_only_by_its_density() {
        let t = Tuning::default();
        let h = t.continental_thickness_threshold;
        for age in [0.0, 30.0, 1000.0] {
            let ocean = baseline_elevation(&t, OCEANIC, age, h);
            let continent = baseline_elevation(&t, CONTINENTAL, age, h);
            assert!(
                (continent - ocean).abs() < 2.0,
                "{age} Myr: ocean {ocean:.2} km became continent at {continent:.2} km"
            );
        }
        let old = 1000.0;
        let jump = baseline_elevation(&t, CONTINENTAL, old, h) - baseline_elevation(&t, OCEANIC, old, h);
        assert!((0.0..1.5).contains(&jump), "old ocean jumped {jump:.2} km on turning continental");
    }

    #[test]
    fn standard_ocean_crust_sits_on_the_cooling_curve() {
        let t = Tuning::default();
        for age in [0.0, 20.0, 80.0, 300.0] {
            let e = baseline_elevation(&t, OCEANIC, age, t.oceanic_thickness);
            assert!((e - thermal_elevation(&t, age)).abs() < 1e-12);
        }
        let plateau = baseline_elevation(&t, OCEANIC, 80.0, t.oceanic_thickness + 10.0);
        assert!(plateau > thermal_elevation(&t, 80.0) + 1.0, "thickened ocean floor not raised");
    }
}
