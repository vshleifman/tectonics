//! One tick of plate motion, mirroring `advect.ts`. Plates move in whole cells,
//! transport is an exact permutation, ownership changes only where crust
//! arrives across a convergent margin the plate overrides, and crust with
//! nowhere to go subducts through the ledger or piles up. See the TypeScript
//! for the geology behind each pass.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData, OCEANIC};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::scratch::{MAX_NBRS, Scratch};
use crate::tuning::Tuning;
use std::collections::HashMap;

const MIN_FOOTHOLD: i32 = 2;
const PLACEMENT_ROUNDS: usize = 6;

/// Per-step events and the conservation ledger.
pub struct AdvectionField {
    /// Source cell each destination drew from, or -1.
    pub winner: Vec<i32>,
    pub accreted: Vec<f32>,
    pub welded: Vec<f32>,
    pub rift_cell: Vec<u8>,
    pub subducted: Vec<u8>,
    /// Thickness each cell took in this step from lateral spreading, net of
    /// what it gave away.
    pub spread_in: Vec<f32>,
    /// Thickness to strip from a continental overriding cell this step, from
    /// ocean subducting under it.
    pub margin_eroded: Vec<f32>,
    pub live_mass: f64,
    pub subducted_this_step: f64,
    pub subducted_total: f64,
    pub created_this_step: f64,
    pub created_total: f64,
    /// Continental mass lost this step to subduction erosion of eaten margins
    /// and to ocean-under-continent margin erosion.
    pub eroded_this_step: f64,
    /// Mass shed this step by roots past the thickness cap.
    pub delaminated_this_step: f64,
    /// Accreted mass this step that could not be taken up at the thickening cap
    /// and foundered into the mantle instead of spilling.
    pub foundered_this_step: f64,
    /// Area turned continental this step, by whether accretion or lateral
    /// spreading carried it past the threshold.
    pub continentalised_by_accretion: f64,
    pub continentalised_by_spreading: f64,
    /// Area of thinned continental crust that reverted to oceanic this step.
    pub decontinentalised: f64,
}

impl AdvectionField {
    pub fn new(n: usize) -> AdvectionField {
        AdvectionField {
            winner: vec![-1; n],
            accreted: vec![0.0; n],
            welded: vec![0.0; n],
            rift_cell: vec![0; n],
            subducted: vec![0; n],
            spread_in: vec![0.0; n],
            margin_eroded: vec![0.0; n],
            live_mass: 0.0,
            subducted_this_step: 0.0,
            subducted_total: 0.0,
            created_this_step: 0.0,
            created_total: 0.0,
            eroded_this_step: 0.0,
            delaminated_this_step: 0.0,
            foundered_this_step: 0.0,
            continentalised_by_accretion: 0.0,
            continentalised_by_spreading: 0.0,
            decontinentalised: 0.0,
        }
    }
}

/// A key that sorts ascending by descending `score`, then ascending `index`:
/// the float's bits made monotonic and inverted, above the index.
#[inline]
fn descending_key(score: f32, index: u32) -> u64 {
    let bits = if score == 0.0 { 0 } else { score.to_bits() };
    let ordered = if bits & 0x8000_0000 != 0 {
        !bits
    } else {
        bits | 0x8000_0000
    };
    ((!ordered as u64) << 32) | index as u64
}

/// Add `x` to an `f32` slot, with the sum taken in double precision.
#[inline]
fn add_f32(slot: &mut f32, x: f64) {
    *slot = (*slot as f64 + x) as f32;
}

