//! Keeping continents whole. Moving plates a whole cell at a time over an
//! irregular mesh cannot carry every cell along: now and then a continental
//! cell is left with nowhere to go, and its crust welds onto whatever moved in
//! beside it. Left alone, a continent sheds a trail of such crumbs behind it
//! as it travels, and reads as a spray of dots rather than a landmass.
//!
//! Each tick, crust on any landmass too small to be a continent in its own
//! right (a single cell, or under `min_continent_area` of the globe) looks up
//! to `cohesion_reach` rings across its own plate for a larger landmass, and
//! trades places with the nearest ocean floor on that landmass's coast. Only
//! the crust moves: the slab beneath, the weakness of the lithosphere and the
//! side of a rift belong to the place, and stay put. A trade is a permutation
//! of the crust, so nothing is created or lost. A crumb with no landmass in
//! reach, such as a young island arc out in the ocean, is left to grow.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::scratch::Scratch;
use crate::tuning::Tuning;

const VISITED: u8 = 1;

/// Trade the crust of cells `a` and `b`, keeping each one's mass as it moves
/// into a cell of a slightly different area.
fn swap_crust(mesh: &Mesh, data: &mut CellData, a: usize, b: usize) {
    let (area_a, area_b) = (mesh.area(a), mesh.area(b));
    let (h_a, h_b) = (data.thickness[a] as f64, data.thickness[b] as f64);
    data.thickness[a] = (h_b * area_b / area_a) as f32;
    data.thickness[b] = (h_a * area_a / area_b) as f32;
    data.crust_type.swap(a, b);
    data.age.swap(a, b);
    data.density.swap(a, b);
    data.keel.swap(a, b);
    data.plume_share.swap(a, b);
    data.elevation.swap(a, b);
}

/// Is `m` continental crust on a landmass large enough to rejoin?
fn on_large(data: &CellData, label: &[i32], large: &[bool], m: usize) -> bool {
    data.crust_type[m] == CONTINENTAL && label[m] >= 0 && large[label[m] as usize]
}

/// If `c` is ocean floor of `plate` on the coast of a large landmass of that
/// plate, whether it also lies on the plate's margin.
fn slot_margin(
    mesh: &Mesh,
    data: &CellData,
    label: &[i32],
    large: &[bool],
    plate: i32,
    c: usize,
) -> Option<bool> {
    if data.crust_type[c] == CONTINENTAL || data.plate_id[c] != plate {
        return None;
    }
    let (mut coast, mut margin) = (false, false);
    for k in mesh.nbrs(c) {
        let m = mesh.nbr[k] as usize;
        if data.plate_id[m] != plate {
            margin = true;
        } else if on_large(data, label, large, m) {
            coast = true;
        }
    }
    coast.then_some(margin)
}

