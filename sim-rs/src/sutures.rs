//! Suturing and boundary death, mirroring `sutures.ts`: continents in contact
//! and closing weld, welded or quiet boundaries age, and a pair that has been
//! one plate for long enough is merged.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{cross, len3, read3, sub};
use crate::mesh::Mesh;
use crate::plates::{PlateData, split_key, suture_key};
use crate::scratch::Scratch;
use crate::tuning::{CRATON_KEEL, SimOptions, Tuning};
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
    // Length-weighted speed, added in arc order so the mean is deterministic.
    let mut speed_moment: IndexMap<i64, f64> = IndexMap::new();
    let mut boundary_len: IndexMap<i64, f64> = IndexMap::new();
    let mut craton_contact: IndexSet<i64> = IndexSet::new();

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
        let arc_len = mesh.arc_length[a] as f64;
        *speed_moment.entry(pair).or_insert(0.0) += apart * arc_len;
        *boundary_len.entry(pair).or_insert(0.0) += arc_len;

        if data.crust_type[ca] != CONTINENTAL || data.crust_type[cb] != CONTINENTAL {
            continue;
        }
        in_contact.insert(pair);
        if data.keel[ca] >= CRATON_KEEL && data.keel[cb] >= CRATON_KEEL {
            craton_contact.insert(pair);
        }
        *net.entry(pair).or_insert(0.0) +=
            boundaries.arc_closing[a] as f64 * mesh.arc_length[a] as f64;
        if boundaries.arc_kind[a] != CONVERGENT {
            continue;
        }
        if plates.sutures.contains_key(&pair) {
            // The seam itself, while the collision is still squeezing it: the
            // weakest line in the continent it joins. Once the two sides are
            // quietly welded it keeps what it was given, fading only slowly,
            // and stops wearing away any keel along it.
            let w = t.suture_weakness as f32;
            data.weakness[ca] = data.weakness[ca].max(w);
            data.weakness[cb] = data.weakness[cb].max(w);
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
    let sutures = &plates.sutures;
    plates
        .craton_welds
        .retain(|key, _| craton_contact.contains(key) && sutures.contains_key(key));
    for key in craton_contact {
        if plates.sutures.contains_key(&key) {
            *plates.craton_welds.entry(key).or_insert(0.0) += dt_myr;
        }
    }

    // Boundaries that have gone quiet, welded or not.
    plates
        .quiet
        .retain(|key, _| relative_speed.contains_key(key));
    for (&key, &apart) in &relative_speed {
        // The fastest spot on a boundary is a harsh test: one transform
        // stretch keeps the whole boundary alive. The length-weighted mean
        // lets a boundary that is quiet almost everywhere still count.
        let speed = if o.quiet_by_mean_speed {
            let len = boundary_len.get(&key).copied().unwrap_or(0.0);
            if len > 0.0 {
                speed_moment.get(&key).copied().unwrap_or(0.0) / len
            } else {
                apart
            }
        } else {
            apart
        };
        if speed > t.merge_quiet_speed {
            plates.quiet.shift_remove(&key);
        } else {
            *plates.quiet.entry(key).or_insert(0.0) += dt_myr;
        }
    }

    // Merge plates that have been one plate for long enough. A pair can qualify
    // more than once; the first list that queues it is why it merged, and the
    // later ones find one of the plates already gone.
    const WELD: u8 = 0;
    const QUIET: u8 = 1;
    const CRATON: u8 = 2;
    let mut merging: Vec<(i64, u8)> = Vec::new();
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
            merging.push((key, WELD));
        }
    }
    for (&key, &age) in &plates.quiet {
        if age >= t.boundary_death_time {
            merging.push((key, QUIET));
        }
    }
    // Cratons that have stayed welded together this long are one craton, and
    // the plates carrying them one plate, whatever slack is left in their
    // motions. The seam stays weak, so the union can still rift along it.
    if t.craton_merge_time > 0.0 {
        for (&key, &age) in &plates.craton_welds {
            if age >= t.craton_merge_time {
                merging.push((key, CRATON));
            }
        }
    }
    for (key, cause) in merging {
        let (a, b) = split_key(key);
        if !plates.by_id.contains_key(&a) || !plates.by_id.contains_key(&b) {
            continue;
        }
        match cause {
            WELD => plates.weld_merges += 1,
            QUIET => plates.quiet_merges += 1,
            _ => plates.craton_merges += 1,
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
    rehome(&mut plates.craton_welds, gone, keep);
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
