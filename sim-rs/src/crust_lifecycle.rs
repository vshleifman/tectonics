//! Crust creation and ageing, mirroring `crustLifecycle.ts`: rift gaps are
//! filled with fresh ocean (and booked as created mass), everything else ages,
//! oceanic crust thickened past the threshold turns continental, and continental
//! crust thinned below the revert thickness turns back into ocean.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData, OCEANIC, total_crust_mass};
use crate::elevation::baseline_elevation;
use crate::mesh::Mesh;
use crate::plates::oceanic_density_from_age;
use crate::scratch::Scratch;
use crate::tuning::Tuning;

const SEEN: u8 = 1;
const RIM: u8 = 2;
/// Enclosed bodies of ocean this many cells or fewer fill in a single step.
const PINHOLE_CELLS: usize = 2;

/// Fill small bodies of ocean trapped inside a continent with crust shed off
/// the land around them. Arcs that coalesce around a patch of sea floor leave
/// it ringed by the continent they build, where no trench can reach it, so it
/// would otherwise ride inside that continent for ever. A body only counts if
/// it is under `basin_infill_max_area` of the globe and lies wholly within one
/// plate, away from any boundary, so a young ocean opening between two plates
/// is never filled. Its rim gives up crust above the continental threshold,
/// and nothing is created or lost.
pub fn infill_enclosed_basins(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    field.continentalised_by_infill = 0.0;
    let rate = t.basin_infill_rate * dt_myr;
    if rate <= 0.0 || t.basin_infill_max_area <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    let limit = t.basin_infill_max_area * mesh.areas.iter().map(|&a| a as f64).sum::<f64>();
    let threshold = t.continental_thickness_threshold;
    let mark = &mut s.u8a;
    let basin = &mut s.queue;
    let mut rim: Vec<usize> = Vec::new();
    let mut infilled = 0.0f64;

    for start in 0..n {
        if mark[start] != 0 || data.crust_type[start] == CONTINENTAL {
            continue;
        }
        let plate = data.plate_id[start];
        mark[start] = SEEN;
        basin[0] = start as i32;
        let (mut head, mut tail) = (0usize, 1usize);
        let mut area = 0.0f64;
        let mut enclosed = true;
        rim.clear();
        while head < tail {
            let cell = basin[head] as usize;
            head += 1;
            area += mesh.area(cell);
            // A rift opened this step is about to be laid as fresh ocean
            // floor, which would overwrite any crust shed into it.
            if field.rift_cell[cell] != 0 {
                enclosed = false;
            }
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if data.plate_id[m] != plate {
                    enclosed = false;
                }
                if data.crust_type[m] == CONTINENTAL {
                    if mark[m] == 0 {
                        mark[m] = RIM;
                        rim.push(m);
                    }
                } else if mark[m] != SEEN {
                    mark[m] = SEEN;
                    basin[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        for &m in &rim {
            mark[m] = 0;
        }
        if !enclosed || area >= limit {
            continue;
        }

        let cells = &basin[..tail];
        // A pinhole of a cell or two closes at once, its rim giving down to
        // the revert thickness: the rim is inland, so it stays continental.
        let pinhole = cells.len() <= PINHOLE_CELLS;
        let (step, rim_floor) = if pinhole {
            (f64::INFINITY, (t.continental_revert_thickness + 1e-3).min(threshold))
        } else {
            (rate, threshold)
        };
        let wanted: f64 = cells
            .iter()
            .map(|&c| {
                let c = c as usize;
                (threshold - data.thickness[c] as f64).clamp(0.0, step) * mesh.area(c)
            })
            .sum();
        let available: f64 = rim
            .iter()
            .map(|&m| (data.thickness[m] as f64 - rim_floor).max(0.0) * mesh.area(m))
            .sum();
        if wanted <= 0.0 || available <= 0.0 {
            continue;
        }
        let moved = wanted.min(available);
        for &m in &rim {
            let spare = (data.thickness[m] as f64 - rim_floor).max(0.0) * mesh.area(m);
            let take = moved * spare / available;
            data.thickness[m] = (data.thickness[m] as f64 - take / mesh.area(m)) as f32;
        }
        let share = moved / wanted;
        for &c in cells {
            let c = c as usize;
            let h = data.thickness[c] as f64;
            let gain = (threshold - h).clamp(0.0, step) * share;
            data.thickness[c] = (h + gain) as f32;
            if h + gain >= threshold - 1e-4 {
                data.crust_type[c] = CONTINENTAL;
                data.density[c] = t.continental_density as f32;
                infilled += mesh.area(c);
            }
        }
    }
    mark.fill(0);
    field.continentalised_by_infill = infilled;
}

/// Crust type flips look at the neighbours, so grid noise cannot speckle a
/// continent: continent only reverts to ocean on its coast (or once thinned to
/// no more than ocean floor, as recycling leaves it), and ocean floor
/// only turns continental beside continent, over a plume, or once it has piled
/// past `continental_nucleation_thickness`. Flips are decided on the types at
/// the start of the pass, so the order cells are visited in does not matter.
pub fn apply_crust_lifecycle(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    let areas = &mesh.areas;
    let mut created = 0.0f64;
    let (mut by_accretion, mut by_spreading, mut decontinentalised) = (0.0f64, 0.0f64, 0.0f64);
    let (mut by_plume, mut by_piling) = (0.0f64, 0.0f64);
    field.piled.fill(0);
    let was = &mut s.u8b;
    for i in 0..areas.len() {
        was[i] = (data.crust_type[i] == CONTINENTAL && field.rift_cell[i] == 0) as u8;
    }
    let continental_nbrs = |i: usize| mesh.nbrs(i).filter(|&k| was[mesh.nbr[k] as usize] != 0).count();
    for i in 0..areas.len() {
        if field.rift_cell[i] != 0 {
            data.crust_type[i] = OCEANIC;
            data.age[i] = 0.0;
            data.plume_share[i] = 0.0;
            data.density[i] = t.oceanic_density_young as f32;
            data.thickness[i] = t.oceanic_thickness as f32;
            data.elevation[i] = baseline_elevation(t, OCEANIC, 0.0, t.oceanic_thickness) as f32;
            created += areas[i] as f64 * t.oceanic_thickness;
            continue;
        }
        data.age[i] = (data.age[i] as f64 + dt_myr) as f32;
        if data.crust_type[i] == OCEANIC {
            let h = data.thickness[i] as f64;
            let nucleates = h >= t.continental_thickness_threshold
                && (field.plume_in[i] > 0.0
                    || h >= t.continental_nucleation_thickness
                    || continental_nbrs(i) >= 2);
            if nucleates {
                data.crust_type[i] = CONTINENTAL;
                data.density[i] = t.continental_density as f32;
                // Whichever brought in more this step gets the credit; crust
                // spilled from a full arc cell counts as accretion.
                if field.plume_in[i] > field.accreted[i].max(field.spread_in[i]) {
                    by_plume += areas[i] as f64;
                } else if field.accreted[i] <= 0.0 && field.spread_in[i] <= 0.0 {
                    by_piling += areas[i] as f64;
                    field.piled[i] = 1;
                } else if field.spread_in[i] > field.accreted[i] {
                    by_spreading += areas[i] as f64;
                } else {
                    by_accretion += areas[i] as f64;
                }
            } else {
                data.density[i] = oceanic_density_from_age(t, data.age[i] as f64) as f32;
            }
        } else if (data.thickness[i] as f64) < t.continental_revert_thickness
            && (continental_nbrs(i) < mesh.nbrs(i).len()
                || data.thickness[i] as f64 <= t.oceanic_thickness + 1e-3)
        {
            data.crust_type[i] = OCEANIC;
            data.age[i] = 0.0;
            data.density[i] = t.oceanic_density_young as f32;
            data.keel[i] = 0.0;
            decontinentalised += areas[i] as f64;
        }
    }
    field.created_this_step = created;
    field.continentalised_by_accretion = by_accretion;
    field.continentalised_by_spreading = by_spreading;
    field.continentalised_by_plume = by_plume;
    field.continentalised_by_piling = by_piling;
    field.decontinentalised = decontinentalised;
    field.created_total += created;
    field.live_mass = total_crust_mass(areas, data);
    was.fill(0);
}
