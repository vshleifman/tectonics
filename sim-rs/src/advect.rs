//! One tick of plate motion, mirroring `advect.ts`. Plates move in whole cells,
//! transport is an exact permutation, ownership changes only where crust
//! arrives across a convergent margin the plate overrides, and crust with
//! nowhere to go subducts through the ledger or piles up. See the TypeScript
//! for the geology behind each pass.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData, OCEANIC, mixed_share};
use crate::mesh::Mesh;
use crate::par::{self, Disjoint};
use crate::plates::PlateData;
use crate::scratch::{MAX_NBRS, PackedPlates, Scratch};
use crate::tuning::{CRATON_KEEL, Tuning};
use std::collections::HashMap;

const MIN_FOOTHOLD: i32 = 2;
const PLACEMENT_ROUNDS: usize = 6;
/// Most cells a gap inside a plate is handed back through towards the margin.
const HOLE_CHAIN_LIMIT: usize = 256;
/// Rings of cells around a gap still inside a plate that give up crust to
/// close it.
const HOLE_DONOR_RINGS: usize = 2;

/// Per-step events and the conservation ledger.
pub struct AdvectionField {
    /// Source cell each destination drew from, or -1.
    pub winner: Vec<i32>,
    pub accreted: Vec<f32>,
    pub welded: Vec<f32>,
    /// Of `welded`, the thickness that is continental crust, and that times
    /// its age and its keel: welded crust keeps its history in the cell it
    /// welds onto rather than taking on that of whatever moved in.
    pub welded_continental: Vec<f32>,
    pub welded_age: Vec<f32>,
    pub welded_keel: Vec<f32>,
    /// `welded` times its plume share, then the same for the welded crust
    /// spilling on to neighbours: tracer mass in transit.
    pub welded_share: Vec<f32>,
    spill_share: Vec<f32>,
    spill_share_next: Vec<f32>,
    spill_share_in: Vec<f32>,
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
    /// Area turned continental this step by trapped ocean filling in.
    pub continentalised_by_infill: f64,
    /// Thickness each cell gained this step over a plume.
    pub plume_in: Vec<f32>,
    /// Crust mass plumes made this step.
    pub plume_created_this_step: f64,
    /// Thickened ocean crust that foundered into a hot mantle this step.
    pub dripped_this_step: f64,
    /// Area turned continental this step over plumes.
    pub continentalised_by_plume: f64,
    /// Area of ocean floor turned continental this step with neither accretion
    /// nor lateral spreading into it: crust piled up where the transport had
    /// nowhere to put it, or spilled over from a full arc cell beside it.
    pub continentalised_by_piling: f64,
    /// 1 where a cell turned continental by piling this step.
    pub piled: Vec<u8>,
    /// Where continental crust lost its cell this step: 1 overrun by another
    /// plate, 2 left with nowhere to go. Either way its mass welds onto the
    /// cell, but the cell takes the age and keel of the crust that moved in.
    pub overrun: Vec<u8>,
    /// Continental area recycled into the mantle at active margins this step.
    pub continental_recycled: f64,
    /// Recycling owed but not yet taken, as area: margins are taken a whole
    /// cell at a time.
    pub recycle_debt: f64,
    /// Running rate (share of the globe per Myr) at which everything but
    /// recycling grows the continents.
    pub continental_growth: f64,
    /// Continental share of the globe at the end of the last step.
    pub continental_share_before: Option<f64>,
    /// Area of thinned continental crust that reverted to oceanic this step.
    pub decontinentalised: f64,
    /// Area vacated this step with no other plate beside it: a gap the
    /// quantised transport tore in a plate's interior, not a divergent margin.
    pub interior_holes: f64,
    /// Of `interior_holes`, the area no nearby crust of the plate could fill,
    /// left to open as a rift.
    pub interior_holes_unfilled: f64,
    /// Continental area left with nowhere to go this step (`overrun == 2`).
    pub torn_continent: f64,
    /// Continental area the cohesion pass put back onto its landmass this step.
    pub rejoined: f64,
}

impl AdvectionField {
    pub fn new(n: usize) -> AdvectionField {
        AdvectionField {
            winner: vec![-1; n],
            accreted: vec![0.0; n],
            welded: vec![0.0; n],
            welded_continental: vec![0.0; n],
            welded_age: vec![0.0; n],
            welded_keel: vec![0.0; n],
            welded_share: vec![0.0; n],
            spill_share: vec![0.0; n],
            spill_share_next: vec![0.0; n],
            spill_share_in: vec![0.0; n],
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
            continentalised_by_infill: 0.0,
            plume_in: vec![0.0; n],
            plume_created_this_step: 0.0,
            dripped_this_step: 0.0,
            continentalised_by_plume: 0.0,
            continentalised_by_piling: 0.0,
            piled: vec![0; n],
            overrun: vec![0; n],
            continental_recycled: 0.0,
            recycle_debt: 0.0,
            continental_growth: 0.0,
            continental_share_before: None,
            decontinentalised: 0.0,
            interior_holes: 0.0,
            interior_holes_unfilled: 0.0,
            torn_continent: 0.0,
            rejoined: 0.0,
        }
    }

