//! Continental recycling: the sink that makes continental growth level off.
//!
//! Arcs add continental crust wherever ocean subducts, and nothing in the rest
//! of the model takes much of it away again, so left alone continents spread
//! until there is no ocean left to make more. On Earth they stopped growing
//! billions of years ago: what arcs add (~3 km³/yr) is matched by what goes
//! back into the mantle at active margins, as sediment eroded off the land and
//! carried down with the subducting plate, and as forearc crust scraped from
//! the underside of the overriding margin. Both scale with how much continent
//! there is to erode and how much margin it has, so the loss is taken as a
//! share of the continental area, removed where it happens: at margins facing
//! a trench or the ocean of another plate, the thinnest forearc first.
//!
//! That share is the one free parameter of the planet. With a target area set,
//! it is matched to the growth the rest of the model produces, averaged over
//! [`GROWTH_MEMORY_MYR`], so the two balance when continents cover the target;
//! without one it is the fixed `continental_recycling_rate`.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::tuning::Tuning;

/// Time (Myr) over which the continental growth rate is averaged before the
/// recycling is matched to it: long enough to see through the supercontinent
/// cycle's swings, short enough to follow the slow decline of the whole.
pub const GROWTH_MEMORY_MYR: f64 = 200.0;

/// Keel above which a margin is cratonic, and too deep-rooted to recycle.
const CRATON_KEEL: f32 = 0.5;

/// Share of the continental area recycled per Myr.
pub fn recycling_rate(t: &Tuning, field: &AdvectionField) -> f64 {
    if t.continental_area_target > 0.0 {
        field.continental_growth.max(0.0) / t.continental_area_target
    } else {
        t.continental_recycling_rate
    }
}

/// Share of the globe covered by continental crust.
pub fn continental_share(mesh: &Mesh, data: &CellData) -> f64 {
    let (mut continental, mut total) = (0.0f64, 0.0f64);
    for i in 0..mesh.cell_count {
        let a = mesh.area(i);
        total += a;
        if data.crust_type[i] == CONTINENTAL {
            continental += a;
        }
    }
    if total > 0.0 {
        continental / total
    } else {
        0.0
    }
}

/// Thin this step's share of the continental area down to oceanic crust at
/// active margins, for the crust lifecycle to turn back into ocean. What is
/// removed goes down the trench, into the ledger.
pub fn recycle_continental_margins(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    t: &Tuning,
) {
    field.continental_recycled = 0.0;
    let n = mesh.cell_count;
    let globe: f64 = mesh.areas.iter().map(|&a| a as f64).sum();
    let share = continental_share(mesh, data);
    let due = recycling_rate(t, field) * share * globe * dt_myr;
    if due <= 0.0 {
        field.recycle_debt = 0.0;
        return;
    }

    // Continental crust over a slab, or beside the ocean of another plate.
    let mut margin: Vec<usize> = (0..n)
        .filter(|&i| {
            data.crust_type[i] == CONTINENTAL
                && data.keel[i] < CRATON_KEEL
                && (data.slab[i] > 0.0
                    || mesh.nbrs(i).any(|k| {
                        let m = mesh.nbr[k] as usize;
                        data.crust_type[m] != CONTINENTAL && data.plate_id[m] != data.plate_id[i]
                    }))
        })
        .collect();
    // Unmet debt carries over, so cells too large for one step's share are
    // still taken in time, but a world with no active margin does not bank
    // its recycling for when one appears.
    if margin.is_empty() {
        field.recycle_debt = 0.0;
        return;
    }
    field.recycle_debt = (field.recycle_debt + due).min(due + globe / n as f64 * 4.0);
    margin.sort_by(|&a, &b| {
        data.thickness[a]
            .total_cmp(&data.thickness[b])
            .then(a.cmp(&b))
    });

    let mut recycled_mass = 0.0f64;
    let mut recycled_area = 0.0f64;
    for &i in &margin {
        if field.recycle_debt <= 0.0 {
            break;
        }
        let a = mesh.area(i);
        let h = data.thickness[i] as f64;
        let keep = t.oceanic_thickness.min(h);
        recycled_mass += (h - keep) * a;
        data.thickness[i] = keep as f32;
        recycled_area += a;
        field.recycle_debt -= a;
    }
    field.subducted_total += recycled_mass;
    field.continental_recycled = recycled_area;
}

/// Fold this tick's change in continental area into the running growth rate,
/// counting back in what recycling took so the rate is that of everything else.
pub fn track_continental_growth(
    mesh: &Mesh,
    data: &CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
) {
    let share = continental_share(mesh, data);
    if let Some(before) = field.continental_share_before {
        let globe: f64 = mesh.areas.iter().map(|&a| a as f64).sum();
        let recycled = field.continental_recycled / globe;
        let growth = (share - before + recycled) / dt_myr.max(1e-9);
        let blend = (dt_myr / GROWTH_MEMORY_MYR).min(1.0);
        field.continental_growth += (growth - field.continental_growth) * blend;
    }
    field.continental_share_before = Some(share);
}
