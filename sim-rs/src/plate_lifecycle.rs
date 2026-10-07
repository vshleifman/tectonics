//! Plate identity over deep time, mirroring `plateLifecycle.ts`: terranes
//! dock, pinched-off fragments become plates or are absorbed, and plates left
//! holding no crust are retired.

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

    let min_area = t.min_plate_area * total_area;
    let mut dissolving = vec![false; comp_count + 1];
    let mut absorb: Vec<usize> = Vec::new();
    for comp in 0..comp_count {
        if largest.get(&comp_plate[comp]) == Some(&comp) {
            continue;
        }
        if comp_area[comp] < min_area {
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
    }

    // Slivers are handed to a surrounding plate, eroded from the outside in.
    let is_dissolving = |component: &[i32], cell: usize| {
        let comp = component[cell];
        comp >= 0 && dissolving[comp as usize]
    };
    let mut frontier: Vec<usize> = Vec::new();
    for &comp in &absorb {
        for q in comp_start[comp]..comp_start[comp + 1] {
            let cell = order[q] as usize;
            if mesh
                .nbrs(cell)
                .any(|k| !is_dissolving(component, mesh.nbr[k] as usize))
            {
                frontier.push(cell);
            }
        }
    }
    let mut head = 0;
    while head < frontier.len() {
        let cell = frontier[head];
        head += 1;
        if !is_dissolving(component, cell) {
            continue;
        }
        let taker = mesh
            .nbrs(cell)
            .map(|k| mesh.nbr[k] as usize)
            .find(|&m| !is_dissolving(component, m) && plate_id[m] >= 0);
        let Some(taker) = taker else { continue };
        plate_id[cell] = plate_id[taker];
        component[cell] = component[taker];
        for k in mesh.nbrs(cell) {
            let m = mesh.nbr[k] as usize;
            if is_dissolving(component, m) {
                frontier.push(m);
            }
        }
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
    plates.by_id.retain(|&id, _| is_live(id));
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