    /// Everything tallied afresh each step, so a tick taken in several steps
    /// can report its sum.
    pub fn step_tallies_mut(&mut self) -> [&mut f64; 18] {
        [
            &mut self.subducted_this_step,
            &mut self.created_this_step,
            &mut self.eroded_this_step,
            &mut self.delaminated_this_step,
            &mut self.foundered_this_step,
            &mut self.continentalised_by_accretion,
            &mut self.continentalised_by_spreading,
            &mut self.continentalised_by_infill,
            &mut self.plume_created_this_step,
            &mut self.dripped_this_step,
            &mut self.continentalised_by_plume,
            &mut self.continentalised_by_piling,
            &mut self.continental_recycled,
            &mut self.decontinentalised,
            &mut self.interior_holes,
            &mut self.interior_holes_unfilled,
            &mut self.torn_continent,
            &mut self.rejoined,
        ]
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
    out.overrun.fill(0);
    let areas = &mesh.areas;
    let area = |i: usize| areas[i] as f64;
    let off = &mesh.nbr_off;
    let nbr = &mesh.nbr;
    let chord = &mesh.chord_dir;
    let range = |i: usize| off[i] as usize..off[i + 1] as usize;

    out.winner.fill(-1);
    out.accreted.fill(0.0);
    out.welded.fill(0.0);
    out.welded_continental.fill(0.0);
    out.welded_age.fill(0.0);
    out.welded_keel.fill(0.0);
    out.welded_share.fill(0.0);
    out.rift_cell.fill(0);
    out.subducted.fill(0);
    out.spread_in.fill(0.0);
    out.margin_eroded.fill(0.0);
    let mut subducted_this_step = 0.0f64;
    let mut eroded_this_step = 0.0f64;
    let mut delaminated_this_step = 0.0f64;
    let mut foundered_this_step = 0.0f64;
    let width = (4.0 * std::f64::consts::PI / n as f64).sqrt();

    // --- Quantised rigid motion ----------------------------------------------
    // A plate steps when its fastest-moving crust has gone a cell.
    s.pack_plates(plates);
    s.pack_surface_speeds(mesh, &src.plate_id);
    s.plate_flag.fill(0);
    for (&id, props) in plates.by_id.iter_mut() {
        props.pending =
            (props.pending + s.plate_speed[id as usize] * dt_myr).min(2.0 * width);
        if props.pending >= width {
            props.pending -= width;
            props.steps = props.steps.wrapping_add(1);
            s.plate_flag[id as usize] = if props.steps % 2 == 0 { 2 } else { 1 };
        }
    }
    let stepping_flags = std::mem::take(&mut s.plate_flag);
    let flag = |p: i32| {
        if p >= 0 && (p as usize) < stepping_flags.len() {
            stepping_flags[p as usize]
        } else {
            0
        }
    };
    let is_stepping = |p: i32| flag(p) != 0;
    let hands_over_ridge = |p: i32| flag(p) == 2;
    let packed = PackedPlates::of(&s.plate_omega, &s.plate_live);

    // --- Where this step carries each cell's crust ---------------------------
    let c = &mesh.centres;
    par::fill_pair(&mut s.forward, &mut s.forward_arc, |cell| {
        let plate = src.plate_id[cell];
        if !is_stepping(plate) || !packed.is_live(plate) {
            return (-1, -1);
        }
        let w = packed.omega(plate);
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
            return (-1, -1);
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
        (best, best_arc)
    });

    // Does `a`'s crust ride over `b`'s when the two meet? Between ocean floors
    // the younger, more buoyant one does. Between continents the craton does:
    // a deep keel is the rigid indenter, and the younger, thinner margin
    // crumples against it, as India's crust does against Asia while the old
    // Indian shield rides through intact.
    let overrides = |a: usize, b: usize| -> bool {
        let a_cont = src.crust_type[a] == CONTINENTAL;
        let b_cont = src.crust_type[b] == CONTINENTAL;
        if a_cont != b_cont {
            return a_cont;
        }
        if a_cont {
            if src.keel[a] != src.keel[b] {
                return src.keel[a] > src.keel[b];
            }
            if src.thickness[a] != src.thickness[b] {
                return src.thickness[a] > src.thickness[b];
            }
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

    // Lower rides over: continent over any ocean, and cratonic margins over
    // younger continent, as in `overrides`.
    let buoyancy = |cell: usize| -> f64 {
        if src.crust_type[cell] == CONTINENTAL {
            -t.max_oceanic_age * (1.0 + src.keel[cell] as f64)
        } else {
            src.age[cell] as f64
        }
    };

    // Crust heading across a convergent margin into another plate's cell, in
    // cell order.
    let contacts: Vec<(usize, usize)> = par::filter_map(n, |cell| {
        let plate = src.plate_id[cell];
        let d = s.forward[cell];
        if plate < 0 || d < 0 {
            return None;
        }
        let d = d as usize;
        let held = src.plate_id[d];
        if held < 0 || held == plate {
            return None;
        }
        let arc = s.forward_arc[cell];
        if arc < 0 || boundaries.arc_kind[arc as usize] != CONVERGENT {
            return None;
        }
        Some((cell, d))
    });
    let mut margins: HashMap<i64, (f64, f64)> = HashMap::new();
    for &(cell, d) in &contacts {
        let key = src.plate_id[cell] as i64 * 65536 + src.plate_id[d] as i64;
        let tally = margins.entry(key).or_insert((0.0, 0.0));
        tally.0 += buoyancy(cell);
        tally.1 += buoyancy(d);
    }

    for &(cell, d) in &contacts {
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
        // Nothing overruns a craton but a deeper-rooted one.
        if src.crust_type[d] == CONTINENTAL
            && src.keel[d] >= CRATON_KEEL
            && src.keel[cell] <= src.keel[d]
        {
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
    // At a convergent margin the plate that goes down goes down along its
    // whole length: its ocean is consumed at the trench whatever crust it
    // meets there, rather than holding up the plate behind it. The front locks
    // only where the plate rides over but could not take the cell ahead, or
    // where continent meets a plate it cannot go under.
    let locked = par::filter_map(n, |cell| {
        let plate = src.plate_id[cell];
        let ahead = s.forward[cell];
        if plate < 0 || ahead < 0 {
            return None;
        }
        let ahead = ahead as usize;
        if src.plate_id[ahead] == plate || s.new_plate[ahead] == plate {
            return None;
        }
        let held = src.plate_id[ahead];
        let arc = s.forward_arc[cell];
        let closing = arc >= 0 && boundaries.arc_kind[arc as usize] == CONVERGENT;
        let locked = if !closing {
            overrides(cell, ahead)
        } else if src.crust_type[cell] == CONTINENTAL {
            true
        } else {
            let rides_over = margins
                .get(&(plate as i64 * 65536 + held as i64))
                .is_some_and(|&(mine, theirs)| mine < theirs);
            rides_over && overrides(cell, ahead)
        };
        locked.then_some(cell)
    });
    s.stalled.fill(0);
    let mut jam_len = 0usize;
    for cell in locked {
        s.stalled[cell] = 1;
        s.queue[jam_len] = cell as i32;
        jam_len += 1;
    }
    // Crust heading into a stalled cell is held up behind it. What heads into
    // a cell is always beside it, so the jam spreads neighbour by neighbour.
    let mut head = 0;
    while head < jam_len {
        let tc = s.queue[head] as usize;
        head += 1;
        for k in range(tc) {
            let b = nbr[k] as usize;
            let heading_in = s.forward[b] == tc as i32 && src.plate_id[b] == src.plate_id[tc];
            if !heading_in || s.stalled[b] != 0 {
                continue;
            }
            s.stalled[b] = 1;
            s.queue[jam_len] = b as i32;
            jam_len += 1;
        }
    }

    // --- Pass 2: transport, as an exact permutation --------------------------
    // Each destination writes only its own slots, and the keys of those with
    // candidates come back in cell order.
    let keys = {
        let source_of = Disjoint::new(&mut s.source_of);
        let claimed = Disjoint::new(&mut s.claimed);
        let cand = Disjoint::new(&mut s.cand);
        let cand_count = Disjoint::new(&mut s.cand_count);
        let (forward, new_plate, stalled) = (&s.forward, &s.new_plate, &s.stalled);
        par::filter_map(n, |d| {
            // Safety: iteration `d` alone touches index `d` of `source_of`,
            // `claimed` and `cand_count`, and `cand`'s slots for `d`.
            unsafe {
                source_of.set(d, -1);
                claimed.set(d, 0);
            }
            let plate = new_plate[d];
            if plate < 0 {
                return None;
            }
            let keeps_own = src.plate_id[d] == plate;
            if !is_stepping(plate) || (stalled[d] != 0 && keeps_own) {
                if keeps_own {
                    unsafe {
                        source_of.set(d, d as i32);
                        claimed.set(d, 1);
                    }
                }
                return None;
            }
            if !packed.is_live(plate) {
                return None;
            }
            let w = packed.omega(plate);
            let (px, py, pz) = (c[d * 3] as f64, c[d * 3 + 1] as f64, c[d * 3 + 2] as f64);
            let vx = w[1] * pz - w[2] * py;
            let vy = w[2] * px - w[0] * pz;
            let vz = w[0] * py - w[1] * px;
            let speed = (vx * vx + vy * vy + vz * vz).sqrt();
            let inv = if speed == 0.0 { 0.0 } else { -1.0 / speed };
            let mut cands = [0i32; MAX_NBRS];
            let mut scores = [0.0f32; MAX_NBRS];
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
                    + if forward[src_cell as usize] == d as i32 {
                        2.0
                    } else {
                        0.0
                    };
                let mut i = count;
                while i > 0 && (scores[i - 1] as f64) < score {
                    cands[i] = cands[i - 1];
                    scores[i] = scores[i - 1];
                    i -= 1;
                }
                cands[i] = src_cell;
                scores[i] = score as f32;
                count += 1;
            }
            unsafe {
                cand_count.set(d, count as i32);
                for (i, &cell) in cands[..count].iter().enumerate() {
                    cand.set(d * MAX_NBRS + i, cell);
                }
            }
            (count > 0).then(|| descending_key(scores[0], d as u32))
        })
    };
    s.order.clear();
    s.order.extend_from_slice(&keys);
    // Best-aligned pairs first; ties by cell index, as a stable sort would.
    par::sort_unstable(&mut s.order);
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

    // --- Pass 2c: a gap torn inside a plate is handed back to its margin -----
    // Moving a plate a whole cell over an irregular mesh cannot map cells one
    // to one: the matching leaves holes at fixed spots of the mesh (the
    // pentagons and their kin), its unplaced crust turning up at the margins.
    // A hole with no other plate beside it is no rift. Moved rigidly, a plate
    // opens its gaps along its trailing edge, so the crust just behind the hole
    // steps into it instead, leaving the hole behind it, until the hole reaches
    // the margin or meets crust that had nowhere to go. Every cell still moves
    // one cell, and nothing is thinned.
    let mut target_of = std::mem::take(&mut s.i32a);
    target_of.fill(-1);
    for d in 0..n {
        let source = s.source_of[d];
        if source >= 0 {
            target_of[source as usize] = d as i32;
        }
    }
    let at_margin =
        |new_plate: &[i32], d: usize| range(d).any(|k| new_plate[nbr[k] as usize] != new_plate[d]);
    let mut interior: Vec<usize> = Vec::new();
    let mut chain: Vec<usize> = Vec::new();
    let mut torn = 0.0f64;
    for start in 0..n {
        let plate = s.new_plate[start];
        if plate < 0 || s.source_of[start] >= 0 || s.claimed[start] == 0 {
            continue;
        }
        if !is_stepping(plate) || at_margin(&s.new_plate, start) {
            continue;
        }
        torn += area(start);
        let w = packed.omega(plate);
        let mut hole = Some(start);
        chain.clear();
        for _ in 0..HOLE_CHAIN_LIMIT {
            let Some(h) = hole else { break };
            if at_margin(&s.new_plate, h) {
                break;
            }
            let (px, py, pz) = (c[h * 3] as f64, c[h * 3 + 1] as f64, c[h * 3 + 2] as f64);
            let v = [
                w[1] * pz - w[2] * py,
                w[2] * px - w[0] * pz,
                w[0] * py - w[1] * px,
            ];
            let mut behind: Option<usize> = None;
            let mut most = 0.0f64;
            for k in range(h) {
                let m = nbr[k] as usize;
                if src.plate_id[m] != plate
                    || s.new_plate[m] != plate
                    || s.stalled[m] != 0
                    || s.eaten[m] != 0
                    || s.u8a[m] != 0
                {
                    continue;
                }
                let along = chord[k * 3] as f64 * v[0]
                    + chord[k * 3 + 1] as f64 * v[1]
                    + chord[k * 3 + 2] as f64 * v[2];
                if along < most {
                    most = along;
                    behind = Some(m);
                }
            }
            let Some(m) = behind else { break };
            s.u8a[m] = 1;
            chain.push(m);
            s.source_of[h] = m as i32;
            let was = target_of[m];
            target_of[m] = h as i32;
            if s.claimed[m] == 0 || was < 0 {
                s.claimed[m] = 1;
                // Crust that was to stay put leaves a gap where it stood,
                // unless something else was already moving in.
                hole = (s.source_of[m] < 0).then_some(m);
            } else {
                let was = was as usize;
                s.source_of[was] = -1;
                // A cell whose own crust never left keeps it: no gap there.
                hole = (s.claimed[was] != 0).then_some(was);
            }
        }
        for &m in &chain {
            s.u8a[m] = 0;
        }
        if let Some(h) = hole
            && !at_margin(&s.new_plate, h)
        {
            interior.push(h);
        }
    }
    s.i32a = target_of;
    out.interior_holes = torn;
    out.interior_holes_unfilled = 0.0;

    // --- Pass 2d: a ridge parts its two plates evenly ------------------------
    // Floor a plate opens along its trailing margin goes by turns to it and to
    // the plate it is leaving, so the ridge follows at half the plate's speed
    // rather than staying where the margin was, and each side grows by half
    // the spreading.
    let mut owner = std::mem::take(&mut s.i32b);
    par::copy(&mut owner, &s.new_plate);
    let (source_of, claimed) = (&s.source_of, &s.claimed);
    par::for_each_mut(&mut s.new_plate, |d, new_plate| {
        let plate = owner[d];
        if plate < 0 || source_of[d] >= 0 || claimed[d] == 0 || !hands_over_ridge(plate) {
            return;
        }
        let (mut beyond, mut most) = (-1, 0);
        for k in range(d) {
            let q = owner[nbr[k] as usize];
            if q < 0 || q == plate || !packed.is_live(q) {
                continue;
            }
            let count = range(d).filter(|&j| owner[nbr[j] as usize] == q).count();
            if count > most {
                most = count;
                beyond = q;
            }
        }
        if beyond < 0 {
            return;
        }
        let closing = range(d).any(|k| {
            let arc = mesh.nbr_arc[k];
            owner[nbr[k] as usize] == beyond
                && arc >= 0
                && boundaries.arc_kind[arc as usize] == CONVERGENT
        });
        if !closing {
            *new_plate = beyond;
        }
    });
    s.i32b = owner;

    // Where each destination's crust comes from: a source cell, itself, or
    // `RIFT` for an empty placeholder left for the crust lifecycle.
    const RIFT: i32 = -1;
    let mut from = std::mem::take(&mut s.i32a);
    {
        let from = Disjoint::new(&mut from);
        let claimed = Disjoint::new(&mut s.claimed);
        let winner = Disjoint::new(&mut out.winner);
        let rift_cell = Disjoint::new(&mut out.rift_cell);
        let (new_plate, source_of) = (&s.new_plate, &s.source_of);
        par::for_each(n, |d| {
            let source = source_of[d];
            // Safety: iteration `d` alone touches index `d` of each.
            unsafe {
                if new_plate[d] < 0 {
                    from.set(d, d as i32);
                } else if source >= 0 {
                    from.set(d, source);
                    winner.set(d, source);
                } else if claimed.get(d) != 0 {
                    from.set(d, RIFT);
                    rift_cell.set(d, 1);
                } else {
                    from.set(d, d as i32);
                    claimed.set(d, 1);
                    winner.set(d, d as i32);
                }
            }
        });
    }
    // Every field is gathered from where its crust came from. Mantle heat
    // belongs to the mantle, not the crust, so it is never moved.
    let gather = |out: &mut [f32], field: &[f32], rift: f32| {
        par::fill(out, |d| match from[d] {
            RIFT => rift,
            f => field[f as usize],
        });
    };
    gather(&mut dst.elevation, &src.elevation, 0.0);
    gather(&mut dst.age, &src.age, 0.0);
    gather(&mut dst.density, &src.density, t.rift_density as f32);
    gather(&mut dst.weakness, &src.weakness, 0.0);
    gather(&mut dst.slab, &src.slab, 0.0);
    gather(&mut dst.keel, &src.keel, 0.0);
    gather(&mut dst.plume_share, &src.plume_share, 0.0);
    par::fill(&mut dst.crust_type, |d| match from[d] {
        RIFT => OCEANIC,
        f => src.crust_type[f as usize],
    });
    par::fill(&mut dst.rift_side, |d| match from[d] {
        RIFT => 0,
        f => src.rift_side[f as usize],
    });
    par::fill(&mut dst.thickness, |d| match from[d] {
        RIFT => 0.0,
        f if f as usize == d => src.thickness[d],
        f => {
            let f = f as usize;
            (src.thickness[f] as f64 * area(f) / area(d)) as f32
        }
    });
    par::fill(&mut dst.plate_id, |d| match s.new_plate[d] {
        p if p < 0 => src.plate_id[d],
        p => p,
    });
    par::copy(&mut dst.mantle_heat, &src.mantle_heat);
    s.i32a = from;

    // --- Pass 3: dispose of the crust that had nowhere to go -----------------
    out.torn_continent = 0.0;
    for cell in 0..n {
        if s.claimed[cell] != 0 || src.plate_id[cell] < 0 {
            continue;
        }
        let ahead = s.forward[cell];
        let blocked_by_other = ahead >= 0 && src.plate_id[ahead as usize] != src.plate_id[cell];
        let mass = area(cell) * src.thickness[cell] as f64;
        if src.crust_type[cell] == CONTINENTAL {
            let kept = if s.eaten[cell] != 0 {
                out.overrun[cell] = 1;
                // Scraped off by the plate that took the cell: nearly all of it
                // welds on, the rest is the trench's erosion of the margin.
                let eroded = t.subduction_erosion * t.craton_wear(src.keel[cell]) * mass;
                subducted_this_step += eroded;
                eroded_this_step += eroded;
                out.subducted[cell] = 1;
                if out.rift_cell[cell] == 0 {
                    dst.crust_type[cell] = CONTINENTAL;
                    dst.density[cell] = t.continental_density as f32;
                }
                (mass - eroded) / area(cell)
            } else {
                out.overrun[cell] = 2;
                out.torn_continent += area(cell);
                mass / area(cell)
            };
            add_f32(&mut out.welded[cell], kept);
            add_f32(&mut out.welded_share[cell], kept * src.plume_share[cell] as f64);
            add_f32(&mut out.welded_continental[cell], kept);
            add_f32(&mut out.welded_age[cell], kept * src.age[cell] as f64);
            add_f32(&mut out.welded_keel[cell], kept * src.keel[cell] as f64);
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
            add_f32(
                &mut out.welded_share[cell],
                mass / area(cell) * src.plume_share[cell] as f64,
            );
        }
    }

    // --- Pass 4: apply this step's gains, handing on what will not fit -------
    let max_gain = t.max_thickening_per_myr * dt_myr;
    let mut surplus = std::mem::take(&mut s.f32a);
    let mut next = std::mem::take(&mut s.f32b);
    let mut incoming = std::mem::take(&mut s.f32d);
    surplus.fill(0.0);
    // Cells holding surplus, in order, and the cells it went to this round.
    let mut active: Vec<usize> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    // `margin_eroded` is reused after the strip as remaining accretion this
    // cell may still absorb at the thickening cap. Each cell sees only its
    // own slots; what it strips and whether it has surplus come back in cell
    // order.
    let gains = {
        let thickness = Disjoint::new(&mut dst.thickness);
        let plume_share = Disjoint::new(&mut dst.plume_share);
        let margin_eroded = Disjoint::new(&mut out.margin_eroded);
        let surplus = Disjoint::new(&mut surplus);
        let (keel, rift_cell, accreted) = (&dst.keel, &out.rift_cell, &out.accreted);
        par::chunks(n, |cells| {
            let (mut stripped, mut held) = (Vec::new(), Vec::new());
            // Safety: iteration `d` alone touches index `d` of each.
            for d in cells {
                unsafe {
                    // A cratonic keel is too deep and buoyant for the trench
                    // to scrape.
                    let eroded_h = margin_eroded.get(d) as f64
                        * (1.0 - keel[d] as f64)
                        * t.craton_wear(keel[d]);
                    if eroded_h > 0.0 && rift_cell[d] == 0 {
                        let h = thickness.get(d) as f64;
                        let available = (h - t.rift_thickness_floor).max(0.0);
                        let take = eroded_h.min(available);
                        thickness.set(d, (h - take) as f32);
                        stripped.push(take * area(d));
                    }
                    let room = if rift_cell[d] != 0 { 0.0 } else { max_gain as f32 };
                    margin_eroded.set(d, room);
                    let acc = accreted[d] as f64;
                    if acc <= 0.0 {
                        continue;
                    }
                    let h = thickness.get(d) as f64;
                    let cap_room = if rift_cell[d] != 0 {
                        0.0
                    } else {
                        (thickness_cap - h).max(0.0)
                    };
                    let take = acc.min(room as f64).min(cap_room);
                    plume_share.set(d, mixed_share(plume_share.get(d), h, take, 0.0));
                    thickness.set(d, (h + take) as f32);
                    margin_eroded.set(d, (room as f64 - take) as f32);
                    let left = (acc - take) as f32;
                    surplus.set(d, left);
                    if left > 0.0 {
                        held.push(d);
                    }
                }
            }
            (stripped, held)
        })
    };
    for (stripped, held) in gains {
        for mass in stripped {
            eroded_this_step += mass;
            subducted_this_step += mass;
        }
        active.extend(held);
    }

    // Surplus only ever sits at a few cells along margins, so each round
    // visits those, in order, and the cells they hand it to, rather than the
    // whole globe. `incoming` and `next` are kept clear between rounds.
    let accreting_room = |dst: &CellData, rift_cell: &[u8], eroded: &[f32], m: usize| -> f64 {
        if rift_cell[m] != 0 {
            0.0
        } else {
            (thickness_cap - dst.thickness[m] as f64)
                .max(0.0)
                .min(eroded[m] as f64) as f32 as f64
        }
    };
    incoming.fill(0.0);
    next.fill(0.0);
    for _round in 0..PLACEMENT_ROUNDS {
        if active.is_empty() {
            break;
        }
        touched.clear();
        for &d in &active {
            let have = surplus[d] as f64;
            let own = dst.plate_id[d];
            let accepts = |m: usize| dst.plate_id[m] == own && out.rift_cell[m] == 0;
            let mut room = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    room += accreting_room(dst, &out.rift_cell, &out.margin_eroded, m);
                }
            }
            let crowded = room <= 0.0;
            let weight_of = |m: usize| {
                if crowded {
                    1.0 / (1.0 + dst.thickness[m] as f64)
                } else {
                    accreting_room(dst, &out.rift_cell, &out.margin_eroded, m)
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
                if s.u8a[m] == 0 {
                    s.u8a[m] = 1;
                    touched.push(m);
                }
            }
        }
        for &d in &touched {
            s.u8a[d] = 0;
            let inc = incoming[d] as f64;
            incoming[d] = 0.0;
            if inc == 0.0 {
                continue;
            }
            let place = inc.min(accreting_room(dst, &out.rift_cell, &out.margin_eroded, d));
            dst.plume_share[d] =
                mixed_share(dst.plume_share[d], dst.thickness[d] as f64, place, 0.0);
            add_f32(&mut dst.thickness[d], place);
            add_f32(&mut out.margin_eroded[d], -place);
            add_f32(&mut next[d], inc - place);
        }
        std::mem::swap(&mut surplus, &mut next);
        for &d in &active {
            next[d] = 0.0;
        }
        active.extend_from_slice(&touched);
        active.retain(|&d| surplus[d] > 0.0);
        active.sort_unstable();
        active.dedup();
    }
    for &d in &active {
        foundered_this_step += surplus[d] as f64 * area(d);
        subducted_this_step += surplus[d] as f64 * area(d);
        surplus[d] = 0.0;
    }
    active.clear();

    out.spill_share.fill(0.0);
    let spilling = {
        let age = Disjoint::new(&mut dst.age);
        let keel = Disjoint::new(&mut dst.keel);
        let plume_share = Disjoint::new(&mut dst.plume_share);
        let thickness = Disjoint::new(&mut dst.thickness);
        let surplus = Disjoint::new(&mut surplus);
        let spill_share = Disjoint::new(&mut out.spill_share);
        let crust_type = &dst.crust_type;
        par::filter_map(n, |d| {
            let welded = out.welded[d] as f64;
            if welded == 0.0 {
                return None;
            }
            // Safety: iteration `d` alone touches index `d` of each.
            unsafe {
                let welded_tracer = (out.welded_share[d] as f64 / welded).clamp(0.0, 1.0);
                let h = thickness.get(d);
                let held = if out.rift_cell[d] != 0 {
                    0.0
                } else {
                    welded.min((thickness_cap - h as f64).max(0.0))
                };
                // Continental crust welded on brings its age and keel with
                // it, mixed by thickness with the crust already in the cell,
                // so a craton that loses its cell to the grid is not wiped
                // from the record. A craton either side of the weld keeps its
                // keel whole: the younger crust crumples against it or is
                // underthrust beneath, and does not dilute its root.
                let continental = (out.welded_continental[d] as f64).min(welded);
                let held_continental = held * continental / welded;
                let stays_continental = crust_type[d] == CONTINENTAL
                    || h as f64 + held >= t.continental_thickness_threshold;
                if held_continental > 0.0 && stays_continental {
                    let own = h.max(0.0) as f64 + (held - held_continental);
                    let total = own + held_continental;
                    let welded_age = out.welded_age[d] as f64 / continental;
                    let welded_keel = out.welded_keel[d] as f64 / continental;
                    let mixed_age =
                        (own * age.get(d) as f64 + held_continental * welded_age) / total;
                    age.set(d, mixed_age as f32);
                    let k = keel.get(d);
                    let mixed = ((own * k as f64 + held_continental * welded_keel) / total) as f32;
                    let deepest = k.max(welded_keel as f32);
                    keel.set(d, if deepest >= CRATON_KEEL { deepest } else { mixed });
                }
                plume_share.set(d, mixed_share(plume_share.get(d), h as f64, held, welded_tracer));
                thickness.set(d, (h as f64 + held) as f32);
                let left = (welded - held) as f32;
                surplus.set(d, left);
                spill_share.set(d, ((welded - held) * welded_tracer) as f32);
                (left > 0.0).then_some(d)
            }
        })
    };
    active.extend(spilling);

    // As above; the plume tracer travels with the crust in `spill_share`.
    let welding_room = |dst: &CellData, rift_cell: &[u8], m: usize| -> f64 {
        if rift_cell[m] != 0 {
            0.0
        } else {
            (thickness_cap - dst.thickness[m] as f64).max(0.0) as f32 as f64
        }
    };
    out.spill_share_in.fill(0.0);
    out.spill_share_next.fill(0.0);
    for round in 0..PLACEMENT_ROUNDS {
        if active.is_empty() {
            break;
        }
        touched.clear();
        for &d in &active {
            let have = surplus[d] as f64;
            let tracer = (out.spill_share[d] as f64 / have).clamp(0.0, 1.0);
            let own = dst.plate_id[d];
            let accepts = |m: usize| dst.plate_id[m] == own && out.rift_cell[m] == 0;
            let mut room = 0.0f64;
            for k in range(d) {
                let m = nbr[k] as usize;
                if accepts(m) {
                    room += welding_room(dst, &out.rift_cell, m);
                }
            }
            let crowded = room <= 0.0;
            let weight_of = |m: usize| {
                if crowded {
                    1.0 / (1.0 + dst.thickness[m] as f64)
                } else {
                    welding_room(dst, &out.rift_cell, m)
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
                add_f32(&mut out.spill_share_next[d], have * tracer);
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
                let moved = have * (w / weight) * area(d) / area(m);
                add_f32(&mut incoming[m], moved);
                add_f32(&mut out.spill_share_in[m], moved * tracer);
                if s.u8a[m] == 0 {
                    s.u8a[m] = 1;
                    touched.push(m);
                }
            }
        }
        let last = round == PLACEMENT_ROUNDS - 1;
        for &d in &touched {
            s.u8a[d] = 0;
            let inc = incoming[d] as f64;
            let carried = out.spill_share_in[d] as f64;
            incoming[d] = 0.0;
            out.spill_share_in[d] = 0.0;
            if inc == 0.0 {
                continue;
            }
            let tracer = (carried / inc).clamp(0.0, 1.0);
            let place = if last {
                inc
            } else {
                inc.min(welding_room(dst, &out.rift_cell, d))
            };
            dst.plume_share[d] =
                mixed_share(dst.plume_share[d], dst.thickness[d] as f64, place, tracer);
            add_f32(&mut dst.thickness[d], place);
            add_f32(&mut next[d], inc - place);
            add_f32(&mut out.spill_share_next[d], (inc - place) * tracer);
        }
        std::mem::swap(&mut surplus, &mut next);
        std::mem::swap(&mut out.spill_share, &mut out.spill_share_next);
        for &d in &active {
            next[d] = 0.0;
            out.spill_share_next[d] = 0.0;
        }
        active.extend_from_slice(&touched);
        active.retain(|&d| surplus[d] > 0.0);
        active.sort_unstable();
        active.dedup();
    }
    for &d in &active {
        let tracer = (out.spill_share[d] / surplus[d]).clamp(0.0, 1.0) as f64;
        dst.plume_share[d] =
            mixed_share(dst.plume_share[d], dst.thickness[d] as f64, surplus[d] as f64, tracer);
        add_f32(&mut dst.thickness[d], surplus[d] as f64);
    }

    // --- Pass 4a: thickened crust spreads under its own weight ---------------
    let spread_rate = (t.crust_spread_rate * dt_myr).clamp(0.0, 0.5);
    if spread_rate > 0.0 {
        let before = &mut surplus;
        let flow = &mut next;
        before.copy_from_slice(&dst.thickness);
        flow.fill(0.0);
        // Crust each cell takes in, and that times the share it carries.
        out.spill_share_next.fill(0.0);
        out.spill_share_in.fill(0.0);
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
                let moved = give * share * area(d) / area(m);
                add_f32(&mut flow[m], moved);
                add_f32(&mut out.spill_share_next[m], moved);
                add_f32(&mut out.spill_share_in[m], moved * dst.plume_share[d] as f64);
            }
        }
        for d in 0..n {
            let inflow = out.spill_share_next[d] as f64;
            if inflow > 0.0 {
                // What it kept of its own, then what flowed in.
                let kept = before[d] as f64 - (inflow - flow[d] as f64);
                let tracer = out.spill_share_in[d] as f64 / inflow;
                dst.plume_share[d] = mixed_share(dst.plume_share[d], kept, inflow, tracer);
            }
            add_f32(&mut dst.thickness[d], flow[d] as f64);
            out.spread_in[d] = flow[d].max(0.0);
        }
    }

    // --- Pass 4d: an over-thick root founders, slowly ------------------------
    let shed_per_step = (t.delamination_rate * dt_myr).max(0.0);
    if shed_per_step > 0.0 {
        let keel = &dst.keel;
        let thickness = Disjoint::new(&mut dst.thickness);
        let shed = par::filter_map(n, |d| {
            // Safety: iteration `d` alone touches index `d`.
            unsafe {
                let h = thickness.get(d) as f64;
                let over = h - thickness_cap;
                if over <= 0.0 {
                    return None;
                }
                let shed = over.min(shed_per_step * t.craton_wear(keel[d]));
                thickness.set(d, (h - shed) as f32);
                Some(shed * area(d))
            }
        });
        for mass in shed {
            subducted_this_step += mass;
            delaminated_this_step += mass;
        }
    }

    // --- Pass 4e: a gap that could not be handed back closes up -------------
    // The hole takes on the crust most of its rim is made of, drawing its
    // thickness from that crust a couple of rings around so no one cell is
    // thinned much, and nothing is created.
    let mut ring: Vec<usize> = Vec::new();
    let mut next_ring: Vec<usize> = Vec::new();
    let mut donors: Vec<usize> = Vec::new();
    let mut spare: Vec<f64> = Vec::new();
    for &d in &interior {
        if out.rift_cell[d] == 0 {
            continue;
        }
        let plate = dst.plate_id[d];
        let rim = |m: usize| dst.plate_id[m] == plate && out.rift_cell[m] == 0;
        let (mut continental, mut oceanic) = (0.0f64, 0.0f64);
        for k in range(d) {
            let m = nbr[k] as usize;
            if !rim(m) {
                continue;
            }
            if dst.crust_type[m] == CONTINENTAL {
                continental += area(m);
            } else {
                oceanic += area(m);
            }
        }
        if continental + oceanic <= 0.0 {
            out.interior_holes_unfilled += area(d);
            continue;
        }
        let kind = if continental >= oceanic {
            CONTINENTAL
        } else {
            OCEANIC
        };
        let like = |m: usize| rim(m) && dst.crust_type[m] == kind;

        let (mut model, mut rim_mass, mut rim_area) = (d, 0.0f64, 0.0f64);
        for k in range(d) {
            let m = nbr[k] as usize;
            if !like(m) {
                continue;
            }
            rim_mass += dst.thickness[m] as f64 * area(m);
            rim_area += area(m);
            if model == d || dst.thickness[m] > dst.thickness[model] {
                model = m;
            }
        }

        donors.clear();
        ring.clear();
        ring.push(d);
        s.u8a[d] = 1;
        for _ in 0..HOLE_DONOR_RINGS {
            next_ring.clear();
            for &cell in &ring {
                for k in range(cell) {
                    let m = nbr[k] as usize;
                    if s.u8a[m] != 0 || !like(m) {
                        continue;
                    }
                    s.u8a[m] = 1;
                    donors.push(m);
                    next_ring.push(m);
                }
            }
            std::mem::swap(&mut ring, &mut next_ring);
        }
        s.u8a[d] = 0;
        for &m in &donors {
            s.u8a[m] = 0;
        }

        // Closing a gap must not thin the continent around it into ocean.
        let floor = if kind == CONTINENTAL {
            t.continental_thickness_threshold
        } else {
            t.rift_thickness_floor
        };
        spare.clear();
        spare.extend(
            donors
                .iter()
                .map(|&m| (dst.thickness[m] as f64 - floor).max(0.0) * area(m)),
        );
        let available: f64 = spare.iter().sum();
        if available <= 0.0 {
            out.interior_holes_unfilled += area(d);
            continue;
        }
        let wanted = available.min(rim_mass / rim_area * area(d));
        let (mut pulled, mut traced) = (0.0f64, 0.0f64);
        for (&m, &own) in donors.iter().zip(&spare) {
            let take = wanted * own / available;
            add_f32(&mut dst.thickness[m], -take / area(m));
            pulled += take;
            traced += take * dst.plume_share[m] as f64;
        }
        dst.plume_share[d] = (traced / pulled.max(1e-12)) as f32;

        dst.elevation[d] = dst.elevation[model];
        dst.crust_type[d] = kind;
        dst.age[d] = dst.age[model];
        dst.density[d] = dst.density[model];
        dst.weakness[d] = dst.weakness[model];
        dst.slab[d] = dst.slab[model];
        dst.rift_side[d] = dst.rift_side[model];
        dst.keel[d] = dst.keel[model];
        dst.thickness[d] = (pulled / area(d)) as f32;
        out.rift_cell[d] = 0;
    }

    // --- Pass 4c: a continent rifts by stretching, not by melting ------------
    // Stretching may not thin the flanks past the revert thickness (with a
    // hair to spare for single-precision rounding), or the rift would punch
    // ocean holes into the continent beside it.
    let flank_floor = t
        .rift_thickness_floor
        .max(t.continental_revert_thickness + 1e-3);
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
                available += (dst.thickness[m] as f64 - flank_floor).max(0.0) * area(m);
            }
        }
        if available <= 0.0 {
            continue;
        }
        let wanted = available.min(t.oceanic_thickness * area(d));
        let (mut pulled, mut traced) = (0.0f64, 0.0f64);
        for k in range(d) {
            let m = nbr[k] as usize;
            if !(dst.plate_id[m] == plate && dst.crust_type[m] == CONTINENTAL) {
                continue;
            }
            let spare = (dst.thickness[m] as f64 - flank_floor).max(0.0) * area(m);
            if spare <= 0.0 {
                continue;
            }
            let take = wanted * spare / available;
            add_f32(&mut dst.thickness[m], -take / area(m));
            pulled += take;
            traced += take * dst.plume_share[m] as f64;
        }
        dst.plume_share[d] = (traced / pulled.max(1e-12)) as f32;
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
    // A cell all but surrounded by another plate joins it, in two rounds that
    // each read the plates as the round before left them. Only a cell beside
    // one that joined in the first round can join in the second.
    let joins = |plate_id: &[i32], d: usize| -> Option<i32> {
        let own = plate_id[d];
        if own < 0 {
            return None;
        }
        let mut best_plate = -1;
        let mut best_count = 0;
        for k in range(d) {
            let p = plate_id[nbr[k] as usize];
            if p < 0 || p == own {
                continue;
            }
            let count = range(d).filter(|&j| plate_id[nbr[j] as usize] == p).count();
            if count > best_count {
                best_count = count;
                best_plate = p;
            }
        }
        (best_count >= 4).then_some(best_plate)
    };
    let first = par::filter_map(n, |d| joins(&dst.plate_id, d).map(|p| (d, p)));
    for &(d, p) in &first {
        dst.plate_id[d] = p;
    }
    let mut beside: Vec<usize> = Vec::new();
    for &(d, _) in &first {
        for cell in std::iter::once(d).chain(range(d).map(|k| nbr[k] as usize)) {
            if s.u8a[cell] == 0 {
                s.u8a[cell] = 1;
                beside.push(cell);
            }
        }
    }
    let second: Vec<(usize, i32)> = beside
        .iter()
        .filter_map(|&d| joins(&dst.plate_id, d).map(|p| (d, p)))
        .collect();
    for &d in &beside {
        s.u8a[d] = 0;
    }
    for &(d, p) in &second {
        dst.plate_id[d] = p;
    }

    s.f32a = surplus;
    s.f32b = next;
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
