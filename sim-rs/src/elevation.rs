//! Relief from what is under each cell: the height of its column of crust and
//! lid floating on the mantle, lifted or dragged by the mantle flowing beneath
//! it, deepened over a trench, and loaded by the water above it.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::CellData;
use crate::mesh::Mesh;
use crate::par;
use crate::slabs::subduction;
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

/// How much further a column sinks under water than it would in air, per km
/// of height lost: the mantle displaced by the same load, less the water on it.
pub fn water_load_factor(t: &Tuning) -> f64 {
    t.mantle_density / (t.mantle_density - t.water_density).max(1e-9)
}

/// Height (km) in air, above the datum, of a column of `thickness` km of crust
/// of `density` over `lithosphere` km of lid with this `keel`, lifted by
/// `dyn_topo`, before any water loads it. Measured against a ridge crest, a
/// column of standard ocean crust with no lid, which stands at `ridge_elevation`
/// under a sea at the datum. Uses [`buoyancy`], so the map and the dynamics
/// agree on what floats.
pub fn air_height(
    t: &Tuning,
    thickness: f64,
    density: f64,
    lithosphere: f64,
    keel: f64,
    dyn_topo: f64,
) -> f64 {
    let ridge = buoyancy(t, t.oceanic_thickness, t.oceanic_density, 0.0, 0.0);
    t.ridge_elevation / water_load_factor(t) + buoyancy(t, thickness, density, lithosphere, keel)
        - ridge
        + dyn_topo
}

/// Height (km) above a sea at `sea` of a column standing at `in_air`: as it is
/// in air above water, and `water_load_factor` times as far below as it would
/// be in air below it.
pub fn under_sea(t: &Tuning, in_air: f64, sea: f64) -> f64 {
    let h = in_air - sea;
    if h >= 0.0 { h } else { h * water_load_factor(t) }
}

/// Elevation (km, relative to the sea) of a column under a sea at `sea`,
/// clamped as a safety net.
pub fn column_height(
    t: &Tuning,
    thickness: f64,
    density: f64,
    lithosphere: f64,
    keel: f64,
    dyn_topo: f64,
    sea: f64,
) -> f64 {
    let in_air = air_height(t, thickness, density, lithosphere, keel, dyn_topo);
    under_sea(t, in_air, sea).clamp(t.elevation_min, t.elevation_max)
}

/// Sea level `s` (km above the datum) at which the water fills the basins:
/// `k * sum area * max(0, s - e_air) = volume`. Returns it with the number of
/// passes over the cells it took. `V(s)` is convex, piecewise linear and
/// increasing, so Newton's method from the last `guess` finds it in a few
/// steps; steps leaving the bracket fall back to bisection. Every pass is a
/// [`par::sum`], so the answer is the same on any thread count.
pub fn solve_sea_level(
    area: impl Fn(usize) -> f64 + Sync + Send,
    e_air: &[f32],
    k: f64,
    volume: f64,
    guess: f64,
) -> (f64, usize) {
    let n = e_air.len();
    let total = par::sum(n, &area);
    if n == 0 || total <= 0.0 || volume <= 0.0 {
        return (e_air.iter().fold(f64::INFINITY, |l, &e| l.min(e as f64)).min(guess), 0);
    }
    // Between no water and water enough if every cell stood at the highest.
    let high = e_air.iter().fold(f64::NEG_INFINITY, |h, &e| h.max(e as f64));
    let low = e_air.iter().fold(f64::INFINITY, |l, &e| l.min(e as f64));
    let (mut lo, mut hi) = (low, high + volume / (k * total));
    let mut s = guess.clamp(lo, hi);
    for pass in 1..=60 {
        // Water held and its rate of change with the sea, over every cell.
        let held = par::sum(n, |i| k * area(i) * (s - e_air[i] as f64).max(0.0));
        let slope = par::sum(n, |i| if (e_air[i] as f64) < s { k * area(i) } else { 0.0 });
        let miss = held - volume;
        if miss < 0.0 {
            lo = lo.max(s);
        } else {
            hi = hi.min(s);
        }
        let step = if slope > 0.0 { miss / slope } else { f64::INFINITY };
        let next = s - step;
        if step.abs() < 1e-7 {
            return (s, pass);
        }
        s = if next > lo && next < hi { next } else { 0.5 * (lo + hi) };
    }
    (s, 60)
}

/// Elevation of cell `i` from its column alone, before the trench term.
pub fn column_elevation(t: &Tuning, data: &CellData, i: usize, dyn_topo: f64, sea: f64) -> f64 {
    column_height(
        t,
        data.thickness[i] as f64,
        data.density[i] as f64,
        data.lithosphere[i] as f64,
        data.keel[i] as f64,
        dyn_topo,
        sea,
    )
}

