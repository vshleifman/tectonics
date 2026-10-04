//! Passive-margin collapse (experimental, off unless
//! `passive_margin_collapse_age` is set): ocean that has aged against a
//! continent on the same plate breaks from it, so the coast becomes a plate
//! boundary where a trench can start.
//!
//! Subduction only ever starts between two plates (see `slabs.rs`), so an ocean
//! that opened inside a plate when a continent rifted has no way to close: its
//! coasts are passive for good. On Earth old passive margins do eventually
//! founder, which is how oceans like the Iapetus closed and how continents that
//! dispersed come back together.

use std::collections::HashMap;

use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateProps};
use crate::registry::Registry;
use crate::scratch::Scratch;
use crate::tuning::Tuning;

/// Shortest stretch of old passive coast (radians, ~640 km) that breaks away.
const MIN_COAST: f64 = 0.1;

/// Detach the ocean of at most one plate from the continent it has aged
/// against, as a new plate.
pub fn collapse_passive_margins(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    registry: &mut Registry,
    s: &mut Scratch,
    t: &Tuning,
) {
    if t.passive_margin_collapse_age <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    let old_coast = |data: &CellData, i: usize| -> f64 {
        if data.crust_type[i] == CONTINENTAL
            || (data.age[i] as f64) < t.passive_margin_collapse_age
        {
            return 0.0;
        }
        mesh.nbrs(i)
            .filter(|&k| {
                let m = mesh.nbr[k] as usize;
                data.crust_type[m] == CONTINENTAL && data.plate_id[m] == data.plate_id[i]
            })
            .map(|k| {
                let arc = mesh.nbr_arc[k];
                if arc >= 0 {
                    mesh.arc_length[arc as usize] as f64
                } else {
                    0.0
                }
            })
            .sum()
    };

    let mut coast: HashMap<i32, f64> = HashMap::new();
    for i in 0..n {
        let len = old_coast(data, i);
        if len > 0.0 {
            *coast.entry(data.plate_id[i]).or_insert(0.0) += len;
        }
    }
    let Some((&id, _)) = coast
        .iter()
        .filter(|&(_, &len)| len >= MIN_COAST)
        .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
    else {
        return;
    };

    // The body of the plate's ocean with the most old coast along it.
    let globe: f64 = mesh.areas.iter().map(|&a| a as f64).sum();
    let plate_area: f64 = (0..n)
        .filter(|&i| data.plate_id[i] == id)
        .map(|i| mesh.area(i))
        .sum();
    let seen = &mut s.u8b;
    seen.fill(0);
    let queue = &mut s.queue;
    let is_ocean = |data: &CellData, i: usize| {
        data.plate_id[i] == id && data.crust_type[i] != CONTINENTAL
    };
    let mut tail = 0usize;
    let (mut best, mut best_coast, mut best_area) = ((0usize, 0usize), 0.0f64, 0.0f64);
    for start in 0..n {
        if seen[start] != 0 || !is_ocean(data, start) || old_coast(data, start) <= 0.0 {
            continue;
        }
        let from = tail;
        seen[start] = 1;
        queue[tail] = start as i32;
        tail += 1;
        let (mut head, mut area, mut length) = (from, 0.0, 0.0);
        while head < tail {
            let cell = queue[head] as usize;
            head += 1;
            area += mesh.area(cell);
            length += old_coast(data, cell);
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if seen[m] == 0 && is_ocean(data, m) {
                    seen[m] = 1;
                    queue[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        if length > best_coast {
            (best, best_coast, best_area) = ((from, tail), length, area);
        }
    }
    let min_area = t.min_plate_area * globe;
    if best_coast < MIN_COAST || best_area < min_area || plate_area - best_area < min_area {
        return;
    }

    let new_id = registry.allocate();
    let omega = plates.by_id.get(&id).map_or([0.0; 3], |p| p.omega);
    plates.by_id.insert(
        new_id,
        PlateProps {
            omega,
            pending: 0.0,
        },
    );
    for q in best.0..best.1 {
        data.plate_id[queue[q] as usize] = new_id;
    }
    plates.passive_collapses += 1;
}
