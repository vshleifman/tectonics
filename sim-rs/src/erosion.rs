//! Land wears down and the wear runs downhill to the coast. Each cell above
//! sea level loses crust at a rate set by its height, the loss is passed from
//! cell to cell down the steepest descent on the same plate, and the sea
//! floor beside the coast keeps it: a shelf where it fills up to
//! `shelf_depth`, then a slope that tails off. Nothing is made or lost: every
//! km removed is deposited, in the same call.

use crate::advect::{AdvectionField, descending_key};
use crate::cell_data::{CONTINENTAL, CellData, mixed_share};
use crate::elevation::water_load_factor;
use crate::mesh::Mesh;
use crate::par;
use crate::tuning::Tuning;

/// Cell is not part of the routed region.
const OUTSIDE: u8 = u8::MAX;

/// Erode every cell standing above sea level and deposit what it sheds,
/// reading `data.elevation` (km above sea, as of the last tick). Sets
/// `field.sediment_eroded` and `field.sediment_deposited` (area x km), which
/// match.
pub fn erode_and_deposit(
    mesh: &Mesh,
    data: &mut CellData,
    field: &mut AdvectionField,
    dt_myr: f64,
    t: &Tuning,
) {
    field.sediment_eroded = 0.0;
    field.sediment_deposited = 0.0;
    if t.erosion_time <= 0.0 || dt_myr <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    let reach = t.sediment_reach.round().clamp(1.0, 200.0) as u8;

    // Land is ring 0; the sea within `reach` cells of it is ring 1 and out.
    let mut ring = vec![OUTSIDE; n];
    let mut frontier: Vec<usize> = Vec::new();
    for i in 0..n {
        if data.elevation[i] > 0.0 {
            ring[i] = 0;
            frontier.push(i);
        }
    }
    let mut next = Vec::new();
    for r in 1..=reach {
        next.clear();
        for &i in &frontier {
            for k in mesh.nbrs(i) {
                let m = mesh.nbr[k] as usize;
                if ring[m] == OUTSIDE {
                    ring[m] = r;
                    next.push(m);
                }
            }
        }
        std::mem::swap(&mut frontier, &mut next);
    }

    // Highest first, ties by index, so the order is the same on any thread count.
    let mut order: Vec<u64> = (0..n)
        .filter(|&i| ring[i] != OUTSIDE)
        .map(|i| descending_key(data.elevation[i], i as u32))
        .collect();
    par::sort_unstable(&mut order);

    let k = water_load_factor(t);
    let mut load = vec![0.0f64; n];
    let (mut eroded, mut deposited) = (0.0f64, 0.0f64);
    for &key in &order {
        let i = (key & 0xffff_ffff) as usize;
        let area = mesh.area(i);
        let c = ((t.mantle_density - data.density[i] as f64) / t.mantle_density).max(1e-3);

        if ring[i] == 0 {
            // Height over sea level is the excess crust holding it up.
            let excess = data.elevation[i] as f64 / c;
            let floor = if data.crust_type[i] == CONTINENTAL {
                t.continental_thickness_threshold
            } else {
                t.oceanic_thickness
            };
            let available = (data.thickness[i] as f64 - floor).max(0.0);
            let take = (excess * dt_myr / t.erosion_time * t.craton_wear(data.keel[i]))
                .min(excess)
                .min(available);
            if take > 0.0 {
                data.thickness[i] = (data.thickness[i] as f64 - take) as f32;
                load[i] += take * area;
                eroded += take * area;
            }
        }

        let have = load[i];
        load[i] = 0.0;
        if have <= 0.0 {
            continue;
        }

        // Downhill on the same plate, within the region.
        let plate = data.plate_id[i];
        let here = data.elevation[i];
        let mut target: Option<usize> = None;
        for kk in mesh.nbrs(i) {
            let m = mesh.nbr[kk] as usize;
            if ring[m] == OUTSIDE || data.plate_id[m] != plate || data.elevation[m] >= here {
                continue;
            }
            if target.is_none_or(|b| data.elevation[m] < data.elevation[b]) {
                target = Some(m);
            }
        }

        let keep = match target {
            // A basin, or the edge of the region, keeps what reaches it.
            None => have,
            Some(_) if ring[i] == 0 => 0.0,
            Some(_) if ring[i] >= reach => have,
            Some(_) if ring[i] == 1 => {
                // The shelf fills up to `shelf_depth` and passes on the rest.
                let room = ((t.shelf_depth - here as f64) / (c * k)).max(0.0);
                have.min(room * area)
            }
            Some(_) => have * t.slope_capture.clamp(0.0, 1.0),
        };
        if keep > 0.0 {
            let added = keep / area;
            let before = data.thickness[i] as f64;
            data.sediment_share[i] = mixed_share(data.sediment_share[i], before, added, 1.0);
            data.thickness[i] = (before + added) as f32;
            deposited += keep;
        }
        if let Some(m) = target {
            load[m] += have - keep;
        }
    }
    field.sediment_eroded = eroded;
    field.sediment_deposited = deposited;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::OCEANIC;
    use crate::elevation::column_height;
    use crate::lithosphere::settle_lithosphere;

    /// A level 4 sphere on one plate: ocean everywhere but a continent over
    /// the northern hemisphere.
    struct World {
        mesh: Mesh,
        data: CellData,
        field: AdvectionField,
        t: Tuning,
    }

    fn world() -> World {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning::default();
        let mut data = CellData::new(n);
        for i in 0..n {
            let north = mesh.centres[i * 3 + 2] > 0.0;
            data.plate_id[i] = 0;
            if north {
                data.crust_type[i] = CONTINENTAL;
                data.thickness[i] = 40.0;
                data.density[i] = t.continental_density as f32;
                data.age[i] = 1000.0;
            } else {
                data.crust_type[i] = OCEANIC;
                data.thickness[i] = t.oceanic_thickness as f32;
                data.density[i] = t.oceanic_density as f32;
                data.age[i] = 60.0;
            }
        }
        settle_lithosphere(&mut data, &t);
        let field = AdvectionField::new(n);
        let mut w = World { mesh, data, field, t };
        w.rise();
        w
    }

    impl World {
        /// Elevation from the columns, with the sea at 0.
        fn rise(&mut self) {
            for i in 0..self.mesh.cell_count {
                let d = &self.data;
                self.data.elevation[i] = column_height(
                    &self.t,
                    d.thickness[i] as f64,
                    d.density[i] as f64,
                    d.lithosphere[i] as f64,
                    d.keel[i] as f64,
                    0.0,
                    0.0,
                ) as f32;
            }
        }

        fn mass(&self) -> f64 {
            (0..self.mesh.cell_count)
                .map(|i| self.mesh.area(i) * self.data.thickness[i] as f64)
                .sum()
        }

        fn step(&mut self, dt: f64) {
            erode_and_deposit(&self.mesh, &mut self.data, &mut self.field, dt, &self.t);
            self.rise();
        }

        fn z(&self, i: usize) -> f64 {
            self.mesh.centres[i * 3 + 2] as f64
        }
    }

    #[test]
    fn what_is_eroded_is_deposited_and_the_ledger_closes() {
        let mut w = world();
        for i in 0..w.mesh.cell_count {
            if w.z(i) > 0.9 {
                w.data.thickness[i] = 60.0;
            }
        }
        w.rise();
        let before = w.mass();
        for _ in 0..50 {
            w.step(2.0);
            assert!((w.field.sediment_eroded - w.field.sediment_deposited).abs() < 1e-9 * before);
        }
        assert!(w.field.sediment_eroded > 0.0);
        assert!((w.mass() - before).abs() < 1e-4 * before, "mass drifted {}", w.mass() - before);
    }

    #[test]
    fn a_single_peak_decays_toward_sea_level_with_the_erosion_time() {
        let mut w = world();
        let peak = (0..w.mesh.cell_count)
            .max_by(|&a, &b| w.z(a).total_cmp(&w.z(b)))
            .unwrap();
        w.data.thickness[peak] = 60.0;
        w.rise();
        let start = w.data.elevation[peak] as f64;
        let mut elapsed = 0.0;
        while (w.data.elevation[peak] as f64) > 0.5 * start && elapsed < 1000.0 {
            w.step(2.0);
            elapsed += 2.0;
        }
        let expected = w.t.erosion_time * std::f64::consts::LN_2;
        assert!(
            (elapsed - expected).abs() < 0.3 * expected,
            "half the relief gone after {elapsed} Myr, not about {expected:.0}"
        );
    }

    #[test]
    fn a_coastal_range_builds_a_shelf_on_its_own_plate() {
        let mut w = world();
        for i in 0..w.mesh.cell_count {
            if w.z(i) > 0.0 && w.z(i) < 0.15 {
                w.data.thickness[i] = 60.0;
            }
        }
        w.rise();
        let before = w.data.thickness.clone();
        for _ in 0..25 {
            w.step(2.0);
        }
        let gained: f64 = (0..w.mesh.cell_count)
            .filter(|&i| w.data.crust_type[i] == OCEANIC && w.data.thickness[i] > before[i])
            .map(|i| w.mesh.area(i) * (w.data.thickness[i] - before[i]) as f64)
            .sum();
        assert!(gained > 0.0, "no shelf was built");
        assert!((0..w.mesh.cell_count).any(|i| w.data.sediment_share[i] > 0.0));
    }

    #[test]
    fn nothing_crosses_a_plate_boundary() {
        let mut w = world();
        for i in 0..w.mesh.cell_count {
            if w.z(i) <= 0.0 {
                w.data.plate_id[i] = 1;
            }
            if w.z(i) > 0.0 && w.z(i) < 0.15 {
                w.data.thickness[i] = 60.0;
            }
        }
        w.rise();
        let before = w.data.thickness.clone();
        w.step(2.0);
        for i in 0..w.mesh.cell_count {
            if w.data.plate_id[i] == 1 {
                assert_eq!(w.data.thickness[i], before[i], "cell {i} across the boundary changed");
            }
        }
    }

    #[test]
    fn a_closed_basin_fills() {
        let mut w = world();
        let pit = (0..w.mesh.cell_count)
            .max_by(|&a, &b| w.z(a).total_cmp(&w.z(b)))
            .unwrap();
        for i in 0..w.mesh.cell_count {
            if w.data.crust_type[i] == CONTINENTAL {
                w.data.thickness[i] = 55.0;
            }
        }
        w.data.thickness[pit] = 42.0;
        w.rise();
        assert!(w.data.elevation[pit] > 0.0, "the pit is under water");
        let before = w.data.thickness[pit];
        w.step(2.0);
        assert!(w.data.thickness[pit] > before, "the pit did not fill");
    }
}
