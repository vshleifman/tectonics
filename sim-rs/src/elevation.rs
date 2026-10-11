//! Relief from plate interaction, mirroring `elevation.ts`: boundary forces
//! nudge elevation, then every cell relaxes toward its isostatic baseline,
//! raised or lowered by the mantle flowing beneath it.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT, NONE};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::clamp01;
use crate::par;
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
    let reference = buoyancy(t.oceanic_thickness, t.oceanic_density);
    let (density, thermal) = if crust_type == CONTINENTAL {
        let cooled = t.abyss_elevation;
        let young = thermal_elevation(t, age) - cooled;
        (t.continental_density, cooled + young * t.continent_thermal_share)
    } else {
        (t.oceanic_density, thermal_elevation(t, age))
    };
    thermal + buoyancy(thickness, density) - reference
}

/// Height (km) the flowing mantle holds the surface at over upper mantle
/// `temp` K hotter than the planet's and a downwelling taking in `sink` more
/// of the area over it per Myr than the globe's mean (less over the gentle
/// upwelling that balances it): hot mantle rising buoys the surface up and
/// sinking slab drags it down, over regions thousands of km across, by up to
/// `dyn_topo_max` either way. It sits under the crust, not in it, so it stays
/// put as plates move over it.
pub fn dynamic_topography(t: &Tuning, temp: f64, sink: f64) -> f64 {
    let limit = t.dyn_topo_max.max(0.0);
    (t.dyn_topo_gain * temp - t.sink_topo_gain * sink).clamp(-limit, limit)
}

/// Density of the mantle lid under crust with this keel: mantle cooled by
/// half the drop across the lid, so denser by thermal contraction, less the
/// depletion of a keel's melt residue, which leaves a full keel about as
/// dense as the mantle it sits in.
pub fn lid_density(t: &Tuning, keel: f64) -> f64 {
    t.mantle_density * (1.0 + t.thermal_expansion * t.lid_delta_t / 2.0) - keel * t.keel_depletion
}

/// Buoyancy (km of mantle displaced) of a column of `thickness` km of crust
/// of `density` over `lithosphere` km of lid: what holds it up, less the
/// weight of its cold lid. Below zero the column is heavier than the mantle
/// and sinks.
pub fn buoyancy(t: &Tuning, thickness: f64, density: f64, lithosphere: f64, keel: f64) -> f64 {
    let crust = thickness * (t.mantle_density - density);
    let lid = lithosphere * (t.mantle_density - lid_density(t, keel));
    (crust + lid) / t.mantle_density.max(1e-9)
}

/// Buoyancy of cell `i`'s whole column, crust and lid.
pub fn column_buoyancy(t: &Tuning, data: &CellData, i: usize) -> f64 {
    buoyancy(
        t,
        data.thickness[i] as f64,
        data.density[i] as f64,
        data.lithosphere[i] as f64,
        data.keel[i] as f64,
    )
}

/// Nudge relief at boundaries, then relax every cell toward its isostatic
/// baseline raised or lowered by `dyn_topo`, the dynamic topography (km)
/// beneath it.
pub fn apply_elevation(
    arc_cells: &[i32],
    data: &mut CellData,
    dyn_topo: &[f32],
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
                // Continent never goes down; between two oceans the heavier
                // column does.
                let a_sinks = data.crust_type[cb] == CONTINENTAL
                    || (data.crust_type[ca] != CONTINENTAL
                        && column_buoyancy(t, data, ca) <= column_buoyancy(t, data, cb));
                let (trench, arc) = if a_sinks { (ca, cb) } else { (cb, ca) };
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
    let (crust_type, age, thickness) = (&data.crust_type, &data.age, &data.thickness);
    par::for_each_mut(&mut data.elevation, |i, elevation| {
        let base = baseline_elevation(t, crust_type[i], age[i] as f64, thickness[i] as f64)
            + dyn_topo[i] as f64;
        let e = *elevation as f64;
        let e = (e + (base - e) * relax).clamp(t.elevation_min, t.elevation_max);
        *elevation = e as f32;
    });
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
    fn hot_mantle_lifts_the_surface_and_sinking_slab_drags_it_down_within_the_clamp() {
        let t = Tuning::default();
        assert!(dynamic_topography(&t, 50.0, 0.0) > 0.0);
        assert!(dynamic_topography(&t, 0.0, 0.01) < 0.0);
        assert!(dynamic_topography(&t, -50.0, 0.0) < 0.0);
        assert!(dynamic_topography(&t, 0.0, -0.01) > 0.0, "upwelling between sinks not raised");
        assert_eq!(dynamic_topography(&t, 0.0, 0.0), 0.0);
        assert_eq!(dynamic_topography(&t, 1e6, 0.0), t.dyn_topo_max);
        assert_eq!(dynamic_topography(&t, -1e6, 1.0), -t.dyn_topo_max);
        let off = Tuning {
            dyn_topo_gain: 0.0,
            sink_topo_gain: 0.0,
            ..Tuning::default()
        };
        assert_eq!(dynamic_topography(&off, 300.0, 0.05), 0.0);
    }

    #[test]
    fn elevation_relaxes_toward_the_baseline_plus_dynamic_topography() {
        let t = Tuning::default();
        let n = 4;
        let relax_for = |dyn_topo: &[f32]| {
            let mut data = CellData::new(n);
            for i in 0..n {
                data.thickness[i] = t.oceanic_thickness as f32;
                data.age[i] = 80.0;
            }
            let boundaries = BoundaryField::new(n, 0);
            for _ in 0..2000 {
                apply_elevation(&[], &mut data, dyn_topo, t.dt_myr, &boundaries, &t);
            }
            data.elevation
        };
        let base = baseline_elevation(&t, OCEANIC, 80.0, t.oceanic_thickness);
        let flat = relax_for(&[0.0; 4]);
        let lifted = relax_for(&[1.0, -1.0, 0.5, 0.0]);
        for i in 0..n {
            assert!((flat[i] as f64 - base).abs() < 1e-3, "cell {i} settled at {}", flat[i]);
        }
        for (i, offset) in [1.0, -1.0, 0.5, 0.0].into_iter().enumerate() {
            let got = lifted[i] as f64 - base;
            assert!((got - offset).abs() < 1e-3, "cell {i} lifted {got:.3} km, not {offset}");
        }
        assert_eq!(lifted[3], flat[3]);
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
