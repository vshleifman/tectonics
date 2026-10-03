//! Slabs: the subducted crust hanging beneath each trench. Slab pull comes
//! from a slab that has actually gone down, so it builds up while a margin
//! converges, sinks away over `slab_decay_time` once it stops, and remembers
//! which side is overriding. Without this memory the down-going side would be
//! re-picked every tick from whichever cell happens to be denser, and two
//! plates of similar age would trade the pull back and forth.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::clamp01;
use crate::mesh::Mesh;
use crate::plates::suture_key;
use crate::scratch::Scratch;
use crate::tuning::Tuning;
use std::collections::HashMap;

/// Share of a full slab below which a trench counts as having none.
const SLAB_TRACE: f64 = 0.01;

/// Which side of a margin goes down, and how much slab there is to pull it.
pub struct Subduction {
    pub over: usize,
    pub down: usize,
    /// Slab present as a share of a full one, in [0, 1].
    pub strength: f64,
}

/// Pull per unit length of trench from crust of the given age, in [0, 1].
pub fn slab_pull(t: &Tuning, age: f64) -> f64 {
    let onset = t.slab_pull_onset_age;
    let span = (t.slab_pull_saturation_age - onset).max(1e-6);
    clamp01((age - onset) / span)
}

/// The two cells either side of arc `a`, if they belong to different plates.
fn margin(mesh: &Mesh, data: &CellData, a: usize) -> Option<(usize, usize)> {
    let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
    if ca < 0 || cb < 0 {
        return None;
    }
    let (ca, cb) = (ca as usize, cb as usize);
    let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
    (pa >= 0 && pb >= 0 && pa != pb).then_some((ca, cb))
}

/// The slab under a cell's stretch of margin: the largest beneath it or a
/// neighbour on the same plate, so a trench keeps its slab as cells shuffle
/// along it.
fn established(mesh: &Mesh, data: &CellData, cell: usize) -> f64 {
    let plate = data.plate_id[cell];
    mesh.nbrs(cell)
        .map(|k| mesh.nbr[k] as usize)
        .filter(|&m| data.plate_id[m] == plate)
        .fold(data.slab[cell] as f64, |best, m| {
            best.max(data.slab[m] as f64)
        })
}

/// Polarity a new subduction zone would start with: continent never goes
/// down, otherwise the denser crust does.
fn initiating_over(data: &CellData, a: usize, b: usize) -> usize {
    if data.crust_type[a] == CONTINENTAL {
        a
    } else if data.crust_type[b] == CONTINENTAL || data.density[a] >= data.density[b] {
        b
    } else {
        a
    }
}

/// The subduction across the margin between cells `a` and `b`, or `None` where
/// two continents meet and neither can go down. Where neither side has a slab
/// yet, `initiate` picks the side that would override.
fn subduction_with(
    mesh: &Mesh,
    data: &CellData,
    a: usize,
    b: usize,
    t: &Tuning,
    initiate: impl Fn(usize, usize) -> usize,
) -> Option<Subduction> {
    let cont_a = data.crust_type[a] == CONTINENTAL;
    let cont_b = data.crust_type[b] == CONTINENTAL;
    if cont_a && cont_b {
        return None;
    }
    let full = t.slab_full_length.max(1e-9);
    let trace = SLAB_TRACE * full;
    let (slab_a, slab_b) = (established(mesh, data, a), established(mesh, data, b));
    let a_over = if cont_a || cont_b {
        cont_a
    } else if slab_a > trace || slab_b > trace {
        slab_a >= slab_b
    } else {
        initiate(a, b) == a
    };
    let (over, down, slab) = if a_over {
        (a, b, slab_a)
    } else {
        (b, a, slab_b)
    };
    Some(Subduction {
        over,
        down,
        strength: clamp01(slab / full),
    })
}

/// The subduction across the margin between cells `a` and `b`, or `None` where
/// two continents meet and neither can go down.
pub fn subduction(
    mesh: &Mesh,
    data: &CellData,
    a: usize,
    b: usize,
    t: &Tuning,
) -> Option<Subduction> {
    subduction_with(mesh, data, a, b, t, |a, b| initiating_over(data, a, b))
}

