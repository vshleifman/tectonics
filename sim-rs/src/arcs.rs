//! Volcanic arcs and the back-arc basins behind them.
//!
//! Crust a trench consumes is melted at depth, and the melt rises to build
//! the arc on the overriding plate: not at the trench itself, where the plate
//! is scraped and bent down into the forearc, but 100-200 km behind it, above
//! the part of the slab that has reached the depth where it dehydrates.
//! [`arc_site`] finds that cell for the transport in `advect.rs`, walking
//! `arc_setback` inland from the trench.
//!
//! Behind the arc the overriding plate may be pulled thin. Where it moves away
//! from the trench (its margin retreating from the slab, as the Pacific's
//! western trenches do) the crust between the arc and the plate's interior
//! stretches into a back-arc basin, the Sea of Japan or the Aegean. The
//! stretching thins a band of cells `back_arc_width` across, and what it
//! loses is piled on the arc, so nothing is created or lost. Continental crust
//! thinned to the thickness of ocean floor reverts to it (see
//! `crust_lifecycle.rs`), which is how a back-arc basin opens as a marginal sea.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{Vec3, dot, normalise, read3, sub};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::slabs::subduction;
use crate::tuning::Tuning;

/// Share of a full slab below which a trench is too young to retreat.
const MIN_SLAB: f64 = 0.5;

/// Typical spacing between cell centres (radians).
fn spacing(mesh: &Mesh) -> f64 {
    (4.0 * std::f64::consts::PI / mesh.cell_count.max(1) as f64).sqrt()
}

/// Whole cells `radians` of arc length spans on this mesh. A coarse mesh,
/// whose cells are wider than the distance, takes none.
pub fn rings_for(mesh: &Mesh, radians: f64) -> usize {
    (radians / spacing(mesh)).round().max(0.0) as usize
}

/// Which way is inland of `cell`, as a tangent vector at it: away from the
/// neighbours on other plates, which lie toward the trench. `None` where it
/// has none, or they surround it evenly.
fn inland(mesh: &Mesh, plate_id: &[i32], cell: usize) -> Option<Vec3> {
    let here = read3(&mesh.centres, cell);
    let mut away = [0.0f64; 3];
    for k in mesh.nbrs(cell) {
        let m = mesh.nbr[k] as usize;
        if plate_id[m] != plate_id[cell] && plate_id[m] >= 0 {
            let d = sub(here, read3(&mesh.centres, m));
            for (a, d) in away.iter_mut().zip(d) {
                *a += d;
            }
        }
    }
    let radial = dot(away, here);
    let tangent = [
        away[0] - radial * here[0],
        away[1] - radial * here[1],
        away[2] - radial * here[2],
    ];
    let size = dot(tangent, tangent).sqrt();
    // Pulled equally from opposite sides, which says nothing about direction.
    (size > 1e-9).then(|| normalise(tangent))
}

/// The neighbour of `cell` on its own plate that lies furthest along `dir`,
/// and that `open` allows, if any lies ahead at all.
fn step(
    mesh: &Mesh,
    plate_id: &[i32],
    open: &impl Fn(usize) -> bool,
    cell: usize,
    dir: Vec3,
) -> Option<usize> {
    let here = read3(&mesh.centres, cell);
    let mut best: Option<(usize, f64)> = None;
    for k in mesh.nbrs(cell) {
        let m = mesh.nbr[k] as usize;
        if plate_id[m] != plate_id[cell] || !open(m) {
            continue;
        }
        let ahead = dot(sub(read3(&mesh.centres, m), here), dir);
        // Ties go to the lower cell, so the same mesh always gives the same arc.
        if ahead > 0.0 && best.is_none_or(|(b, a)| ahead > a || (ahead == a && m < b)) {
            best = Some((m, ahead));
        }
    }
    best.map(|(m, _)| m)
}

/// The cells `count` steps inland of trench-side cell `from`, one after
/// another, as far as the overriding plate reaches. Empty where there is no
/// way inland.
fn line_inland(
    mesh: &Mesh,
    plate_id: &[i32],
    open: &impl Fn(usize) -> bool,
    from: usize,
    count: usize,
) -> Vec<usize> {
    let mut line = Vec::with_capacity(count);
    let Some(dir) = inland(mesh, plate_id, from) else {
        return line;
    };
    let mut cell = from;
    for _ in 0..count {
        let Some(next) = step(mesh, plate_id, open, cell, dir) else {
            break;
        };
        line.push(next);
        cell = next;
    }
    line
}

