//! Relief from plate interaction, mirroring `elevation.ts`: boundary forces
//! nudge elevation, then every cell relaxes toward its isostatic baseline.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT, NONE};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::clamp01;
use crate::tuning::Tuning;

/// Isostatic baseline elevation of a cell from its crust alone.
pub fn baseline_elevation(t: &Tuning, crust_type: u8, age: f64, thickness: f64) -> f64 {
    if crust_type == CONTINENTAL {
        let extra = (thickness - t.continent_thickness_ref).max(0.0);
        return t.continent_base + extra * t.continent_thickness_gain;
    }
    let s = clamp01(age / t.max_oceanic_age);
    t.ridge_elevation + (t.abyss_elevation - t.ridge_elevation) * s
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
