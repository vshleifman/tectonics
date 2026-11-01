//! A hand laid on the globe: plates near it are drawn toward it or pushed away
//! from it, on top of whatever their own forces are doing, for as long as it
//! is held.
//!
//! The hand grabs the crust under it and moves it at `speed` toward (or away
//! from) its centre, fading to nothing at `radius`. Each plate it touches takes
//! the one rigid rotation that best gives the crust under the hand that motion,
//! however large the plate is, so a small hand still visibly moves a large
//! plate. The plate's own motion is left alone: the hand's rotation is added on
//! top while held and taken off again when let go.
//!
//! A hand held long enough grips. A gripping hand that draws plates in folds
//! together, every tick, the plates that meet within its core, as a suture
//! would, leaving the seam weak. One that pushes plates apart lays a rift
//! through its centre across the plate under it and holds it open (see
//! [`crate::rifting::open_rift_through`]) until it breaks or is let go.

use crate::cell_data::CellData;
use crate::math::{Sym3, Vec3, cross, dot, len3, scale, solve_sym3, sub};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::sutures::merge_plates;
use crate::tuning::Tuning;
use std::collections::{BTreeMap, BTreeSet};

/// Share of the hand's reach that is its core, where a gripping hand folds
/// plates together.
pub const CORE_SHARE: f64 = 0.5;

/// Spin about the crust under the hand that the fit is allowed, relative to
/// the turn it is asked for. The crust under a narrow sliver of plate pins
/// down only two of the three axes of its rotation; this keeps the third from
/// running away.
const SPIN_DAMPING: f64 = 0.05;

#[derive(Clone, Copy, Debug)]
pub struct Hand {
    /// Unit vector to the hand's centre.
    pub centre: Vec3,
    /// Angular distance (radians) at which the hand lets go.
    pub radius: f64,
    /// Speed (rad/Myr) the crust under the hand is moved at: toward the centre
    /// if positive, away from it if negative.
    pub speed: f64,
    /// Whether it has been held long enough to grip: to fold plates together
    /// or rift one apart.
    pub grip: bool,
}

/// The cell nearest the hand's centre.
pub fn cell_under(mesh: &Mesh, hand: &Hand) -> usize {
    let c = &mesh.centres;
    let p = hand.centre;
    let closeness =
        |i: usize| c[i * 3] as f64 * p[0] + c[i * 3 + 1] as f64 * p[1] + c[i * 3 + 2] as f64 * p[2];
    (0..mesh.cell_count)
        .max_by(|&a, &b| closeness(a).total_cmp(&closeness(b)))
        .unwrap_or(0)
}

/// Fold together the plate under the hand and every plate that meets it, or
/// meets one that does, within the hand's core. The seams between them are
/// left as weak as a suture's. How many plates were folded away.
pub fn fold_plates(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    hand: &Hand,
    t: &Tuning,
) -> usize {
    let core = (hand.radius * CORE_SHARE).cos();
    let c = &mesh.centres;
    let p = hand.centre;
    let in_core = |i: usize| {
        c[i * 3] as f64 * p[0] + c[i * 3 + 1] as f64 * p[1] + c[i * 3 + 2] as f64 * p[2] > core
    };
    let first = data.plate_id[cell_under(mesh, hand)];
    if first < 0 {
        return 0;
    }
    // Which plates meet which within the core, and along which cells.
    let mut meets: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    let mut seams: Vec<(usize, usize)> = Vec::new();
    for a in 0..mesh.arc_count() {
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (ca, cb) = (ca as usize, cb as usize);
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        if pa < 0 || pb < 0 || pa == pb || !in_core(ca) || !in_core(cb) {
            continue;
        }
        meets.entry(pa).or_default().insert(pb);
        meets.entry(pb).or_default().insert(pa);
        seams.push((ca, cb));
    }
    let mut joined = BTreeSet::from([first]);
    let mut queue = vec![first];
    while let Some(id) = queue.pop() {
        for &other in meets.get(&id).into_iter().flatten() {
            if joined.insert(other) {
                queue.push(other);
            }
        }
    }
    if joined.len() < 2 {
        return 0;
    }
    let weak = t.suture_weakness as f32;
    for (ca, cb) in seams {
        if joined.contains(&data.plate_id[ca]) && joined.contains(&data.plate_id[cb]) {
            data.weakness[ca] = data.weakness[ca].max(weak);
            data.weakness[cb] = data.weakness[cb].max(weak);
        }
    }
    // Each fold keeps the larger of the two, so carry on from whichever kept.
    let folded = joined.len() - 1;
    let mut ids = joined.into_iter();
    let mut keep = ids.next().expect("two plates");
    for other in ids {
        keep = merge_plates(data, plates, keep, other);
    }
    folded
}

