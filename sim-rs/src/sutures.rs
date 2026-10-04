//! Suturing and boundary death, mirroring `sutures.ts`: continents in contact
//! and closing weld, welded or quiet boundaries age, and a pair that has been
//! one plate for long enough is merged.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{cross, len3, read3, sub};
use crate::mesh::Mesh;
use crate::plates::{PlateData, split_key, suture_key};
use crate::scratch::Scratch;
use crate::tuning::{SimOptions, Tuning};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;

#[allow(clippy::too_many_arguments)]
pub fn update_sutures(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    boundaries: &BoundaryField,
    dt_myr: f64,
    scratch: &mut Scratch,
    t: &Tuning,
    o: &SimOptions,
) {
    scratch.pack_plates(plates);
    let mut in_contact: IndexSet<i64> = IndexSet::new();
    let mut hardest: HashMap<i64, f64> = HashMap::new();
    // Closing times length over all of each pair's continental contact.
    let mut net: HashMap<i64, f64> = HashMap::new();
    let mut relative_speed: IndexMap<i64, f64> = IndexMap::new();

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
        let pair = suture_key(pa, pb);
        let w = sub(scratch.omega(pa), scratch.omega(pb));
        let r = cross(w, read3(&mesh.arc_mid, a));
        let apart = len3(r[0], r[1], r[2]);
        if apart > relative_speed.get(&pair).copied().unwrap_or(0.0) {
            relative_speed.insert(pair, apart);
        }

        if data.crust_type[ca] != CONTINENTAL || data.crust_type[cb] != CONTINENTAL {
            continue;
        }
        in_contact.insert(pair);
        if plates.sutures.contains_key(&pair) {
            // The seam itself: the weakest line in the continent it joins.
            let w = t.suture_weakness as f32;
            data.weakness[ca] = data.weakness[ca].max(w);
            data.weakness[cb] = data.weakness[cb].max(w);
        }
        *net.entry(pair).or_insert(0.0) +=
            boundaries.arc_closing[a] as f64 * mesh.arc_length[a] as f64;
        if boundaries.arc_kind[a] != CONVERGENT {
            continue;
        }
        let closing = boundaries.arc_closing[a] as f64;
        let h = hardest.entry(pair).or_insert(0.0);
        if closing > *h {
            *h = closing;
        }
    }

    // A suture only survives while the two masses are still in contact.
    plates.sutures.retain(|key, _| in_contact.contains(key));
    for &key in &in_contact {
        if let Some(age) = plates.sutures.get_mut(&key) {
            *age += dt_myr;
        } else {
            let closing = if o.weld_on_net_closing {
                net.get(&key).copied().unwrap_or(0.0)
            } else {
                hardest.get(&key).copied().unwrap_or(0.0)
            };
            if closing > 0.0 {
                plates.sutures.insert(key, dt_myr);
            }
        }
    }

    // Boundaries that have gone quiet, welded or not.
    plates
        .quiet
        .retain(|key, _| relative_speed.contains_key(key));
    for (&key, &apart) in &relative_speed {
        if apart > t.merge_quiet_speed {
            plates.quiet.shift_remove(&key);
        } else {
            *plates.quiet.entry(key).or_insert(0.0) += dt_myr;
        }
    }

    // Merge plates that have been one plate for long enough.
    let mut merging: Vec<i64> = Vec::new();
    for (&key, &age) in &plates.sutures {
        if age < t.suture_merge_time {
            continue;
        }
        let (ia, ib) = split_key(key);
        let (Some(a), Some(b)) = (plates.by_id.get(&ia), plates.by_id.get(&ib)) else {
            continue;
        };
        let d = sub(a.omega, b.omega);
        if len3(d[0], d[1], d[2]) <= t.merge_quiet_speed {
            merging.push(key);
        }
    }
    for (&key, &age) in &plates.quiet {
        if age >= t.boundary_death_time {
            merging.push(key);
        }
    }
    for key in merging {
        let (a, b) = split_key(key);
        if !plates.by_id.contains_key(&a) || !plates.by_id.contains_key(&b) {
            continue;
        }
        merge_plates(data, plates, a, b);
    }
}

/// Fold one plate into the other; the larger keeps its identity.
fn merge_plates(data: &mut CellData, plates: &mut PlateData, a: i32, b: i32) {
    let mut count_a = 0usize;
    let mut count_b = 0usize;
    for &p in &data.plate_id {
        if p == a {
            count_a += 1;
        } else if p == b {
            count_b += 1;
        }
    }
    let keep = if count_a >= count_b { a } else { b };
    let gone = if keep == a { b } else { a };
    // A rift opening across the plate that keeps its identity carries on across
    // the merged one, its sides spreading onto the crust folded in; one across
    // the plate folded in is abandoned.
    for (p, side) in data.plate_id.iter_mut().zip(data.rift_side.iter_mut()) {
        if *p == gone {
            *p = keep;
            *side = 0;
        }
    }
    plates.by_id.shift_remove(&gone);
    plates.opening.shift_remove(&gone);
    rehome(&mut plates.sutures, gone, keep);
    rehome(&mut plates.quiet, gone, keep);
}

/// Move every ledger entry naming `gone` onto `keep`, dropping their shared one.
fn rehome(ledger: &mut IndexMap<i64, f64>, gone: i32, keep: i32) {
    let entries: Vec<(i64, f64)> = ledger.iter().map(|(&k, &v)| (k, v)).collect();
    for (key, age) in entries {
        let (x, y) = split_key(key);
        if x != gone && y != gone {
            continue;
        }
        ledger.shift_remove(&key);
        let partner = if x == gone { y } else { x };
        if partner == keep {
            continue;
        }
        let moved = suture_key(keep, partner);
        let prev = ledger.get(&moved).copied().unwrap_or(0.0);
        ledger.insert(moved, prev.max(age));
    }
}
