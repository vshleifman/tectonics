//! Rifting, mirroring `rifting.ts`: a plate tears when the tension across it
//! beats the strength of the weakest line through it, and the far side of that
//! line becomes a new plate.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{Vec3, clamp01, len3};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateProps};
use crate::registry::Registry;
use crate::scratch::Scratch;
use crate::tuning::{SimOptions, Tuning};

const CUT_BAND: f64 = 0.06;
const SCAN_BINS: usize = 64;

struct Cut {
    offset: f64,
    weakness: f64,
    heat: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn update_rifting(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    dt_myr: f64,
    registry: &mut Registry,
    s: &mut Scratch,
    t: &Tuning,
    o: &SimOptions,
) {
    // Weakness heals, but barely.
    let keep = (-dt_myr / t.weakness_decay_time.max(1e-6)).exp();
    for w in data.weakness.iter_mut() {
        *w = (*w as f64 * keep) as f32;
    }
    if o.supercontinent_insulation {
        for i in 0..mesh.cell_count {
            let rate = if data.crust_type[i] == CONTINENTAL {
                t.insulation_rate
            } else {
                -t.insulation_cooling
            };
            data.mantle_heat[i] = clamp01(data.mantle_heat[i] as f64 + rate * dt_myr) as f32;
        }
    }

    // Each plate's cells by one counting sort, visited in order of lowest cell.
    let n = mesh.cell_count;
    let max_id = data.plate_id.iter().copied().max().unwrap_or(-1);
    let bound = (max_id + 1).max(0) as usize;
    let mut start = vec![0usize; bound + 1];
    let mut area_of = vec![0.0f64; bound];
    let mut visit: Vec<i32> = Vec::new();
    let mut total_area = 0.0f64;
    for i in 0..n {
        total_area += mesh.area(i);
        let id = data.plate_id[i];
        if id < 0 {
            continue;
        }
        let p = id as usize;
        if start[p + 1] == 0 {
            visit.push(id);
        }
        start[p + 1] += 1;
        area_of[p] += mesh.area(i);
    }
    for p in 0..bound {
        start[p + 1] += start[p];
    }
    let mut cells_by_plate = std::mem::take(&mut s.i32b);
    let mut write = start.clone();
    for i in 0..n {
        let id = data.plate_id[i];
        if id >= 0 {
            cells_by_plate[write[id as usize]] = i as i32;
            write[id as usize] += 1;
        }
    }

    let min_area = t.min_rift_area * total_area;
    for &id in &visit {
        let p = id as usize;
        let cells = &cells_by_plate[start[p]..start[p + 1]];
        if area_of[p] < 2.0 * min_area {
            continue;
        }
        let stress = plates.stress.get(&id).cloned();
        let axis = stress
            .as_ref()
            .map_or_else(|| hottest_axis(mesh, data, cells), |st| st.axis);
        let Some(cut) = weakest_cut(mesh, data, cells, axis) else {
            continue;
        };
        let drive = stress.as_ref().map_or(0.0, |st| st.tension)
            + if o.supercontinent_insulation {
                t.insulation_drive * cut.heat
            } else {
                0.0
            };
        let strength = t.rift_strength
            * (1.0 - t.weakness_relief * cut.weakness - t.insulation_weakening * cut.heat).max(0.0);
        if drive < strength {
            continue;
        }
        split_plate(
            mesh, data, plates, id, cells, axis, cut.offset, registry, min_area, s, t,
        );
    }
    s.i32b = cells_by_plate;
}

fn hottest_axis(mesh: &Mesh, data: &CellData, cells: &[i32]) -> Vec3 {
    let c = &mesh.centres;
    let (mut x, mut y, mut z) = (0.0f64, 0.0f64, 0.0f64);
    for &cell in cells {
        let i = cell as usize;
        let w = data.mantle_heat[i] as f64 * mesh.area(i);
        x += c[i * 3] as f64 * w;
        y += c[i * 3 + 1] as f64 * w;
        z += c[i * 3 + 2] as f64 * w;
    }
    let l = len3(x, y, z);
    if l < 1e-12 {
        [1.0, 0.0, 0.0]
    } else {
        [x / l, y / l, z / l]
    }
}

#[inline]
fn along(mesh: &Mesh, cell: usize, axis: Vec3) -> f64 {
    let c = &mesh.centres;
    c[cell * 3] as f64 * axis[0]
        + c[cell * 3 + 1] as f64 * axis[1]
        + c[cell * 3 + 2] as f64 * axis[2]
}

/// The softest line across the plate perpendicular to `axis`, scanned over
/// bins and kept clear of the ends.
fn weakest_cut(mesh: &Mesh, data: &CellData, cells: &[i32], axis: Vec3) -> Option<Cut> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &cell in cells {
        let v = along(mesh, cell as usize, axis);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let span = hi - lo;
    if !span.is_finite() || span <= 1e-6 {
        return None;
    }
    let mut area_bin = [0.0f64; SCAN_BINS];
    let mut weak_bin = [0.0f64; SCAN_BINS];
    let mut heat_bin = [0.0f64; SCAN_BINS];
    for &cell in cells {
        let i = cell as usize;
        let u = (along(mesh, i, axis) - lo) / span;
        let bin = ((u * SCAN_BINS as f64).floor().max(0.0) as usize).min(SCAN_BINS - 1);
        let a = mesh.area(i);
        area_bin[bin] += a;
        weak_bin[bin] += data.weakness[i] as f64 * a;
        heat_bin[bin] += data.mantle_heat[i] as f64 * a;
    }
    let half = ((CUT_BAND * SCAN_BINS as f64).round() as usize).max(1);
    let first = (0.25 * SCAN_BINS as f64).round() as usize;
    let last = (0.75 * SCAN_BINS as f64).round() as usize;
    let mut best: Option<Cut> = None;
    let mut best_score = f64::NEG_INFINITY;
    for centre in first..=last {
        let (mut weight, mut weakness, mut heat) = (0.0, 0.0, 0.0);
        for b in centre.saturating_sub(half)..=(centre + half).min(SCAN_BINS - 1) {
            weight += area_bin[b];
            weakness += weak_bin[b];
            heat += heat_bin[b];
        }
        if weight <= 0.0 {
            continue;
        }
        let score = weakness / weight;
        if score > best_score {
            best_score = score;
            best = Some(Cut {
                offset: lo + span * (centre as f64 + 0.5) / SCAN_BINS as f64,
                weakness: score,
                heat: heat / weight,
            });
        }
    }
    best
}

/// Cut a plate along the line at `offset`, giving the largest connected piece
/// of the far side a new id and stamping both new margins as weak.
#[allow(clippy::too_many_arguments)]
fn split_plate(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    id: i32,
    cells: &[i32],
    axis: Vec3,
    offset: f64,
    registry: &mut Registry,
    min_area: f64,
    s: &mut Scratch,
    t: &Tuning,
) {
    let far = &mut s.u8a;
    let seen = &mut s.u8b;
    let mut far_count = 0;
    for &cell in cells {
        let i = cell as usize;
        if along(mesh, i, axis) > offset {
            far[i] = 1;
            far_count += 1;
        }
    }
    if far_count == 0 {
        return;
    }

    let queue = &mut s.queue;
    let mut tail = 0usize;
    let (mut best_start, mut best_end, mut best_area) = (0usize, 0usize, 0.0f64);
    for &first in cells {
        let f = first as usize;
        if far[f] == 0 || seen[f] != 0 {
            continue;
        }
        let piece_start = tail;
        queue[tail] = first;
        tail += 1;
        seen[f] = 1;
        let mut area = 0.0;
        let mut head = piece_start;
        while head < tail {
            let cell = queue[head] as usize;
            head += 1;
            area += mesh.area(cell);
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if far[m] != 0 && seen[m] == 0 {
                    seen[m] = 1;
                    queue[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        if area > best_area {
            best_area = area;
            best_start = piece_start;
            best_end = tail;
        }
    }
    for &cell in cells {
        far[cell as usize] = 0;
        seen[cell as usize] = 0;
    }
    if best_area < min_area {
        return;
    }

    let new_id = registry.allocate();
    let omega = plates.by_id.get(&id).map_or([0.0, 0.0, 0.0], |p| p.omega);
    plates.by_id.insert(
        new_id,
        PlateProps {
            omega,
            pending: 0.0,
        },
    );
    for q in best_start..best_end {
        data.plate_id[queue[q] as usize] = new_id;
    }
    let margin = t.rift_margin_weakness as f32;
    for q in best_start..best_end {
        let cell = queue[q] as usize;
        for k in mesh.nbrs(cell) {
            let m = mesh.nbr[k] as usize;
            if data.plate_id[m] == new_id {
                continue;
            }
            data.weakness[cell] = data.weakness[cell].max(margin);
            data.weakness[m] = data.weakness[m].max(margin);
        }
    }
}