pub fn keep_continents_whole(
    mesh: &Mesh,
    data: &mut CellData,
    field: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    field.rejoined = 0.0;
    let reach = t.cohesion_reach.max(0.0).round() as usize;
    if reach == 0 {
        return;
    }
    let n = mesh.cell_count;
    let globe: f64 = mesh.areas.iter().map(|&a| a as f64).sum();
    let small_area = t.min_continent_area.max(0.0) * globe;

    // Landmasses, joined across plates: a crumb touching another plate's
    // continent is not adrift.
    let label = &mut s.i32a;
    label.fill(-1);
    let stack = &mut s.queue;
    let mut large: Vec<bool> = Vec::new();
    let mut strays: Vec<usize> = Vec::new();
    for start in 0..n {
        if label[start] >= 0 || data.crust_type[start] != CONTINENTAL {
            continue;
        }
        let id = large.len() as i32;
        label[start] = id;
        stack[0] = start as i32;
        let (mut head, mut tail) = (0usize, 1usize);
        let mut area = 0.0f64;
        while head < tail {
            let cell = stack[head] as usize;
            head += 1;
            area += mesh.area(cell);
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if label[m] < 0 && data.crust_type[m] == CONTINENTAL {
                    label[m] = id;
                    stack[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        let is_large = tail > 1 && area >= small_area;
        large.push(is_large);
        if !is_large {
            strays.extend(stack[..tail].iter().map(|&c| c as usize));
        }
    }
    if strays.is_empty() || !large.iter().any(|&l| l) {
        return;
    }
    strays.sort_unstable();

    let mark = &mut s.u8a;
    mark.fill(0);
    let mut touched: Vec<usize> = Vec::new();
    let mut ring: Vec<usize> = Vec::new();
    let mut next_ring: Vec<usize> = Vec::new();
    let mut rejoined = 0.0f64;
    for &stray in &strays {
        let plate = data.plate_id[stray];
        if plate < 0 || data.crust_type[stray] != CONTINENTAL {
            continue;
        }
        // An earlier trade may already have joined it up.
        if mesh
            .nbrs(stray)
            .any(|k| on_large(data, label, &large, mesh.nbr[k] as usize))
        {
            continue;
        }

        mark[stray] = VISITED;
        touched.push(stray);
        ring.clear();
        ring.push(stray);
        let mut slot: Option<(bool, usize)> = None;
        for _ in 0..reach {
            next_ring.clear();
            for &cell in &ring {
                for k in mesh.nbrs(cell) {
                    let m = mesh.nbr[k] as usize;
                    if mark[m] != 0 || data.plate_id[m] != plate {
                        continue;
                    }
                    mark[m] = VISITED;
                    touched.push(m);
                    next_ring.push(m);
                }
            }
            // Nearest first; within a ring, away from the plate's margin,
            // where it would only be overrun again, then by index.
            for &c in &next_ring {
                if let Some(margin) = slot_margin(mesh, data, label, &large, plate, c)
                    && slot.is_none_or(|best| (margin, c) < best)
                {
                    slot = Some((margin, c));
                }
            }
            if slot.is_some() || next_ring.is_empty() {
                break;
            }
            std::mem::swap(&mut ring, &mut next_ring);
        }
        for &c in &touched {
            mark[c] = 0;
        }
        touched.clear();

        let Some((_, slot)) = slot else { continue };
        swap_crust(mesh, data, stray, slot);
        let joined = mesh
            .nbrs(slot)
            .map(|k| mesh.nbr[k] as usize)
            .find(|&m| on_large(data, label, &large, m))
            .map_or(-1, |m| label[m]);
        label[slot] = joined;
        label[stray] = -1;
        rejoined += mesh.area(slot);
    }
    field.rejoined = rejoined;
}

#[cfg(test)]
mod tests {
    use super::keep_continents_whole;
    use crate::advect::AdvectionField;
    use crate::cell_data::{CONTINENTAL, CellData, OCEANIC, total_crust_mass};
    use crate::mesh::Mesh;
    use crate::scratch::Scratch;
    use crate::tuning::Tuning;
    use crate::world::Sim;

    /// One plate of ocean with a round continent about `centre`, and the rings
    /// of ocean around it, nearest first.
    fn island(mesh: &Mesh, centre: usize, rings: usize) -> (CellData, Vec<Vec<usize>>) {
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        let mut ring_of = vec![usize::MAX; n];
        let mut ring = vec![centre];
        ring_of[centre] = 0;
        for r in 1..=3 {
            let mut next = Vec::new();
            for &c in &ring {
                for k in mesh.nbrs(c) {
                    let m = mesh.nbr[k] as usize;
                    if ring_of[m] == usize::MAX {
                        ring_of[m] = r;
                        next.push(m);
                    }
                }
            }
            ring = next;
        }
        for i in 0..n {
            let continental = ring_of[i] <= 3;
            data.crust_type[i] = if continental { CONTINENTAL } else { OCEANIC };
            data.thickness[i] = if continental { 35.0 } else { 7.0 };
            data.age[i] = if continental { 500.0 } else { 50.0 };
        }
        let mut out: Vec<Vec<usize>> = Vec::new();
        let mut seen: Vec<bool> = (0..n).map(|i| ring_of[i] <= 3).collect();
        let mut frontier: Vec<usize> = (0..n).filter(|&i| ring_of[i] == 3).collect();
        for _ in 0..rings {
            let mut next = Vec::new();
            for &c in &frontier {
                for k in mesh.nbrs(c) {
                    let m = mesh.nbr[k] as usize;
                    if !seen[m] {
                        seen[m] = true;
                        next.push(m);
                    }
                }
            }
            next.sort_unstable();
            out.push(next.clone());
            frontier = next;
        }
        (data, out)
    }

    fn plant(data: &mut CellData, cell: usize) {
        data.crust_type[cell] = CONTINENTAL;
        data.thickness[cell] = 30.0;
        data.age[cell] = 800.0;
        data.keel[cell] = 0.3;
    }

    #[test]
    fn a_crumb_beside_its_continent_rejoins_it_and_keeps_the_mass() {
        let sim = Sim::new(4);
        let mesh = &sim.mesh;
        let (mut data, rings) = island(mesh, 0, 3);
        let stray = rings[1][0];
        plant(&mut data, stray);
        data.slab[stray] = 0.25;
        data.weakness[stray] = 0.5;
        let before = total_crust_mass(&mesh.areas, &data);
        let mut field = AdvectionField::new(mesh.cell_count);
        let mut s = Scratch::new(mesh.cell_count);
        keep_continents_whole(mesh, &mut data, &mut field, &mut s, &Tuning::default());

        assert_eq!(data.crust_type[stray], OCEANIC, "the crumb stayed put");
        let slot = rings[0]
            .iter()
            .copied()
            .find(|&c| data.crust_type[c] == CONTINENTAL)
            .expect("the crumb did not land on the coast");
        assert_eq!(data.age[slot], 800.0);
        assert_eq!(data.keel[slot], 0.3);
        assert_eq!(data.slab[stray], 0.25, "the slab moved with the crust");
        assert_eq!(data.weakness[stray], 0.5, "the weakness moved with the crust");
        assert!(field.rejoined > 0.0);
        let after = total_crust_mass(&mesh.areas, &data);
        assert!((after - before).abs() < 1e-6 * before, "mass {before} -> {after}");
    }

    #[test]
    fn a_crumb_out_of_reach_is_left_to_grow() {
        let sim = Sim::new(4);
        let mesh = &sim.mesh;
        let (mut data, rings) = island(mesh, 0, 6);
        let lone = rings[5][0];
        plant(&mut data, lone);
        let mut field = AdvectionField::new(mesh.cell_count);
        let mut s = Scratch::new(mesh.cell_count);
        keep_continents_whole(mesh, &mut data, &mut field, &mut s, &Tuning::default());
        assert_eq!(data.crust_type[lone], CONTINENTAL);
        assert_eq!(field.rejoined, 0.0);
    }

    #[test]
    fn a_crumb_rejoins_away_from_the_plate_margin() {
        let sim = Sim::new(4);
        let mesh = &sim.mesh;
        let (mut data, rings) = island(mesh, 0, 3);
        let near = |a: usize, b: usize| mesh.nbrs(a).any(|k| mesh.nbr[k] as usize == b);
        // A crumb two rings out with two coastal cells beside it, one of them
        // put on another plate's margin.
        let (stray, a, b, other) = rings[1]
            .iter()
            .find_map(|&stray| {
                let coast: Vec<usize> = rings[0].iter().copied().filter(|&c| near(stray, c)).collect();
                if coast.len() < 2 {
                    return None;
                }
                let (a, b) = (coast[0], coast[1]);
                mesh.nbrs(a)
                    .map(|k| mesh.nbr[k] as usize)
                    .find(|&m| {
                        m != stray
                            && data.crust_type[m] == OCEANIC
                            && !coast.contains(&m)
                            && !near(m, b)
                            && !near(m, stray)
                    })
                    .map(|other| (stray, a, b, other))
            })
            .expect("no crumb with two coastal cells beside it");
        data.plate_id[other] = 1;
        plant(&mut data, stray);
        let mut field = AdvectionField::new(mesh.cell_count);
        let mut s = Scratch::new(mesh.cell_count);
        keep_continents_whole(mesh, &mut data, &mut field, &mut s, &Tuning::default());
        assert_eq!(data.crust_type[a], OCEANIC, "rejoined on the plate margin");
        assert_eq!(data.crust_type[b], CONTINENTAL, "did not rejoin away from it");
    }

    #[test]
    fn a_running_world_keeps_its_ledger_with_cohesion_on() {
        let mut sim = Sim::new(5);
        sim.crack_great_circles(4, "cohesion");
        let mut rejoined = 0.0;
        for _ in 0..100 {
            sim.tick();
            rejoined += sim.advection().rejoined;
        }
        assert!(rejoined > 0.0, "nothing ever rejoined");
        assert!(sim.mass_drift().abs() < 1e-4, "drift {}", sim.mass_drift());
    }
}