/// The cell the arc forms in, for crust consumed at trench-side cell `from`:
/// `rings` cells inland on the same plate, or as far as the plate allows, or
/// `from` itself where there is no way inland.
pub fn arc_site(
    mesh: &Mesh,
    plate_id: &[i32],
    rift_cell: &[u8],
    from: usize,
    rings: usize,
) -> usize {
    if rings == 0 {
        return from;
    }
    let open = |m: usize| rift_cell[m] == 0;
    line_inland(mesh, plate_id, &open, from, rings)
        .last()
        .copied()
        .unwrap_or(from)
}

/// Stretch the continental crust behind each arc whose plate is moving away
/// from its trench. Returns the area of crust thinned this step.
pub fn extend_back_arcs(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &PlateData,
    boundaries: &BoundaryField,
    dt_myr: f64,
    t: &Tuning,
) -> f64 {
    let width = t.back_arc_width;
    let zone_rings = rings_for(mesh, width).max(1);
    if t.back_arc_extension <= 0.0 || width <= 0.0 {
        return 0.0;
    }
    let setback = rings_for(mesh, t.arc_setback);
    // The strain over the band's real width, as the mesh quantises it.
    let band = zone_rings as f64 * spacing(mesh);
    let cap = t.max_crust_thickness.max(t.continental_thickness_threshold);
    let floor = t.oceanic_thickness.max(t.rift_thickness_floor);
    let mut thinned = 0.0f64;
    for a in 0..mesh.arc_count() {
        if boundaries.arc_kind[a] != CONVERGENT {
            continue;
        }
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let Some(margin) = subduction(mesh, data, ca as usize, cb as usize, t) else {
            continue;
        };
        if margin.strength < MIN_SLAB {
            continue;
        }
        let trench = margin.over;
        let plate = data.plate_id[trench];
        // How fast the plate moves inland, away from its trench.
        let Some(dir) = inland(mesh, &data.plate_id, trench) else {
            continue;
        };
        let retreat = dot(plates.linear_velocity(plate, read3(&mesh.centres, trench)), dir);
        if retreat <= 0.0 {
            continue;
        }
        // The arc, and the band behind it that stretches. Both are the plate's
        // own, and the band is continental crust, which is what thins.
        let open = |_: usize| true;
        let line = line_inland(mesh, &data.plate_id, &open, trench, setback + zone_rings);
        if line.len() <= setback {
            continue;
        }
        let arc = if setback == 0 { trench } else { line[setback - 1] };
        let zone: Vec<usize> = line[setback..]
            .iter()
            .copied()
            .filter(|&m| data.crust_type[m] == CONTINENTAL)
            .collect();
        if zone.is_empty() || data.crust_type[arc] != CONTINENTAL {
            continue;
        }

        let strain = t.back_arc_extension * retreat * dt_myr / band;
        let keep = 1.0 / (1.0 + strain);
        let mut lost: Vec<(usize, f64)> = Vec::with_capacity(zone.len());
        let mut lost_mass = 0.0f64;
        for &m in &zone {
            let h = data.thickness[m] as f64;
            let take = (h - (h * keep).max(floor)).max(0.0);
            lost.push((m, take));
            lost_mass += take * mesh.area(m);
        }
        // The arc takes up what the band gives, as far as it can hold it.
        let room = (cap - data.thickness[arc] as f64).max(0.0) * mesh.area(arc);
        if lost_mass <= 0.0 || room <= 0.0 {
            continue;
        }
        let share = (room / lost_mass).min(1.0);
        for (m, take) in lost {
            data.thickness[m] = (data.thickness[m] as f64 - take * share) as f32;
            thinned += take * share * mesh.area(m);
        }
        let arc_gain = lost_mass * share / mesh.area(arc);
        data.thickness[arc] = (data.thickness[arc] as f64 + arc_gain) as f32;
    }
    thinned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setback_is_a_distance_not_a_number_of_cells() {
        // 160 km, 0.025 rad: no cell of a coarse mesh, a ring on level 6 and
        // several on a fine one.
        let coarse = Mesh::icosphere(4);
        let medium = Mesh::icosphere(6);
        let fine = Mesh::icosphere(8);
        assert_eq!(rings_for(&coarse, 0.025), 0);
        assert_eq!(rings_for(&medium, 0.025), 1);
        assert!(rings_for(&fine, 0.025) >= 3);
    }

    /// Two plates split by the equator, the northern one overriding: where
    /// the arc forms, going north from a cell on the equator's northern side.
    #[test]
    fn the_arc_forms_inland_of_the_trench_on_the_overriding_plate() {
        let mesh = Mesh::icosphere(6);
        let n = mesh.cell_count;
        let plate_id: Vec<i32> = (0..n)
            .map(|i| (mesh.centres[i * 3 + 2] > 0.0) as i32)
            .collect();
        let rift = vec![0u8; n];
        // A cell of the northern plate beside the southern one.
        let from = (0..n)
            .find(|&i| {
                plate_id[i] == 1 && mesh.nbrs(i).any(|k| plate_id[mesh.nbr[k] as usize] == 0)
            })
            .unwrap();
        let rings = 3;
        let site = arc_site(&mesh, &plate_id, &rift, from, rings);
        assert_ne!(site, from);
        assert_eq!(plate_id[site], 1, "the arc left its plate");
        let moved = dot(read3(&mesh.centres, from), read3(&mesh.centres, site))
            .clamp(-1.0, 1.0)
            .acos();
        let want = rings as f64 * spacing(&mesh);
        assert!(
            moved > 0.5 * want && moved < 1.5 * want,
            "moved {moved:.3} rad, wanted about {want:.3}"
        );
        assert!(mesh.centres[site * 3 + 2] > mesh.centres[from * 3 + 2]);
        // No way inland, no setback.
        assert_eq!(arc_site(&mesh, &plate_id, &rift, from, 0), from);
    }

    /// A northern continent over a southern ocean, its trench along the
    /// equator, spinning so that it moves north (away from the trench) where
    /// x < 0 and south (into it) where x > 0.
    #[test]
    fn a_plate_retreating_from_its_trench_stretches_behind_the_arc() {
        use crate::boundaries::BoundaryField;
        use crate::plates::PlateProps;

        let mesh = Mesh::icosphere(6);
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        for i in 0..n {
            let north = mesh.centres[i * 3 + 2] > 0.0;
            data.plate_id[i] = north as i32;
            if north {
                data.crust_type[i] = CONTINENTAL;
                data.thickness[i] = 35.0;
                data.slab[i] = 1.0;
            } else {
                data.thickness[i] = 7.0;
            }
        }
        let mut boundaries = BoundaryField::new(n, mesh.arc_count());
        for a in 0..mesh.arc_count() {
            let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
            if ca >= 0 && cb >= 0 && data.plate_id[ca as usize] != data.plate_id[cb as usize] {
                boundaries.arc_kind[a] = CONVERGENT;
            }
        }
        let mut plates = PlateData::default();
        plates.by_id.insert(1, PlateProps { omega: [0.0, 0.01, 0.0], pending: 0.0, steps: 0 });
        plates.by_id.insert(0, PlateProps { omega: [0.0; 3], pending: 0.0, steps: 0 });

        let t = Tuning::default();
        let mass = |d: &CellData| -> f64 {
            (0..n).map(|i| d.thickness[i] as f64 * mesh.area(i)).sum()
        };
        let before = mass(&data);
        let mut moved = 0.0;
        for _ in 0..10 {
            moved += extend_back_arcs(&mesh, &mut data, &plates, &boundaries, 2.0, &t);
        }
        assert!(moved > 0.0, "nothing stretched");
        assert!((mass(&data) - before).abs() < 1e-4 * before, "crust was made or lost");
        // Thinned where the plate retreats, left alone where it advances.
        let thinnest = |west: bool| {
            (0..n)
                .filter(|&i| data.plate_id[i] == 1 && (mesh.centres[i * 3] < -0.3) == west
                    && (mesh.centres[i * 3] > 0.3) != west)
                .map(|i| data.thickness[i])
                .fold(f32::INFINITY, f32::min)
        };
        assert!(thinnest(true) < 30.0, "no basin behind the retreating margin");
        assert!(thinnest(false) >= 35.0, "the advancing margin stretched");
        // The arc is piled higher than the crust it was built from.
        assert!(
            (0..n).any(|i| data.thickness[i] > 36.0),
            "nothing piled on the arc"
        );
    }
}