/// The rotation (rad/Myr) the hand adds to each plate it touches.
pub fn hand_omegas(mesh: &Mesh, plate_id: &[i32], hand: &Hand) -> BTreeMap<i32, Vec3> {
    let mut fit: BTreeMap<i32, (Sym3, Vec3)> = BTreeMap::new();
    if hand.radius <= 0.0 || hand.speed == 0.0 {
        return BTreeMap::new();
    }
    let p = hand.centre;
    let reach = hand.radius.cos();
    let c = &mesh.centres;
    for (i, &plate) in plate_id.iter().enumerate() {
        if plate < 0 {
            continue;
        }
        let x: Vec3 = [c[i * 3] as f64, c[i * 3 + 1] as f64, c[i * 3 + 2] as f64];
        let along = dot(x, p);
        if along <= reach {
            continue;
        }
        // Toward the centre within the surface; nothing at the centre itself.
        let toward = sub(p, scale(x, along));
        let gap = len3(toward[0], toward[1], toward[2]);
        if gap < 1e-9 {
            continue;
        }
        let theta = along.clamp(-1.0, 1.0).acos() / hand.radius;
        let fade = (1.0 - theta * theta).powi(2);
        let w = fade * mesh.area(i);
        let v = scale(toward, hand.speed / gap);
        let (m, tq) = fit.entry(plate).or_insert(([0.0; 6], [0.0; 3]));
        // Least squares for omega x x = v over the cells: sum w (I - x x^T)
        // omega = sum w (x × v).
        m[0] += w * (1.0 - x[0] * x[0]);
        m[1] += w * (1.0 - x[1] * x[1]);
        m[2] += w * (1.0 - x[2] * x[2]);
        m[3] -= w * x[0] * x[1];
        m[4] -= w * x[0] * x[2];
        m[5] -= w * x[1] * x[2];
        let t = cross(x, v);
        for k in 0..3 {
            tq[k] += w * t[k];
        }
    }
    fit.into_iter()
        .map(|(plate, (mut m, tq))| {
            let weight = (m[0] + m[1] + m[2]) / 2.0;
            for d in &mut m[..3] {
                *d += SPIN_DAMPING * weight;
            }
            (plate, solve_sym3(&m, tq))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::normalise;

    /// Surface velocity of a point under rotation `omega`.
    fn velocity(omega: Vec3, x: Vec3) -> Vec3 {
        cross(omega, x)
    }

    fn halves(mesh: &Mesh) -> Vec<i32> {
        (0..mesh.cell_count)
            .map(|i| if mesh.centres[i * 3] >= 0.0 { 0 } else { 1 })
            .collect()
    }

    #[test]
    fn plates_either_side_of_the_hand_are_drawn_together() {
        let mesh = Mesh::icosphere(4);
        let plates = halves(&mesh);
        let hand = Hand {
            centre: [0.0, 0.0, 1.0],
            radius: 0.4,
            speed: 0.01,
            grip: false,
        };
        let omegas = hand_omegas(&mesh, &plates, &hand);
        // Just either side of the boundary under the hand.
        let east = normalise([0.1, 0.0, 1.0]);
        let west = normalise([-0.1, 0.0, 1.0]);
        let ve = velocity(omegas[&0], east);
        let vw = velocity(omegas[&1], west);
        assert!(ve[0] < -0.005, "the east plate moves west: {ve:?}");
        assert!(vw[0] > 0.005, "the west plate moves east: {vw:?}");
    }

    #[test]
    fn pushing_parts_them_and_a_plate_wholly_under_the_hand_stays() {
        let mesh = Mesh::icosphere(4);
        let plates = halves(&mesh);
        let hand = Hand {
            centre: [0.0, 0.0, 1.0],
            radius: 0.4,
            speed: -0.01,
            grip: false,
        };
        let omegas = hand_omegas(&mesh, &plates, &hand);
        let ve = velocity(omegas[&0], normalise([0.1, 0.0, 1.0]));
        assert!(ve[0] > 0.005, "the east plate moves east: {ve:?}");

        let one = vec![0; mesh.cell_count];
        let still = hand_omegas(&mesh, &one, &hand)[&0];
        assert!(len3(still[0], still[1], still[2]) < 1e-3, "{still:?}");
    }

    #[test]
    fn plates_out_of_reach_are_untouched() {
        let mesh = Mesh::icosphere(4);
        let plates = halves(&mesh);
        let hand = Hand {
            centre: [1.0, 0.0, 0.0],
            radius: 0.3,
            speed: 0.01,
            grip: false,
        };
        let omegas = hand_omegas(&mesh, &plates, &hand);
        assert!(!omegas.contains_key(&1));
    }
}
