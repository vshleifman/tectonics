//! Plate identity over deep time, mirroring `plateLifecycle.ts`: terranes
//! dock, pinched-off fragments become plates or are absorbed, and plates left
//! holding no crust are retired.
//!
//! A piece pinched off its plate becomes a plate of its own only once it covers
//! `min_fragment_area`. Anything smaller joins, as a whole, the neighbouring
//! plate it shares the most boundary with: a handful of cells left behind by
//! moving the crust a cell at a time is an artefact of the mesh, not a plate.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{cross, read3, sub};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateProps, split_key};
use crate::registry::Registry;
use crate::scratch::Scratch;
use crate::tuning::Tuning;
use std::collections::HashMap;

pub fn update_plate_identity(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    registry: &mut Registry,
    s: &mut Scratch,
    t: &Tuning,
) {
    dock_terranes(mesh, data, plates, s, t);
    let n = mesh.cell_count;
    let plate_id = &mut data.plate_id;

    // Connected components of each plate, filled breadth-first into one shared
    // order buffer so each component's cells are a contiguous run of it.
    let component = &mut s.i32a;
    let order = &mut s.queue;
    component.fill(-1);
    let mut comp_plate: Vec<i32> = Vec::new();
    let mut comp_area: Vec<f64> = Vec::new();
    let mut comp_start: Vec<usize> = Vec::new();
    let total_area: f64 = (0..n).map(|i| mesh.area(i)).sum();

    let mut tail = 0usize;
    for i in 0..n {
        if component[i] >= 0 || plate_id[i] < 0 {
            continue;
        }
        let plate = plate_id[i];
        let comp = comp_plate.len() as i32;
        comp_plate.push(plate);
        comp_start.push(tail);
        let mut area = 0.0f64;
        let mut head = tail;
        order[tail] = i as i32;
        tail += 1;
        component[i] = comp;
        while head < tail {
            let cell = order[head] as usize;
            head += 1;
            area += mesh.area(cell);
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if component[m] < 0 && plate_id[m] == plate {
                    component[m] = comp;
                    order[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        comp_area.push(area);
    }
    comp_start.push(tail);
    let comp_count = comp_plate.len();

    // The largest piece of each plate keeps its identity.
    let mut largest: HashMap<i32, usize> = HashMap::new();
    for comp in 0..comp_count {
        match largest.get(&comp_plate[comp]) {
            Some(&best) if comp_area[comp] <= comp_area[best] => {}
            _ => {
                largest.insert(comp_plate[comp], comp);
            }
        }
    }

    // Below `min_plate_area` a piece was never a plate. Between that and
    // `min_fragment_area` it used to be promoted; it is now absorbed whole.
    let promote_at = t.min_fragment_area.max(t.min_plate_area) * total_area;
    let mut dissolving = vec![false; comp_count + 1];
    let mut absorb: Vec<usize> = Vec::new();
    for comp in 0..comp_count {
        if largest.get(&comp_plate[comp]) == Some(&comp) {
            continue;
        }
        if comp_area[comp] < promote_at {
            absorb.push(comp);
            dissolving[comp] = true;
            continue;
        }
        // A genuine fragment: its own plate, inheriting its parent's motion.
        let id = registry.allocate();
        let omega = plates
            .by_id
            .get(&comp_plate[comp])
            .map_or([0.0, 0.0, 0.0], |p| p.omega);
        plates.by_id.insert(
            id,
            PlateProps {
                omega,
                pending: 0.0,
                steps: 0,
            },
        );
        for q in comp_start[comp]..comp_start[comp + 1] {
            plate_id[order[q] as usize] = id;
        }
        plates.fragments += 1;
    }

    // Each pinched piece joins the settled plate it shares the most boundary
    // with. A piece that only touches other pinched pieces waits until one of
    // them has joined, so the choice spreads in from the pieces that do touch
    // a real plate.
    let is_dissolving = |component: &[i32], dissolving: &[bool], cell: usize| {
        let comp = component[cell];
        comp >= 0 && dissolving[comp as usize]
    };
    let mut pending = absorb;
    while !pending.is_empty() {
        let mut waiting = Vec::new();
        let mut progressed = false;
        for comp in pending {
            let mut tally: Vec<(i32, f64)> = Vec::new();
            for q in comp_start[comp]..comp_start[comp + 1] {
                let cell = order[q] as usize;
                for k in mesh.nbrs(cell) {
                    let m = mesh.nbr[k] as usize;
                    if is_dissolving(component, &dissolving, m) {
                        continue;
                    }
                    let other = plate_id[m];
                    if other < 0 {
                        continue;
                    }
                    let arc = mesh.nbr_arc[k];
                    if arc < 0 {
                        continue;
                    }
                    let len = mesh.arc_length[arc as usize] as f64;
                    if let Some(entry) = tally.iter_mut().find(|e| e.0 == other) {
                        entry.1 += len;
                    } else {
                        tally.push((other, len));
                    }
                }
            }
            // Longest shared boundary, and the smaller id when they tie, so
            // the same mesh always gives the same plate.
            let Some(&(id, _)) = tally
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            else {
                waiting.push(comp);
                continue;
            };
            for q in comp_start[comp]..comp_start[comp + 1] {
                plate_id[order[q] as usize] = id;
            }
            dissolving[comp] = false;
            plates.absorbed_fragments += 1;
            progressed = true;
        }
        if !progressed {
            break;
        }
        pending = waiting;
    }

    // Retire whatever is left holding no crust.
    let max_id = plate_id.iter().copied().max().unwrap_or(-1);
    let mut live = vec![false; (max_id + 1).max(0) as usize];
    for &p in plate_id.iter() {
        if p >= 0 {
            live[p as usize] = true;
        }
    }
    let is_live = |id: i32| id >= 0 && id <= max_id && live[id as usize];
    let before = plates.by_id.len();
    plates.by_id.retain(|&id, _| is_live(id));
    plates.consumed += (before - plates.by_id.len()) as u64;
    plates.sutures.retain(|&key, _| {
        let (a, b) = split_key(key);
        is_live(a) && is_live(b)
    });
}

/// Buoyant crust riding an oceanic plate into a continent docks onto it.
fn dock_terranes(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &PlateData,
    s: &mut Scratch,
    t: &Tuning,
) {
    s.pack_plates(plates);
    let mut docking: Vec<(usize, i32)> = Vec::new();
    for i in 0..mesh.cell_count {
        if data.crust_type[i] != CONTINENTAL {
            continue;
        }
        let plate = data.plate_id[i];
        if plate < 0 {
            continue;
        }
        let mut own_continental = 0;
        let mut own_total = 0;
        let mut target = -1;
        let mut closing = 0.0f64;
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            let other = data.plate_id[m];
            if other == plate {
                own_total += 1;
                if data.crust_type[m] == CONTINENTAL {
                    own_continental += 1;
                }
                continue;
            }
            if other < 0 || data.crust_type[m] != CONTINENTAL {
                continue;
            }
            let arc = mesh.nbr_arc[k];
            if arc < 0 {
                continue;
            }
            let arc = arc as usize;
            let w = sub(s.omega(plate), s.omega(other));
            let r = cross(w, read3(&mesh.arc_mid, arc));
            let nrm = read3(&mesh.arc_normal, arc);
            let sign = if mesh.arc_cells[arc * 2] as usize == i {
                1.0
            } else {
                -1.0
            };
            let rate = sign * (r[0] * nrm[0] + r[1] * nrm[1] + r[2] * nrm[2]);
            if rate > closing {
                closing = rate;
                target = other;
            }
        }
        if target < 0 || closing <= t.active_speed_eps {
            continue;
        }
        if own_total > 0 && own_continental * 2 > own_total {
            continue;
        }
        docking.push((i, target));
    }
    for (cell, onto) in docking {
        data.plate_id[cell] = onto;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;
    use crate::registry::Registry;

    fn props() -> PlateProps {
        PlateProps {
            omega: [0.0; 3],
            pending: 0.0,
            steps: 0,
        }
    }

    /// A connected patch of `count` cells around the southernmost cell.
    fn southern_patch(mesh: &Mesh, count: usize) -> Vec<usize> {
        let south = (0..mesh.cell_count)
            .min_by(|&a, &b| {
                mesh.centres[a * 3 + 2]
                    .total_cmp(&mesh.centres[b * 3 + 2])
                    .then(a.cmp(&b))
            })
            .unwrap();
        let mut patch = vec![south];
        let mut seen = vec![false; mesh.cell_count];
        seen[south] = true;
        let mut head = 0;
        while patch.len() < count && head < patch.len() {
            let cell = patch[head];
            head += 1;
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if !seen[m] && patch.len() < count {
                    seen[m] = true;
                    patch.push(m);
                }
            }
        }
        patch
    }

    fn shares(mesh: &Mesh, data: &CellData, patch: &[usize]) -> (f64, f64) {
        let mut share = [0.0f64; 3];
        let in_patch = |i: usize| patch.contains(&i);
        for &cell in patch {
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if in_patch(m) {
                    continue;
                }
                let id = data.plate_id[m];
                if !(1..=2).contains(&id) {
                    continue;
                }
                let arc = mesh.nbr_arc[k];
                if arc >= 0 {
                    share[id as usize] += mesh.arc_length[arc as usize] as f64;
                }
            }
        }
        (share[1], share[2])
    }

    #[test]
    fn a_pinched_piece_joins_the_neighbour_with_the_longest_shared_boundary() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let patch = southern_patch(&mesh, 10);
        assert!(
            patch.iter().all(|&i| mesh.centres[i * 3 + 2] < -0.5),
            "the patch reaches the parent plate"
        );
        // The parent is the north. The south is two large plates split by the
        // y = 0 plane, so each is one connected body and the patch, sitting on
        // the south pole, borders both.
        let mut data = CellData::new(n);
        for i in 0..n {
            let (y, z) = (mesh.centres[i * 3 + 1], mesh.centres[i * 3 + 2]);
            data.plate_id[i] = if patch.contains(&i) || z > 0.2 {
                0
            } else if y > 0.0 {
                2
            } else {
                1
            };
        }
        let (share1, share2) = shares(&mesh, &data, &patch);
        let winner = if share2 > share1 { 2 } else { 1 };
        assert!(
            share1 != share2 && share1 > 0.0 && share2 > 0.0,
            "the piece does not border both neighbours: {share1}, {share2}"
        );

        let mut plates = PlateData::default();
        for id in 0..3 {
            plates.by_id.insert(id, props());
        }
        let mut registry = Registry::default();
        for _ in 0..3 {
            registry.allocate();
        }
        let mut s = Scratch::new(n);
        let t = Tuning {
            min_fragment_area: 0.02,
            ..Tuning::default()
        };
        update_plate_identity(&mesh, &mut data, &mut plates, &mut registry, &mut s, &t);

        assert!(
            patch.iter().all(|&i| data.plate_id[i] == winner),
            "joined {:?}, expected {winner} (shares {share1:.4} vs {share2:.4})",
            patch.iter().map(|&i| data.plate_id[i]).collect::<Vec<_>>()
        );
        assert_eq!(plates.fragments, 0);
        assert!(plates.absorbed_fragments >= 1);
    }

    #[test]
    fn a_piece_above_min_fragment_area_becomes_a_plate() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        for i in 0..n {
            let z = mesh.centres[i * 3 + 2];
            data.plate_id[i] = if z.abs() > 0.3 { 0 } else { 1 };
        }
        let mut plates = PlateData::default();
        plates.by_id.insert(0, props());
        plates.by_id.insert(1, props());
        let mut registry = Registry::default();
        registry.allocate();
        registry.allocate();
        let mut s = Scratch::new(n);
        let t = Tuning {
            min_fragment_area: 0.005,
            ..Tuning::default()
        };
        update_plate_identity(&mesh, &mut data, &mut plates, &mut registry, &mut s, &t);

        let ids = |sign: f32| -> Vec<i32> {
            (0..n)
                .filter(|&i| mesh.centres[i * 3 + 2] * sign > 0.3)
                .map(|i| data.plate_id[i])
                .collect()
        };
        let (north, south) = (ids(1.0), ids(-1.0));
        assert!(north.iter().all(|&id| id == north[0]));
        assert!(south.iter().all(|&id| id == south[0]));
        assert_ne!(north[0], south[0], "the two caps stayed one plate");
        assert!(north[0] == 0 || south[0] == 0);
        assert_eq!(plates.fragments, 1);
    }
}
