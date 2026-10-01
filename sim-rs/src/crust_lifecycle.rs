//! Crust creation and ageing, mirroring `crustLifecycle.ts`: rift gaps are
//! filled with fresh ocean (and booked as created mass), everything else ages,
//! and oceanic crust thickened past the threshold turns continental.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData, OCEANIC, total_crust_mass};
use crate::elevation::baseline_elevation;
use crate::plates::oceanic_density_from_age;
use crate::tuning::Tuning;

pub fn apply_crust_lifecycle(
    areas: &[f32],
    data: &mut CellData,
    dt_myr: f64,
    field: &mut AdvectionField,
    t: &Tuning,
) {
    let mut created = 0.0f64;
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
            } else {
                data.density[i] = oceanic_density_from_age(t, data.age[i] as f64) as f32;
            }
        }
    }
    field.created_this_step = created;
    field.created_total += created;
    field.live_mass = total_crust_mass(areas, data);
}
