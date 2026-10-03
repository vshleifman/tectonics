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
        let wanted: f64 = cells
            .iter()
            .map(|&c| {
                let c = c as usize;
                (threshold - data.thickness[c] as f64).clamp(0.0, rate) * mesh.area(c)
            })
            .sum();
        let available: f64 = rim
            .iter()
            .map(|&m| (data.thickness[m] as f64 - threshold).max(0.0) * mesh.area(m))
            .sum();
        if wanted <= 0.0 || available <= 0.0 {
            continue;
        }
        let moved = wanted.min(available);
        for &m in &rim {
            let spare = (data.thickness[m] as f64 - threshold).max(0.0) * mesh.area(m);
            let take = moved * spare / available;
            data.thickness[m] = (data.thickness[m] as f64 - take / mesh.area(m)) as f32;
        }
        let share = moved / wanted;
        for &c in cells {
            let c = c as usize;
            let h = data.thickness[c] as f64;
            let gain = (threshold - h).clamp(0.0, rate) * share;
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

pub fn apply_crust_lifecycle(
    areas: &[f32],
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    t: &Tuning,
) {
    let mut created = 0.0f64;
    let (mut by_accretion, mut by_spreading, mut decontinentalised) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..areas.len() {
        if field.rift_cell[i] != 0 {
            data.crust_type[i] = OCEANIC;
            data.age[i] = 0.0;
            data.density[i] = t.oceanic_density_young as f32;
            data.thickness[i] = t.oceanic_thickness as f32;
            data.elevation[i] = baseline_elevation(t, OCEANIC, 0.0, t.oceanic_thickness) as f32;
            created += areas[i] as f64 * t.oceanic_thickness;
            continue;
        }
        data.age[i] = (data.age[i] as f64 + dt_myr) as f32;
        if data.crust_type[i] == OCEANIC {
            if data.thickness[i] as f64 >= t.continental_thickness_threshold {
                data.crust_type[i] = CONTINENTAL;
                data.density[i] = t.continental_density as f32;
                // Whichever brought in more this step gets the credit; crust
                // spilled from a full arc cell counts as accretion.
                if field.spread_in[i] > field.accreted[i] {
                    by_spreading += areas[i] as f64;
                } else {
                    by_accretion += areas[i] as f64;
                }
            } else {
                data.density[i] = oceanic_density_from_age(t, data.age[i] as f64) as f32;
            }
        } else if (data.thickness[i] as f64) < t.continental_revert_thickness {
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
    field.decontinentalised = decontinentalised;
    field.created_total += created;
    field.live_mass = total_crust_mass(areas, data);
}