/// Grow each trench's slab by how fast crust is converging into it, start new
/// ones where ocean older than `slab_initiation_age` founders at a margin that
/// is not spreading, and let every slab sink away a little.
pub fn update_slabs(
    mesh: &Mesh,
    data: &mut CellData,
    boundaries: &BoundaryField,
    scratch: &mut Scratch,
    t: &Tuning,
    dt_myr: f64,
) {
    let full = t.slab_full_length.max(1e-9);
    let trace = SLAB_TRACE * full;

    // Which way each plate pair's existing trenches dip, by arc length, so a
    // new stretch of trench joins them rather than dipping the other way.
    let mut dip: HashMap<i64, f64> = HashMap::new();
    for a in 0..mesh.arc_count() {
        let Some((ca, cb)) = margin(mesh, data, a) else {
            continue;
        };
        if established(mesh, data, ca) <= trace && established(mesh, data, cb) <= trace {
            continue;
        }
        let Some(sub) = subduction(mesh, data, ca, cb, t) else {
            continue;
        };
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        let len = mesh.arc_length[a] as f64;
        *dip.entry(suture_key(pa, pb)).or_insert(0.0) += if data.plate_id[sub.over] == pa.min(pb) {
            len
        } else {
            -len
        };
    }
    let initiate = |ca: usize, cb: usize| -> usize {
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        match dip.get(&suture_key(pa, pb)) {
            Some(&v) if v != 0.0 => {
                if (pa < pb) == (v > 0.0) {
                    ca
                } else {
                    cb
                }
            }
            _ => initiating_over(data, ca, cb),
        }
    };

    let founder_rate = full / t.slab_initiation_time.max(1e-6);
    let growth = &mut scratch.f32a;
    growth.fill(0.0);
    for a in 0..mesh.arc_count() {
        let kind = boundaries.arc_kind[a];
        if kind == DIVERGENT {
            continue;
        }
        let Some((ca, cb)) = margin(mesh, data, a) else {
            continue;
        };
        let Some(sub) = subduction_with(mesh, data, ca, cb, t, initiate) else {
            continue;
        };
        let closing = if kind == CONVERGENT {
            (boundaries.arc_closing[a] as f64).max(0.0)
        } else {
            0.0
        };
        // Unforced, a trench only starts where the ocean has grown old and
        // heavy enough to founder on its own, and slowly at that: the
        // Atlantic's 180 Myr-old margins have yet to start one.
        let age = data.age[sub.down] as f64;
        let founder = if data.crust_type[sub.down] == CONTINENTAL || age < t.slab_initiation_age {
            0.0
        } else {
            founder_rate * slab_pull(t, age)
        };
        let rate = (closing + founder) as f32;
        growth[sub.over] = growth[sub.over].max(rate);
    }

    let keep = (-dt_myr / t.slab_decay_time.max(1e-6)).exp();
    for i in 0..mesh.cell_count {
        let slab = (data.slab[i] as f64 * keep + growth[i] as f64 * dt_myr).min(full);
        data.slab[i] = if slab < trace { 0.0 } else { slab as f32 };
    }
}

/// Give every margin that has no slab yet a full one, so a freshly cracked
/// world starts with working trenches rather than at rest. Each pair of plates
/// takes one polarity along the whole margin, by majority of where it would
/// start, so a trench does not alternate direction cell by cell. Spreading
/// ridges are left alone.
pub fn seed_slabs(mesh: &Mesh, data: &mut CellData, boundaries: &BoundaryField, t: &Tuning) {
    let trace = SLAB_TRACE * t.slab_full_length;
    let mut margins: Vec<(usize, usize)> = Vec::new();
    // Per plate pair, arc length voting for the lower-numbered plate overriding.
    let mut vote: HashMap<i64, f64> = HashMap::new();
    for a in 0..mesh.arc_count() {
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 || boundaries.arc_kind[a] == DIVERGENT {
            continue;
        }
        let (ca, cb) = (ca as usize, cb as usize);
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        if pa < 0 || pb < 0 || pa == pb {
            continue;
        }
        if data.crust_type[ca] == CONTINENTAL && data.crust_type[cb] == CONTINENTAL {
            continue;
        }
        if established(mesh, data, ca) > trace || established(mesh, data, cb) > trace {
            continue;
        }
        let over_plate = data.plate_id[initiating_over(data, ca, cb)];
        let len = mesh.arc_length[a] as f64;
        *vote.entry(suture_key(pa, pb)).or_insert(0.0) +=
            if over_plate == pa.min(pb) { len } else { -len };
        margins.push((ca, cb));
    }
    for (ca, cb) in margins {
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        let over = if data.crust_type[ca] == CONTINENTAL || data.crust_type[cb] == CONTINENTAL {
            initiating_over(data, ca, cb)
        } else {
            let lower_over = vote.get(&suture_key(pa, pb)).copied().unwrap_or(0.0) >= 0.0;
            if (pa < pb) == lower_over { ca } else { cb }
        };
        data.slab[over] = t.slab_full_length as f32;
    }
}

#[cfg(test)]
mod tests {
    use crate::world::Sim;

    #[test]
    fn seeded_world_has_trenches() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "seed");
        assert!(sim.front().slab.iter().any(|&s| s > 0.0));
    }
}