#[allow(clippy::too_many_arguments)]
pub fn advect(
    mesh: &Mesh,
    src: &CellData,
    dst: &mut CellData,
    plates: &mut PlateData,
    dt_myr: f64,
    boundaries: &BoundaryField,
    out: &mut AdvectionField,
    s: &mut Scratch,
    t: &Tuning,
) {
    let n = mesh.cell_count;
    let areas = &mesh.areas;
    let area = |i: usize| areas[i] as f64;
    let off = &mesh.nbr_off;
    let nbr = &mesh.nbr;
    let chord = &mesh.chord_dir;
    let range = |i: usize| off[i] as usize..off[i + 1] as usize;

    out.winner.fill(-1);
    out.accreted.fill(0.0);
    out.welded.fill(0.0);
    out.rift_cell.fill(0);
    out.subducted.fill(0);
    out.spread_in.fill(0.0);
    out.margin_eroded.fill(0.0);
    let mut subducted_this_step = 0.0f64;
    let mut eroded_this_step = 0.0f64;
    let mut delaminated_this_step = 0.0f64;
    let mut foundered_this_step = 0.0f64;

    // Mantle heat belongs to the mantle, not the crust, so it stays put.
    dst.mantle_heat.copy_from_slice(&src.mantle_heat);

    let width = (4.0 * std::f64::consts::PI / n as f64).sqrt();

    // --- Quantised rigid motion ----------------------------------------------
    s.pack_plates(plates);
    s.plate_flag.fill(0);
    for (&id, props) in plates.by_id.iter_mut() {
        let w = props.omega;
        props.pending = (props.pending + (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt() * dt_myr)
            .min(2.0 * width);
        if props.pending >= width {
            props.pending -= width;
            s.plate_flag[id as usize] = 1;
        }
    }
    let stepping_flags = std::mem::take(&mut s.plate_flag);
    let is_stepping =
        |p: i32| p >= 0 && (p as usize) < stepping_flags.len() && stepping_flags[p as usize] == 1;

    // --- Where this step carries each cell's crust ---------------------------
    s.forward.fill(-1);
    s.forward_arc.fill(-1);
    let c = &mesh.centres;
    for cell in 0..n {
        let plate = src.plate_id[cell];
        if !is_stepping(plate) || !s.is_live(plate) {
            continue;
        }
        let w = s.omega(plate);
        let (px, py, pz) = (
            c[cell * 3] as f64,
            c[cell * 3 + 1] as f64,
            c[cell * 3 + 2] as f64,
        );
        let vx = w[1] * pz - w[2] * py;
        let vy = w[2] * px - w[0] * pz;
        let vz = w[0] * py - w[1] * px;
        let speed = (vx * vx + vy * vy + vz * vz).sqrt();
        if speed == 0.0 {
            continue;
        }
        let mut best = -1;
        let mut best_arc = -1;
        let mut best_dot = 0.0;
        for k in range(cell) {
            let a = (chord[k * 3] as f64 * vx
                + chord[k * 3 + 1] as f64 * vy
                + chord[k * 3 + 2] as f64 * vz)
                / speed;
            if a > best_dot {
                best_dot = a;
                best = nbr[k];
                best_arc = mesh.nbr_arc[k];
            }
        }
        s.forward[cell] = best;
        s.forward_arc[cell] = best_arc;
    }

    // Does `a`'s crust ride over `b`'s when the two meet?
    let overrides = |a: usize, b: usize| -> bool {
        let a_cont = src.crust_type[a] == CONTINENTAL;
        let b_cont = src.crust_type[b] == CONTINENTAL;
        if a_cont != b_cont {
            return a_cont;
        }
        if src.age[a] != src.age[b] {
            return src.age[a] < src.age[b];
        }
        a < b
    };

    let thickness_cap = t.max_crust_thickness.max(t.continental_thickness_threshold);

    // --- Pass 1: who takes which cell, decided a whole margin at a time ------
    s.new_plate.copy_from_slice(&src.plate_id);
    s.claimant.fill(-1);
    s.eaten.fill(0);

    let buoyancy = |cell: usize| -> f64 {
        if src.crust_type[cell] == CONTINENTAL {
            -t.max_oceanic_age
        } else {
            src.age[cell] as f64
        }
    };

    let mut contacts = 0usize;
    let mut margins: HashMap<i64, (f64, f64)> = HashMap::new();
    for cell in 0..n {
        let plate = src.plate_id[cell];
        let d = s.forward[cell];
        if plate < 0 || d < 0 {
            continue;
        }
        let d = d as usize;
        let held = src.plate_id[d];
        if held < 0 || held == plate {
            continue;
        }
        let arc = s.forward_arc[cell];
        if arc < 0 || boundaries.arc_kind[arc as usize] != CONVERGENT {
            continue;
        }
        s.from_cell[contacts] = cell as i32;
        s.onto_cell[contacts] = d as i32;
        contacts += 1;
        let tally = margins
            .entry(plate as i64 * 65536 + held as i64)
            .or_insert((0.0, 0.0));
        tally.0 += buoyancy(cell);
        tally.1 += buoyancy(d);
    }

    for i in 0..contacts {
        let cell = s.from_cell[i] as usize;
        let d = s.onto_cell[i] as usize;
        let plate = src.plate_id[cell];
        let Some(&(mine, theirs)) = margins.get(&(plate as i64 * 65536 + src.plate_id[d] as i64))
        else {
            continue;
        };
        if mine >= theirs {
            continue;
        }
        if src.crust_type[d] == CONTINENTAL && src.crust_type[cell] != CONTINENTAL {
            continue;
        }
        let foothold = range(d)
            .filter(|&k| src.plate_id[nbr[k] as usize] == plate)
            .count() as i32;
        if foothold < MIN_FOOTHOLD {
            continue;
        }
        if src.crust_type[d] == CONTINENTAL
            && src.crust_type[cell] == CONTINENTAL
            && src.thickness[cell] as f64 + src.thickness[d] as f64 > thickness_cap
        {
            continue;
        }
        let prev = s.claimant[d];
        if prev < 0 || overrides(cell, prev as usize) {
            s.claimant[d] = cell as i32;
        }
    }

    for d in 0..n {
        let claim = s.claimant[d];
        if claim < 0 {
            continue;
        }
        s.new_plate[d] = src.plate_id[claim as usize];
        s.eaten[d] = 1;
    }

    // --- Pass 1b: a locked front stalls the crust behind it ------------------
    s.stalled.fill(0);
    let mut jam_len = 0usize;
    for cell in 0..n {
        let plate = src.plate_id[cell];
        let ahead = s.forward[cell];
        if plate < 0 || ahead < 0 {
            continue;
        }
        let ahead = ahead as usize;
        if src.plate_id[ahead] == plate || s.new_plate[ahead] == plate {
            continue;
        }
        if !overrides(cell, ahead) {
            continue;
        }
        s.stalled[cell] = 1;
        s.queue[jam_len] = cell as i32;
        jam_len += 1;
    }
    s.behind_start.fill(0);
    for cell in 0..n {
        let f = s.forward[cell];
        if f >= 0 {
            s.behind_start[f as usize + 1] += 1;
        }
    }
    for i in 0..n {
        s.behind_start[i + 1] += s.behind_start[i];
    }
    s.fill.copy_from_slice(&s.behind_start[..n]);
    for cell in 0..n {
        let f = s.forward[cell];
        if f >= 0 {
            let f = f as usize;
            s.behind[s.fill[f] as usize] = cell as i32;
            s.fill[f] += 1;
        }
    }
    let mut head = 0;
    while head < jam_len {
        let tc = s.queue[head] as usize;
        head += 1;
        for k in s.behind_start[tc] as usize..s.behind_start[tc + 1] as usize {
            let b = s.behind[k] as usize;
            if s.stalled[b] != 0 || src.plate_id[b] != src.plate_id[tc] {
                continue;
            }
            s.stalled[b] = 1;
            s.queue[jam_len] = b as i32;
            jam_len += 1;
        }
    }

    // --- Pass 2: transport, as an exact permutation --------------------------
    s.source_of.fill(-1);
    s.claimed.fill(0);
    s.order.clear();
    for d in 0..n {
        let plate = s.new_plate[d];
        if plate < 0 {
            continue;
        }
        if !is_stepping(plate) {
            if src.plate_id[d] == plate {
                s.source_of[d] = d as i32;
                s.claimed[d] = 1;
            }
            continue;
        }
        if s.stalled[d] != 0 && src.plate_id[d] == plate {
            s.source_of[d] = d as i32;
            s.claimed[d] = 1;
            continue;
        }
        if !s.is_live(plate) {
            continue;
        }
        let w = s.omega(plate);
        let (px, py, pz) = (c[d * 3] as f64, c[d * 3 + 1] as f64, c[d * 3 + 2] as f64);
        let vx = w[1] * pz - w[2] * py;
        let vy = w[2] * px - w[0] * pz;
        let vz = w[0] * py - w[1] * px;
        let speed = (vx * vx + vy * vy + vz * vz).sqrt();
        let inv = if speed == 0.0 { 0.0 } else { -1.0 / speed };
        let base = d * MAX_NBRS;
        let mut count = 0usize;
        for k in range(d) {
            let src_cell = nbr[k];
            if src.plate_id[src_cell as usize] != plate {
                continue;
            }
            let score = (chord[k * 3] as f64 * vx
                + chord[k * 3 + 1] as f64 * vy
                + chord[k * 3 + 2] as f64 * vz)
                * inv
                + if s.forward[src_cell as usize] == d as i32 {
                    2.0
                } else {
                    0.0
                };
            let mut i = count;
            while i > 0 && (s.cand_score[base + i - 1] as f64) < score {
                s.cand[base + i] = s.cand[base + i - 1];
                s.cand_score[base + i] = s.cand_score[base + i - 1];
                i -= 1;
            }
            s.cand[base + i] = src_cell;
            s.cand_score[base + i] = score as f32;
            count += 1;
        }
        s.cand_count[d] = count as i32;
        if count == 0 {
            continue;
        }
        s.order.push(descending_key(s.cand_score[base], d as u32));
    }
    // Best-aligned pairs first; ties by cell index, as a stable sort would.
    s.order.sort_unstable();
    for q in 0..s.order.len() {
        let d = (s.order[q] & 0xffff_ffff) as usize;
        let base = d * MAX_NBRS;
        for i in 0..s.cand_count[d] as usize {
            let src_cell = s.cand[base + i] as usize;
            if s.claimed[src_cell] != 0 {
                continue;
            }
            s.claimed[src_cell] = 1;
            s.source_of[d] = src_cell as i32;
            break;
        }
    }

    // --- Pass 2b: a neighbour follows into a vacated cell --------------------
    for d in 0..n {
        if s.source_of[d] >= 0 || s.new_plate[d] < 0 {
            continue;
        }
        let mut taker: i32 = -1;
        for k in range(d) {
            let cand = nbr[k] as usize;
            if s.claimed[cand] != 0 || s.forward[cand] != d as i32 {
                continue;
            }
            let plate = src.plate_id[cand];
            if plate < 0 || plate == s.new_plate[d] {
                continue;
            }
            let foothold = range(d)
                .filter(|&j| src.plate_id[nbr[j] as usize] == plate)
                .count();
            if foothold < 2 {
                continue;
            }
            if taker < 0 || overrides(cand, taker as usize) {
                taker = cand as i32;
            }
        }
        if taker < 0 {
            continue;
        }
        let taker = taker as usize;
        s.claimed[taker] = 1;
        s.source_of[d] = taker as i32;
        s.new_plate[d] = src.plate_id[taker];
    }

    let copy_cell = |dst: &mut CellData, source: usize, dest: usize| {
        dst.elevation[dest] = src.elevation[source];
        dst.plate_id[dest] = src.plate_id[source];
        dst.crust_type[dest] = src.crust_type[source];
        dst.age[dest] = src.age[source];
        dst.density[dest] = src.density[source];
        dst.weakness[dest] = src.weakness[source];
        dst.slab[dest] = src.slab[source];
        dst.rift_side[dest] = src.rift_side[source];
        dst.thickness[dest] = if source == dest {
            src.thickness[source]
        } else {
            (src.thickness[source] as f64 * area(source) / area(dest)) as f32
        };
    };

    for d in 0..n {
        let plate = s.new_plate[d];
        if plate < 0 {
            copy_cell(dst, d, d);
            continue;
        }
        let source = s.source_of[d];
        if source < 0 {
            if s.claimed[d] != 0 {
                // Leave an empty rift placeholder for the crust lifecycle.
                dst.elevation[d] = 0.0;
                dst.plate_id[d] = plate;
                dst.crust_type[d] = OCEANIC;
                dst.age[d] = 0.0;
                dst.density[d] = t.rift_density as f32;
                dst.thickness[d] = 0.0;
                dst.weakness[d] = src.weakness[d];
                dst.slab[d] = 0.0;
                dst.rift_side[d] = 0;
                out.rift_cell[d] = 1;
            } else {
                copy_cell(dst, d, d);
                dst.plate_id[d] = plate;
                s.claimed[d] = 1;
                out.winner[d] = d as i32;
            }
            continue;
        }
        copy_cell(dst, source as usize, d);
        dst.plate_id[d] = plate;
        out.winner[d] = source;
    }

    // --- Pass 3: dispose of the crust that had nowhere to go -----------------
    for cell in 0..n {
        if s.claimed[cell] != 0 || src.plate_id[cell] < 0 {
            continue;
        }
        let ahead = s.forward[cell];
        let blocked_by_other = ahead >= 0 && src.plate_id[ahead as usize] != src.plate_id[cell];
        let mass = area(cell) * src.thickness[cell] as f64;
        if src.crust_type[cell] == CONTINENTAL {
            if s.eaten[cell] != 0 {
                // Scraped off by the plate that took the cell: nearly all of it
                // welds on, the rest is the trench's erosion of the margin.
                let eroded = t.subduction_erosion * mass;
                subducted_this_step += eroded;
                eroded_this_step += eroded;
                add_f32(&mut out.welded[cell], (mass - eroded) / area(cell));
                out.subducted[cell] = 1;
                if out.rift_cell[cell] == 0 {
                    dst.crust_type[cell] = CONTINENTAL;
                    dst.density[cell] = t.continental_density as f32;
                }
            } else {
                add_f32(&mut out.welded[cell], mass / area(cell));
            }
        } else if s.eaten[cell] != 0 || blocked_by_other {
            let into = if s.eaten[cell] != 0 {
                cell
            } else {
                ahead as usize
            };
            let accreted = t.accretion_fraction * mass;
            subducted_this_step += mass - accreted;
            add_f32(&mut out.accreted[into], accreted / area(into));
            out.subducted[into] = 1;
            if dst.crust_type[into] == CONTINENTAL && out.rift_cell[into] == 0 {
                add_f32(
                    &mut out.margin_eroded[into],
                    t.margin_erosion * mass / area(into),
                );
            }
        } else {
            add_f32(&mut out.welded[cell], mass / area(cell));
        }
    }

    // --- Pass 4: apply this step's gains, handing on what will not fit -------
    let max_gain = t.max_thickening_per_myr * dt_myr;
    let mut surplus = std::mem::take(&mut s.f32a);
    let mut next = std::mem::take(&mut s.f32b);
    let mut headroom = std::mem::take(&mut s.f32c);
    let mut incoming = std::mem::take(&mut s.f32d);
    surplus.fill(0.0);
    // `margin_eroded` is reused after the strip as remaining accretion this
    // cell may still absorb at the thickening cap.
    for d in 0..n {
        let eroded_h = out.margin_eroded[d] as f64;
        if eroded_h > 0.0 && out.rift_cell[d] == 0 {
            let available = (dst.thickness[d] as f64 - t.rift_thickness_floor).max(0.0);
            let take = eroded_h.min(available);
            add_f32(&mut dst.thickness[d], -take);
            eroded_this_step += take * area(d);
            subducted_this_step += take * area(d);
        }
        out.margin_eroded[d] = if out.rift_cell[d] != 0 {
            0.0
        } else {
            max_gain as f32
        };
        let acc = out.accreted[d] as f64;
        if acc <= 0.0 {
            continue;
        }
        let cap_room = if out.rift_cell[d] != 0 {
            0.0
        } else {
            (thickness_cap - dst.thickness[d] as f64).max(0.0)
        };
        let take = acc.min(out.margin_eroded[d] as f64).min(cap_room);
        add_f32(&mut dst.thickness[d], take);
        out.margin_eroded[d] = (out.margin_eroded[d] as f64 - take) as f32;
        surplus[d] = (acc - take) as f32;
    }

    for _round in 0..PLACEMENT_ROUNDS {
        if !surplus.iter().any(|&x| x > 0.0) {
            break;
        }
        for d in 0..n {
            headroom[d] = if out.rift_cell[d] != 0 {
                0.0
            } else {
                (thickness_cap - dst.thickness[d] as f64)
                    .max(0.0)
                    .min(out.margin_eroded[d] as f64) as f32
            };
        }
        incoming.fill(0.0);
        next.fill(0.0);
        for d in 0..n {
            let have = surplus[d] as f64;
            if have <= 0.0 {
                continue;
            }
            let own = dst.plate_id[d];
            let accepts = |m: usize| dst.plate_id[m] == own && out.rift_cell[m] == 0;
            let mut room = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    room += headroom[m] as f64;
                }
            }
            let crowded = room <= 0.0;
            let weight_of = |m: usize| {
                if crowded {
                    1.0 / (1.0 + dst.thickness[m] as f64)
                } else {
                    headroom[m] as f64
                }
            };
            let mut weight = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    weight += weight_of(m);
                }
            }
            if weight <= 0.0 {
                add_f32(&mut next[d], have);
                continue;
            }
            for k in range(d) {
                let m = nbr[k] as usize;
                if !accepts(m) {
                    continue;
                }
                let w = weight_of(m);
                if w <= 0.0 {
                    continue;
                }
                add_f32(&mut incoming[m], have * (w / weight) * area(d) / area(m));
            }
        }
        for d in 0..n {
            let inc = incoming[d] as f64;
            if inc == 0.0 {
                continue;
            }
            let place = inc.min(headroom[d] as f64);
            add_f32(&mut dst.thickness[d], place);
            add_f32(&mut out.margin_eroded[d], -place);
            add_f32(&mut next[d], inc - place);
        }
        std::mem::swap(&mut surplus, &mut next);
    }
    for d in 0..n {
        if surplus[d] > 0.0 {
            foundered_this_step += surplus[d] as f64 * area(d);
            subducted_this_step += surplus[d] as f64 * area(d);
            surplus[d] = 0.0;
        }
    }

    for d in 0..n {
        let welded = out.welded[d] as f64;
        if welded == 0.0 {
            continue;
        }
        let held = if out.rift_cell[d] != 0 {
            0.0
        } else {
            welded.min((thickness_cap - dst.thickness[d] as f64).max(0.0))
        };
        add_f32(&mut dst.thickness[d], held);
        surplus[d] = (welded - held) as f32;
    }

    for round in 0..PLACEMENT_ROUNDS {
        if !surplus.iter().any(|&x| x > 0.0) {
            break;
        }
        for d in 0..n {
            headroom[d] = if out.rift_cell[d] != 0 {
                0.0
            } else {
                (thickness_cap - dst.thickness[d] as f64).max(0.0) as f32
            };
        }
        incoming.fill(0.0);
        next.fill(0.0);
        for d in 0..n {
            let have = surplus[d] as f64;
            if have <= 0.0 {
                continue;
            }
            let own = dst.plate_id[d];
            let accepts = |m: usize| dst.plate_id[m] == own && out.rift_cell[m] == 0;
            let mut room = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    room += headroom[m] as f64;
                }
            }
            let crowded = room <= 0.0;
            let weight_of = |m: usize| {
                if crowded {
                    1.0 / (1.0 + dst.thickness[m] as f64)
                } else {
                    headroom[m] as f64
                }
            };
            let mut weight = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    weight += weight_of(m);
                }
            }
            if weight <= 0.0 {
                add_f32(&mut next[d], have);
                continue;
            }
            for k in range(d) {
                let m = nbr[k] as usize;
                if !accepts(m) {
                    continue;
                }
                let w = weight_of(m);
                if w <= 0.0 {
                    continue;
                }
                add_f32(&mut incoming[m], have * (w / weight) * area(d) / area(m));
            }
        }
        let last = round == PLACEMENT_ROUNDS - 1;
        for d in 0..n {
            let inc = incoming[d] as f64;
            if inc == 0.0 {
                continue;
            }
            let place = if last {
                inc
            } else {
                inc.min(headroom[d] as f64)
            };
            add_f32(&mut dst.thickness[d], place);
            add_f32(&mut next[d], inc - place);
        }
        std::mem::swap(&mut surplus, &mut next);
    }
    for d in 0..n {
        if surplus[d] > 0.0 {
            add_f32(&mut dst.thickness[d], surplus[d] as f64);
        }
    }

    // --- Pass 4a: thickened crust spreads under its own weight ---------------
    let spread_rate = (t.crust_spread_rate * dt_myr).clamp(0.0, 0.5);
    if spread_rate > 0.0 {
        let before = &mut surplus;
        let flow = &mut next;
        before.copy_from_slice(&dst.thickness);
        flow.fill(0.0);
        for d in 0..n {
            let excess = before[d] as f64 - t.crust_spread_start;
            if excess <= 0.0 || out.rift_cell[d] != 0 {
                continue;
            }
            let own = dst.plate_id[d];
            let mut deficit = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if dst.plate_id[m] != own
                    || out.rift_cell[m] != 0
                    || dst.crust_type[m] != CONTINENTAL
                {
                    continue;
                }
                if before[m] < before[d] {
                    deficit += before[d] as f64 - before[m] as f64;
                }
            }
            if deficit <= 0.0 {
                continue;
            }
            let give = excess.min(deficit) * spread_rate;
            add_f32(&mut flow[d], -give);
            for k in range(d) {
                let m = nbr[k] as usize;
                if dst.plate_id[m] != own
                    || out.rift_cell[m] != 0
                    || dst.crust_type[m] != CONTINENTAL
                    || before[m] >= before[d]
                {
                    continue;
                }
                let share = (before[d] as f64 - before[m] as f64) / deficit;
                add_f32(&mut flow[m], give * share * area(d) / area(m));
            }
        }
        for d in 0..n {
            add_f32(&mut dst.thickness[d], flow[d] as f64);
            out.spread_in[d] = flow[d].max(0.0);
        }
    }

    // --- Pass 4d: an over-thick root founders, slowly ------------------------
    let shed_per_step = (t.delamination_rate * dt_myr).max(0.0);
    if shed_per_step > 0.0 {
        for d in 0..n {
            let over = dst.thickness[d] as f64 - thickness_cap;
            if over <= 0.0 {
                continue;
            }
            let shed = over.min(shed_per_step);
            add_f32(&mut dst.thickness[d], -shed);
            subducted_this_step += shed * area(d);
            delaminated_this_step += shed * area(d);
        }
    }

    // --- Pass 4c: a continent rifts by stretching, not by melting ------------
    for d in 0..n {
        if out.rift_cell[d] == 0 {
            continue;
        }
        let plate = dst.plate_id[d];
        let donor = |m: usize| dst.plate_id[m] == plate && dst.crust_type[m] == CONTINENTAL;
        let mut available = 0.0f64;
        for k in range(d) {
            let m = nbr[k] as usize;
            if donor(m) {
                available += (dst.thickness[m] as f64 - t.rift_thickness_floor).max(0.0) * area(m);
            }
        }
        if available <= 0.0 {
            continue;
        }
        let wanted = available.min(t.oceanic_thickness * area(d));
        let mut pulled = 0.0f64;
        for k in range(d) {
            let m = nbr[k] as usize;
            if !(dst.plate_id[m] == plate && dst.crust_type[m] == CONTINENTAL) {
                continue;
            }
            let spare = (dst.thickness[m] as f64 - t.rift_thickness_floor).max(0.0) * area(m);
            if spare <= 0.0 {
                continue;
            }
            let take = wanted * spare / available;
            add_f32(&mut dst.thickness[m], -take / area(m));
            pulled += take;
        }
        out.rift_cell[d] = 0;
        dst.thickness[d] = (pulled / area(d)) as f32;
        dst.age[d] = 0.0;
        if dst.thickness[d] as f64 >= t.continental_thickness_threshold {
            dst.crust_type[d] = CONTINENTAL;
            dst.density[d] = t.continental_density as f32;
        } else {
            dst.crust_type[d] = OCEANIC;
            dst.density[d] = t.oceanic_density_young as f32;
        }
    }

    // --- Pass 4b: the front has flexural strength ----------------------------
    let labels = &mut s.i32a;
    for _ in 0..2 {
        labels.copy_from_slice(&dst.plate_id);
        for d in 0..n {
            let own = labels[d];
            if own < 0 {
                continue;
            }
            let mut best_plate = -1;
            let mut best_count = 0;
            for k in range(d) {
                let p = labels[nbr[k] as usize];
                if p < 0 || p == own {
                    continue;
                }
                let count = range(d).filter(|&j| labels[nbr[j] as usize] == p).count();
                if count > best_count {
                    best_count = count;
                    best_plate = p;
                }
            }
            if best_count >= 4 {
                dst.plate_id[d] = best_plate;
            }
        }
    }

    s.f32a = surplus;
    s.f32b = next;
    s.f32c = headroom;
    s.f32d = incoming;
    s.plate_flag = stepping_flags;

    out.subducted_this_step = subducted_this_step;
    out.subducted_total += subducted_this_step;
    out.eroded_this_step = eroded_this_step;
    out.delaminated_this_step = delaminated_this_step;
    out.foundered_this_step = foundered_this_step;
}

#[cfg(test)]
mod tests {
    use crate::world::Sim;

    #[test]
    fn transport_is_a_permutation() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "perm");
        for _ in 0..20 {
            sim.tick();
            let mut used = vec![false; sim.mesh.cell_count];
            for &w in &sim.advection().winner {
                if w < 0 {
                    continue;
                }
                assert!(!used[w as usize], "source {w} feeds two destinations");
                used[w as usize] = true;
            }
        }
    }
}