/// Recompute every cell's elevation from what is under it: its column in
/// air, lifted by `dyn_topo` (km); the sea level that `ocean_volume` of water
/// fills the basins to, solved from `sea_level`, last tick's; the column's
/// height above that sea, loaded by any water over it; and the flexure of a
/// trench on the down-going side of each subduction zone, taken fresh each
/// tick. Returns the new sea level (km above the datum).
pub fn apply_elevation(
    mesh: &Mesh,
    data: &mut CellData,
    dyn_topo: &[f32],
    boundaries: &BoundaryField,
    sea_level: f64,
    t: &Tuning,
) -> f64 {
    let mut trenches = Vec::new();
    for a in 0..boundaries.arc_kind.len() {
        if boundaries.arc_kind[a] != CONVERGENT {
            continue;
        }
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        if let Some(s) = subduction(mesh, data, ca as usize, cb as usize, t) {
            trenches.push((s.down, s.strength as f32));
        }
    }

    // Heights in air go in the elevation buffer for the solve, and the trench
    // strengths wait beside them.
    let mut strength = vec![0.0f32; mesh.cell_count];
    for (cell, share) in trenches {
        strength[cell] = strength[cell].max(share);
    }
    let (crust, lid, keel, thick) = (&data.density, &data.lithosphere, &data.keel, &data.thickness);
    par::for_each_mut(&mut data.elevation, |i, e| {
        *e = air_height(
            t,
            thick[i] as f64,
            crust[i] as f64,
            lid[i] as f64,
            keel[i] as f64,
            dyn_topo[i] as f64,
        ) as f32;
    });
    let volume = t.ocean_volume * par::sum(mesh.cell_count, |i| mesh.area(i));
    let (sea, _) = solve_sea_level(
        |i| mesh.area(i),
        &data.elevation,
        water_load_factor(t),
        volume,
        sea_level,
    );

    par::for_each_mut(&mut data.elevation, |i, elevation| {
        let column = under_sea(t, *elevation as f64, sea);
        let e = (column - t.trench_depth * strength[i] as f64).clamp(t.elevation_min, t.elevation_max);
        *elevation = e as f32;
    });
    sea
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lithosphere::{continental_lithosphere, ocean_lithosphere};

    fn ridge(t: &Tuning) -> f64 {
        column_height(t, t.oceanic_thickness, t.oceanic_density, 0.0, 0.0, 0.0, 0.0)
    }

    fn floor(t: &Tuning, age: f64) -> f64 {
        let lid = ocean_lithosphere(t, age);
        column_height(t, t.oceanic_thickness, t.oceanic_density, lid, 0.0, 0.0, 0.0) - ridge(t)
    }

    /// Height of a continental column over a ridge crest beside it, under the
    /// same sea. Keeled columns stand under a full-depth lid.
    fn continent(t: &Tuning, thickness: f64, keel: f64) -> f64 {
        let lid = continental_lithosphere(t, keel);
        let density = t.continental_density;
        column_height(t, thickness, density, lid, keel, 0.0, 0.0) - ridge(t)
    }

    #[test]
    fn a_ridge_crest_stands_at_the_ridge_elevation() {
        let t = Tuning::default();
        assert!((ridge(&t) - t.ridge_elevation).abs() < 1e-9);
        let hot = Tuning { ridge_elevation: t.ridge_elevation + 0.3, ..t.clone() };
        assert!((ridge(&hot) - hot.ridge_elevation).abs() < 1e-9);
    }

    #[test]
    fn the_sea_floor_sinks_as_the_square_root_of_its_age_then_flattens() {
        let t = Tuning::default();
        let depth = |age: f64| floor(&t, age);
        assert!(depth(0.0).abs() < 1e-9);
        assert!(depth(25.0) < depth(6.0) && depth(100.0) < depth(25.0));
        // sqrt(age): a quarter of the age gives half the lid, so half the sinking.
        assert!((depth(25.0) - 0.5 * depth(100.0)).abs() < 1e-6);
        // The lid stops growing at `plate_thickness`, and so does the sinking.
        let flat = (t.plate_thickness / t.lithosphere_growth).powi(2);
        assert!((depth(flat) - depth(3.0 * flat)).abs() < 1e-9);
    }

    #[test]
    fn a_100_myr_floor_lies_2_5_to_3_5_km_below_the_ridge() {
        let t = Tuning::default();
        let drop = -floor(&t, 100.0);
        assert!((2.5..3.5).contains(&drop), "100 Myr floor {drop:.2} km under the ridge");
    }

    #[test]
    fn the_column_model_agrees_with_the_cooling_curve_the_ridge_push_uses() {
        let t = Tuning::default();
        let model = t.ridge_elevation + floor(&t, t.seafloor_flattening_age);
        assert!(
            (model - t.abyss_elevation).abs() < 0.5,
            "column model puts old floor at {model:.2} km, abyss_elevation says {:.2}",
            t.abyss_elevation
        );
    }

    #[test]
    fn unkeeled_continent_stands_2_5_to_3_5_km_above_the_ridge() {
        let t = Tuning::default();
        let up = continent(&t, 40.0, 0.0);
        assert!((2.5..3.5).contains(&up), "40 km continent {up:.2} km over the ridge");
    }

    #[test]
    fn a_keeled_craton_stands_within_half_a_km_of_unkeeled_continent() {
        let t = Tuning::default();
        let (craton, plain) = (continent(&t, 40.0, 1.0), continent(&t, 40.0, 0.0));
        assert!((craton - plain).abs() < 0.5, "craton {craton:.2} km, continent {plain:.2} km");
    }

    #[test]
    fn an_orogen_stands_2_to_4_km_above_ordinary_continent() {
        let t = Tuning::default();
        let up = continent(&t, 60.0, 0.0) - continent(&t, 40.0, 0.0);
        assert!((2.0..4.0).contains(&up), "60 km orogen {up:.2} km over 40 km continent");
    }

    #[test]
    fn a_column_sinks_further_under_water_than_it_would_rise_in_air() {
        let t = Tuning::default();
        let k = water_load_factor(&t);
        assert!((k - 1.45).abs() < 0.01);
        let wet = |dyn_topo: f64| {
            column_height(&t, t.oceanic_thickness, t.oceanic_density, 0.0, 0.0, dyn_topo, 0.0)
        };
        assert!(((wet(-1.0) - wet(0.0)) - -k).abs() < 1e-9);
        // Raising the sea drowns a coast, and leaves a column above it alone.
        let height = |sea: f64| column_height(&t, 40.0, t.continental_density, 150.0, 0.0, 0.0, sea);
        assert!((height(-0.5) - height(0.0) - 0.5).abs() < 1e-9, "a sea 0.5 km higher");
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

    fn bowl(areas: &[f64], heights: &[f32], volume: f64, guess: f64) -> (f64, usize) {
        solve_sea_level(|i| areas[i], heights, 1.0, volume, guess)
    }

    #[test]
    fn the_sea_in_a_flat_world_stands_at_the_depth_of_its_water() {
        let heights = [-1.0f32; 10];
        let (s, _) = bowl(&[1.0; 10], &heights, 10.0 * 2.6, 0.0);
        assert!((s - (-1.0 + 2.6)).abs() < 1e-6, "sea at {s}");
    }

    #[test]
    fn a_two_level_world_is_solved_exactly() {
        // Half the area at -2, half at +1, water for 1.5 deep over all: fills
        // the low half to -2 + 3 = 1 (volume 3 * 0.5) and then spreads over both.
        let areas = [1.0, 1.0];
        let heights = [-2.0f32, 1.0];
        let (s, _) = bowl(&areas, &heights, 1.5, 0.0);
        assert!((s - -0.5).abs() < 1e-6, "sea at {s}");
        // Water enough to cover both: 3 + 2*(s-1) = 4.
        let (s, _) = bowl(&areas, &heights, 4.0, 0.0);
        assert!((s - 1.5).abs() < 1e-6, "sea at {s}");
    }

    #[test]
    fn the_sea_converges_from_cold_in_eight_passes_and_from_warm_in_three() {
        let areas = vec![1.0; 5000];
        let heights: Vec<f32> = (0..5000).map(|i| ((i * 7919) % 5000) as f32 / 5000.0 * 12.0 - 8.0).collect();
        let volume = 2.6 * 5000.0;
        let (s, cold) = bowl(&areas, &heights, volume, 0.0);
        assert!(cold <= 8, "cold start took {cold} passes");
        let (again, warm) = bowl(&areas, &heights, volume, s + 0.01);
        assert!(warm <= 3, "warm start took {warm} passes");
        assert!((again - s).abs() < 1e-6);
    }

    #[test]
    fn more_water_raises_the_sea() {
        let areas = vec![1.0; 100];
        let heights: Vec<f32> = (0..100).map(|i| i as f32 / 10.0 - 5.0).collect();
        let mut last = f64::NEG_INFINITY;
        for volume in [10.0, 50.0, 200.0, 800.0] {
            let (s, _) = bowl(&areas, &heights, volume, 0.0);
            assert!(s > last, "{volume} of water put the sea at {s}, after {last}");
            last = s;
        }
    }
}
