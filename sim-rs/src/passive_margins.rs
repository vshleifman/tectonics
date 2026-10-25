//! Passive-margin collapse (off unless `passive_margin_collapse_age` is set):
//! ocean that has aged against a continent on the same plate breaks from it,
//! so the coast becomes a plate boundary where a trench can start.
//!
//! Subduction only ever starts between two plates (see `slabs.rs`), so an ocean
//! that opened inside a plate when a continent rifted has no way to close: its
//! coasts are passive for good. On Earth old passive margins do eventually
//! founder, which is how oceans like the Iapetus closed and how continents that
//! dispersed come back together.
//!
//! With `collapse_stagnant_only`, only a stagnant plate sheds its ocean: one
//! with little working trench for its size, or whose floor is on average more
//! than twice the collapse age. A plate that is part of a moving lid keeps
//! renewing its floor and has trenches to consume it, so an old passive coast
//! or two does not break it up. One that has stopped, above all a single
//! plate covering the globe or nearly so, ages without limit until its
//! margins founder. That is what stops a world from settling into one
//! stagnant lid.

use std::collections::HashMap;

use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateProps};
use crate::registry::Registry;
use crate::scratch::Scratch;
use crate::slabs::subduction;
use crate::tuning::{SimOptions, Tuning};

/// Shortest stretch of old passive coast (radians, ~640 km) that breaks away.
const MIN_COAST: f64 = 0.1;

/// Share of a full slab above which a margin counts as a working trench.
const TRENCH_SLAB: f64 = 0.1;

/// Trench, per unit of a plate's width (the square root of its area), below
/// which the plate counts as stagnant. A round plate's rim is about 3.5 times
/// its width, so this is trench along about a seventh of it: a plate covering
/// the globe beside one small trench is still a stagnant lid.
const STAGNANT_TRENCH: f64 = 0.5;

/// Mean ocean age, as a multiple of the collapse age, above which a plate
/// counts as stagnant whatever its trenches. A plate covering most of the
/// globe has a rim made of the small plates around it, and can have trench
/// all along that rim while its floor goes unrenewed.
const STAGNANT_AGE: f64 = 2.0;

/// Length of working trench (radians) along each plate's boundary, on either
/// side. A continental plate overrides its trenches rather than going down,
/// but its margins are still active.
fn trench_length(mesh: &Mesh, data: &CellData, t: &Tuning) -> HashMap<i32, f64> {
    let mut trench: HashMap<i32, f64> = HashMap::new();
    for a in 0..mesh.arc_count() {
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (ca, cb) = (ca as usize, cb as usize);
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        if pa < 0 || pb < 0 || pa == pb {
            continue;
        }
        if subduction(mesh, data, ca, cb, t).is_some_and(|s| s.strength > TRENCH_SLAB) {
            let len = mesh.arc_length[a] as f64;
            *trench.entry(pa).or_insert(0.0) += len;
            *trench.entry(pb).or_insert(0.0) += len;
        }
    }
    trench
}

/// Each plate's area, and the area-weighted mean age (Myr) of its ocean floor.
fn plate_area_and_ocean_age(mesh: &Mesh, data: &CellData) -> HashMap<i32, (f64, f64)> {
    let mut sums: HashMap<i32, [f64; 3]> = HashMap::new();
    for i in 0..mesh.cell_count {
        let a = mesh.area(i);
        let e = sums.entry(data.plate_id[i]).or_insert([0.0; 3]);
        e[0] += a;
        if data.crust_type[i] != CONTINENTAL {
            e[1] += a * data.age[i] as f64;
            e[2] += a;
        }
    }
    sums.into_iter()
        .map(|(p, [area, age, ocean])| (p, (area, age / ocean.max(1e-12))))
        .collect()
}

/// Detach the ocean of at most one plate from the continent it has aged
/// against, as a new plate.
pub fn collapse_passive_margins(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    registry: &mut Registry,
    s: &mut Scratch,
    t: &Tuning,
    o: &SimOptions,
) {
    if t.passive_margin_collapse_age <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    let (trench, sizes) = if o.collapse_stagnant_only {
        (trench_length(mesh, data, t), plate_area_and_ocean_age(mesh, data))
    } else {
        (HashMap::new(), HashMap::new())
    };
    let stagnant = |plate: i32| {
        if !o.collapse_stagnant_only {
            return true;
        }
        let (area, ocean_age) = sizes.get(&plate).copied().unwrap_or((0.0, 0.0));
        let trench = trench.get(&plate).copied().unwrap_or(0.0);
        trench < (STAGNANT_TRENCH * area.sqrt()).max(MIN_COAST)
            || ocean_age >= STAGNANT_AGE * t.passive_margin_collapse_age
    };
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
        .filter(|&(&plate, &len)| len >= MIN_COAST && stagnant(plate))
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
            steps: 0,
        },
    );
    for q in best.0..best.1 {
        data.plate_id[queue[q] as usize] = new_id;
    }
    plates.passive_collapses += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::OCEANIC;

    fn props() -> PlateProps {
        PlateProps {
            omega: [0.0; 3],
            pending: 0.0,
            steps: 0,
        }
    }

    /// A continent over the north pole, a strip of ocean twice the collapse
    /// age along its coast, and beyond it ocean of age `far`, all one plate.
    /// With `trench`, the far south is a second plate, with a working trench
    /// along the whole of its edge.
    fn world(
        mesh: &Mesh,
        t: &Tuning,
        far: f64,
        trench: bool,
    ) -> (CellData, PlateData, Registry) {
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        for i in 0..n {
            let z = mesh.centres[i * 3 + 2];
            data.plate_id[i] = if trench && z < -0.5 { 1 } else { 0 };
            if z > 0.5 {
                data.crust_type[i] = CONTINENTAL;
            } else {
                data.crust_type[i] = OCEANIC;
                let old = 2.0 * t.passive_margin_collapse_age;
                data.age[i] = if z > 0.35 { old } else { far } as f32;
            }
            if trench {
                data.slab[i] = t.slab_full_length as f32;
            }
        }
        let mut plates = PlateData::default();
        let mut registry = Registry::default();
        for id in 0..if trench { 2 } else { 1 } {
            plates.by_id.insert(id, props());
            registry.allocate();
        }
        (data, plates, registry)
    }

    fn collapses(far: f64, trench: bool, o: &SimOptions) -> u64 {
        let mesh = Mesh::icosphere(4);
        let t = Tuning {
            passive_margin_collapse_age: 300.0,
            ..Tuning::default()
        };
        let (mut data, mut plates, mut registry) = world(&mesh, &t, far, trench);
        let mut s = Scratch::new(mesh.cell_count);
        collapse_passive_margins(&mesh, &mut data, &mut plates, &mut registry, &mut s, &t, o);
        if plates.passive_collapses > 0 {
            let continent = (0..mesh.cell_count)
                .find(|&i| data.crust_type[i] == CONTINENTAL)
                .unwrap();
            let parent = data.plate_id[continent];
            assert!(
                (0..mesh.cell_count)
                    .filter(|&i| data.crust_type[i] == CONTINENTAL)
                    .all(|i| data.plate_id[i] == parent),
                "the continent was split"
            );
            assert!(
                (0..mesh.cell_count)
                    .any(|i| data.crust_type[i] != CONTINENTAL && data.plate_id[i] != parent),
                "no ocean broke away"
            );
        }
        plates.passive_collapses
    }

    #[test]
    fn a_single_plate_covering_the_globe_breaks_at_its_old_margin() {
        assert_eq!(collapses(20.0, false, &SimOptions::default()), 1);
    }

    #[test]
    fn a_plate_with_trenches_and_young_floor_keeps_an_old_coast() {
        assert_eq!(collapses(20.0, true, &SimOptions::default()), 0);
        let anywhere = SimOptions {
            collapse_stagnant_only: false,
            ..SimOptions::default()
        };
        assert_eq!(collapses(20.0, true, &anywhere), 1, "the gate was not what held it");
    }

    #[test]
    fn a_plate_whose_floor_has_gone_unrenewed_breaks_despite_its_trenches() {
        assert_eq!(collapses(1000.0, true, &SimOptions::default()), 1);
    }
}
