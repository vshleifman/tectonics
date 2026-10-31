//! Land wears down and the wear runs downhill to the coast. Each cell above
//! sea level loses crust at a rate set by its height, the loss is passed from
//! cell to cell down the steepest descent on the same plate, and the sea
//! floor beside the coast keeps it: a shelf where it fills up to
//! `shelf_depth`, then a slope that tails off. Nothing is made: every km
//! removed is deposited in the same call, except that most of what reaches
//! the overriding edge of a trench goes down with the plate instead.
//!
//! What is shed is what the land is made of. A cell loses its loose sediment
//! cover before its basement, and the rock that travels downhill is felsic or
//! mafic clastic depending on where it came from, so a granite range sheds
//! different sediment from a basalt plateau. It settles as cover on top of
//! whatever is there, and cover buried deeper than `lithification_depth` turns
//! to rock, part of the basement from then on.
//!
//! Basement that erosion removes is rock that was buried there, so each cell
//! keeps how deep it has cut (`exhumed`) and the hottest and most squeezed the
//! rock now at the surface has been, from the geotherm at that depth.

use crate::advect::{AdvectionField, descending_key};
use crate::cell_data::{
    CONTINENTAL, COVER_LANES, CellData, LANE_COVER_FELSIC, LANE_COVER_MAFIC, LANE_EXHUMED,
    LANE_FELSIC, LANE_MAFIC, LANE_SEDIMENT, LANES,
};
use crate::climate::{Climate, Setting, deposit_lanes, weathering_rate};
use crate::elevation::water_load_factor;
use crate::mesh::Mesh;
use crate::metamorphism::raise_peaks;
use crate::par;
use crate::tuning::Tuning;

/// Cell is not part of the routed region.
const OUTSIDE: u8 = u8::MAX;

/// Take `take` km off the top of cell `i`: its cover first, then its basement.
/// Returns the clastic rock shed (area x km) in each cover lane, and how much
/// of it came off the cover.
///
/// The plume lane, which records where crust came from, is left as it was.
/// Basement taken adds to the cell's exhumation, and the rock it now shows has
/// been as deep as that, `heat_flow_mw` being the cell's heat flow and
/// `mantle_c` the mantle's temperature (°C).
fn strip(
    data: &mut CellData,
    i: usize,
    take: f64,
    area: f64,
    heat_flow_mw: f64,
    mantle_c: f64,
    t: &Tuning,
) -> ([f64; COVER_LANES], f64) {
    let h = data.thickness[i] as f64;
    let lanes: [f64; LANES] = data.lanes.get(i).map(|x| x as f64);
    let cover = [
        lanes[LANE_COVER_FELSIC] * h,
        lanes[LANE_COVER_MAFIC] * h,
    ];
    let cover_total = (cover[0] + cover[1]).min(h);
    let basement = (h - cover_total).max(0.0);
    let (mafic, felsic) = (lanes[LANE_MAFIC] * h, lanes[LANE_FELSIC] * h);
    // Sediment that is no longer cover has been buried into the basement.
    let metasediment = (lanes[LANE_SEDIMENT] * h - cover_total).max(0.0);

    let from_cover = take.min(cover_total);
    let from_basement = (take - from_cover).min(basement);
    let mut shed = [0.0; COVER_LANES];
    if cover_total > 0.0 {
        let sum = cover[0] + cover[1];
        shed[0] += from_cover * cover[0] / sum;
        shed[1] += from_cover * cover[1] / sum;
    }
    if basement > 0.0 {
        // Felsic rock and what was sediment shed felsic clastic; mafic sheds mafic.
        shed[0] += from_basement * (felsic + metasediment) / basement;
        shed[1] += from_basement * mafic / basement;
    }

    data.thickness[i] = (h - take) as f32;
    let h_after = data.thickness[i] as f64;
    let left_cover = if cover_total > 0.0 { 1.0 - from_cover / cover_total } else { 0.0 };
    let left_basement = if basement > 0.0 { 1.0 - from_basement / basement } else { 0.0 };
    let cover = [cover[0] * left_cover, cover[1] * left_cover];
    let (mafic, felsic, metasediment) = (
        mafic * left_basement,
        felsic * left_basement,
        metasediment * left_basement,
    );
    let share = |km: f64| (km / h_after).clamp(0.0, 1.0) as f32;
    data.lanes.lane_mut(LANE_MAFIC)[i] = share(mafic);
    data.lanes.lane_mut(LANE_FELSIC)[i] = share(felsic);
    data.lanes.lane_mut(LANE_COVER_FELSIC)[i] = share(cover[0]);
    data.lanes.lane_mut(LANE_COVER_MAFIC)[i] = share(cover[1]);
    data.lanes.lane_mut(LANE_SEDIMENT)[i] = share(metasediment + cover[0] + cover[1]);
    if from_basement > 0.0 {
        let exhumed = data.lanes.lane(LANE_EXHUMED)[i] as f64 + from_basement;
        data.lanes.lane_mut(LANE_EXHUMED)[i] = exhumed as f32;
        raise_peaks(data, i, heat_flow_mw, exhumed, mantle_c, t);
    }
    (shed.map(|x| x * area), from_cover * area)
}

/// Cover deeper than `depth` km is buried, compacted and cemented into rock:
/// the excess leaves the cover lanes and stays in the sediment lane, as part
/// of the basement. What turns to rock was buried as deep as the cover was
/// thick, so the cell's peaks rise to the conditions there.
fn lithify(data: &mut CellData, depth: f64, heat_flow: &[f32], mantle_c: f64, t: &Tuning) {
    for i in 0..data.thickness.len() {
        let h = data.thickness[i] as f64;
        let (f, m) = (
            data.lanes.lane(LANE_COVER_FELSIC)[i] as f64,
            data.lanes.lane(LANE_COVER_MAFIC)[i] as f64,
        );
        let cover = (f + m) * h;
        if cover > depth.max(0.0) {
            let keep = depth.max(0.0) / cover;
            data.lanes.lane_mut(LANE_COVER_FELSIC)[i] = (f * keep) as f32;
            data.lanes.lane_mut(LANE_COVER_MAFIC)[i] = (m * keep) as f32;
            raise_peaks(data, i, heat_flow[i] as f64, cover, mantle_c, t);
        }
    }
}

/// Erode every cell standing above sea level and deposit what it sheds,
/// reading `data.elevation` (km above sea, as of the last tick). Sets
/// `field.sediment_eroded`, `field.sediment_deposited` and
/// `field.sediment_subducted` (area x km), the first the sum of the others,
/// and `field.sediment_eroded_cover`, the part of the former that was
/// loose cover. `heat_flow` (mW/m²) sets how hot the rock was at depth, up to
/// the mantle's temperature `mantle_c` (°C).
pub fn erode_and_deposit(
    mesh: &Mesh,
    data: &mut CellData,
    field: &mut AdvectionField,
    dt_myr: f64,
    t: &Tuning,
    climate: &Climate,
    heat_flow: &[f32],
    mantle_c: f64,
) {
    field.sediment_eroded = 0.0;
    field.sediment_deposited = 0.0;
    field.sediment_eroded_cover = 0.0;
    field.sediment_subducted = 0.0;
    field.sediment_deposited_trench_edge = 0.0;
    field.sediment_deposited_near_trench = 0.0;
    if t.erosion_time <= 0.0 || dt_myr <= 0.0 {
        return;
    }
    let n = mesh.cell_count;
    // 2 on the overriding edge of a trench (slab beneath, the other plate
    // beside), 1 within a cell of one on the same plate, else 0.
    let edge: Vec<bool> = (0..n)
        .map(|i| {
            data.slab[i] > 0.0
                && mesh
                    .nbrs(i)
                    .any(|k| data.plate_id[mesh.nbr[k] as usize] != data.plate_id[i])
        })
        .collect();
    let trench: Vec<u8> = (0..n)
        .map(|i| {
            if edge[i] {
                2
            } else if mesh.nbrs(i).any(|k| {
                let m = mesh.nbr[k] as usize;
                edge[m] && data.plate_id[m] == data.plate_id[i]
            }) {
                1
            } else {
                0
            }
        })
        .collect();
    let (mut at_edge, mut near_trench) = (0.0f64, 0.0f64);
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
    // What `load` is made of: the mass (area x km) of each kind of clastic rock.
    let mut load_lanes = vec![[0.0f64; COVER_LANES]; n];
    let (mut eroded, mut deposited, mut eroded_cover) = (0.0f64, 0.0f64, 0.0f64);
    let mut subducted = 0.0f64;
    let down_share = t.trench_sediment_subduction.clamp(0.0, 1.0);
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
            let take = (excess * dt_myr / t.erosion_time
                * t.craton_wear(data.keel[i])
                * weathering_rate(climate.sample(i)))
            .min(excess)
            .min(available);
            if take > 0.0 {
                let (shed, from_cover) = strip(data, i, take, area, heat_flow[i] as f64, mantle_c, t);
                for lane in 0..COVER_LANES {
                    load_lanes[i][lane] += shed[lane];
                }
                load[i] += take * area;
                eroded += take * area;
                eroded_cover += from_cover;
            }
        }

        let mut have = load[i];
        let have_lanes = load_lanes[i];
        load[i] = 0.0;
        load_lanes[i] = [0.0; COVER_LANES];
        if have <= 0.0 {
            continue;
        }
        if trench[i] == 2 {
            // At a trench most of what arrives goes down with the plate; the
            // rest builds the prism, here or on its way inboard.
            let down = have * down_share;
            subducted += down;
            have -= down;
            if have <= 0.0 {
                continue;
            }
        }
        // What the load is made of, as shares of it.
        let made_of = {
            let total = have_lanes[0] + have_lanes[1];
            if total > 0.0 {
                [have_lanes[0] / total, have_lanes[1] / total]
            } else {
                [0.5, 0.5]
            }
        };

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

        let (keep, setting) = match target {
            // A basin, or the edge of the region, keeps what reaches it.
            None => (have, Setting::Basin),
            Some(_) if ring[i] == 0 => (0.0, Setting::Slope),
            Some(_) if ring[i] >= reach => (have, Setting::Basin),
            Some(_) if ring[i] == 1 => {
                // The shelf fills up to `shelf_depth` and passes on the rest.
                let room = ((t.shelf_depth - here as f64) / (c * k)).max(0.0);
                (have.min(room * area), Setting::Shelf)
            }
            Some(_) => (have * t.slope_capture.clamp(0.0, 1.0), Setting::Slope),
        };
        if keep > 0.0 {
            let added = keep / area;
            let before = data.thickness[i] as f64;
            // What settles may not be all that arrived: a climate can turn
            // some of it into other kinds of rock.
            let laid = deposit_lanes(
                setting,
                climate.sample(i),
                [keep * made_of[0], keep * made_of[1]],
            );
            data.lanes.mix_lane(LANE_SEDIMENT, i, before, added, 1.0);
            // Silt dilutes the igneous rock beneath it.
            data.lanes.mix_lane(LANE_MAFIC, i, before, added, 0.0);
            data.lanes.mix_lane(LANE_FELSIC, i, before, added, 0.0);
            data.lanes
                .mix_lane(LANE_COVER_FELSIC, i, before, added, laid[0] / keep);
            data.lanes
                .mix_lane(LANE_COVER_MAFIC, i, before, added, laid[1] / keep);
            data.thickness[i] = (before + added) as f32;
            deposited += keep;
            match trench[i] {
                2 => {
                    at_edge += keep;
                    near_trench += keep;
                }
                1 => near_trench += keep,
                _ => {}
            }
        }
        if let Some(m) = target {
            let passed = have - keep;
            load[m] += passed;
            for lane in 0..COVER_LANES {
                load_lanes[m][lane] += passed * made_of[lane];
            }
        }
    }
    lithify(data, t.lithification_depth, heat_flow, mantle_c, t);
    field.sediment_eroded = eroded;
    field.sediment_deposited = deposited;
    field.sediment_eroded_cover = eroded_cover;
    field.sediment_subducted = subducted;
    field.subducted_total += subducted;
    field.sediment_deposited_trench_edge = at_edge;
    field.sediment_deposited_near_trench = near_trench;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::{LANE_PEAK_PRESSURE, LANE_PEAK_TEMP, OCEANIC};
    use crate::elevation::column_height;
    use crate::geotherm::temperature_at;
    use crate::metamorphism::{Facies, facies};
    use crate::lithosphere::settle_lithosphere;

    /// The mantle beneath the test worlds (°C), about today's.
    const MANTLE_C: f64 = 1330.0;

    /// A level 4 sphere on one plate: ocean everywhere but a continent over
    /// the northern hemisphere.
    struct World {
        mesh: Mesh,
        data: CellData,
        field: AdvectionField,
        t: Tuning,
        /// Heat flow (mW/m²) everywhere.
        heat_flow: Vec<f32>,
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
                data.density[i] = t.mean_continental_density() as f32;
                data.age[i] = 1000.0;
            } else {
                data.crust_type[i] = OCEANIC;
                data.thickness[i] = t.oceanic_thickness as f32;
                data.density[i] = t.mafic_density as f32;
                data.age[i] = 60.0;
            }
        }
        settle_lithosphere(&mut data, &t);
        let field = AdvectionField::new(n);
        let heat_flow = vec![70.0; n];
        let mut w = World { mesh, data, field, t, heat_flow };
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
            erode_and_deposit(
                &self.mesh,
                &mut self.data,
                &mut self.field,
                dt,
                &self.t,
                &Climate::default(),
                &self.heat_flow,
                MANTLE_C,
            );
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
            let f = &w.field;
            let laid_or_down = f.sediment_deposited + f.sediment_subducted;
            assert!((f.sediment_eroded - laid_or_down).abs() < 1e-9 * before);
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
        assert!((0..w.mesh.cell_count).any(|i| w.data.lanes.lane(LANE_SEDIMENT)[i] > 0.0));
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
        assert_eq!(w.field.sediment_subducted, 0.0, "a passive coast subducted sediment");
    }

    /// The northern continent on one plate, a coastal range along its edge,
    /// and the southern ocean on another plate, going down beneath the
    /// continent's edge when `trench` is set. Returns the edge cells.
    fn range_over(w: &mut World, trench: bool) -> Vec<usize> {
        for i in 0..w.mesh.cell_count {
            if w.z(i) <= 0.0 {
                w.data.plate_id[i] = 1;
            }
            if w.z(i) > 0.0 && w.z(i) < 0.15 {
                w.data.thickness[i] = 60.0;
            }
        }
        let edge: Vec<usize> = (0..w.mesh.cell_count)
            .filter(|&i| {
                w.data.plate_id[i] == 0
                    && w.mesh.nbrs(i).any(|k| w.data.plate_id[w.mesh.nbr[k] as usize] == 1)
            })
            .collect();
        if trench {
            for &i in &edge {
                w.data.slab[i] = 1.0;
            }
        }
        w.rise();
        edge
    }

    #[test]
    fn a_coastal_range_beside_a_trench_sends_the_set_share_down() {
        let one_step = |share: f64| {
            let mut w = world();
            w.t.trench_sediment_subduction = share;
            let edge = range_over(&mut w, true);
            let before = w.data.thickness.clone();
            let mass = w.mass();
            w.step(2.0);
            let f = &w.field;
            let laid_or_down = f.sediment_deposited + f.sediment_subducted;
            assert!((f.sediment_eroded - laid_or_down).abs() < 1e-9 * mass, "ledger open");
            let edge_gain: f64 = edge
                .iter()
                .map(|&i| (w.data.thickness[i] - before[i]).max(0.0) as f64)
                .sum();
            (f.sediment_subducted, f.sediment_eroded, edge_gain)
        };
        let (all, eroded, all_gain) = one_step(1.0);
        let (most, _, _) = one_step(0.7);
        assert!(all > 0.0, "nothing reached the trench");
        assert!(all < eroded, "everything eroded went down");
        assert_eq!(all_gain, 0.0, "the edge kept sediment when all of it goes down");
        // Edge cells can pass load along to one another, so what the second
        // keeps back can go down at the next: between the share and all.
        assert!(most >= 0.7 * all - 1e-12 && most <= all, "{most} against {all}");
    }

    #[test]
    fn the_same_range_with_no_trench_subducts_nothing() {
        let mut w = world();
        range_over(&mut w, false);
        for _ in 0..5 {
            w.step(2.0);
            assert_eq!(w.field.sediment_subducted, 0.0);
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

    /// Make the whole northern continent one kind of rock.
    fn continent_of(w: &mut World, felsic: bool) {
        for i in 0..w.mesh.cell_count {
            if w.data.crust_type[i] == CONTINENTAL {
                let lanes = if felsic { [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] };
                w.data.lanes.set(i, lanes);
            }
        }
    }

    /// Mass (area x km) of the cover in each cover lane, over the whole globe.
    fn cover_mass(w: &World) -> [f64; COVER_LANES] {
        let mut mass = [0.0; COVER_LANES];
        for i in 0..w.mesh.cell_count {
            let km = w.mesh.area(i) * w.data.thickness[i] as f64;
            mass[0] += km * w.data.lanes.lane(LANE_COVER_FELSIC)[i] as f64;
            mass[1] += km * w.data.lanes.lane(LANE_COVER_MAFIC)[i] as f64;
        }
        mass
    }

    fn ranged_world() -> World {
        let mut w = world();
        // Keep every bit of cover: lithification has its own test.
        w.t.lithification_depth = 1e9;
        for i in 0..w.mesh.cell_count {
            if w.z(i) > 0.0 && w.z(i) < 0.3 {
                w.data.thickness[i] = 60.0;
            }
        }
        w.rise();
        w
    }

    #[test]
    fn a_granite_range_sheds_felsic_clastic_and_a_basalt_plateau_mafic() {
        for felsic in [true, false] {
            let mut w = ranged_world();
            continent_of(&mut w, felsic);
            for _ in 0..25 {
                w.step(2.0);
            }
            let [fel, maf] = cover_mass(&w);
            let (shed, other) = if felsic { (fel, maf) } else { (maf, fel) };
            assert!(shed > 0.0, "nothing was shed");
            assert!(other < 1e-6 * shed, "wrong kind of clastic: {shed} against {other}");
        }
    }

    #[test]
    fn the_cover_ledger_closes() {
        let mut w = ranged_world();
        let mut made_from_basement = 0.0;
        for _ in 0..40 {
            w.step(2.0);
            made_from_basement += w.field.sediment_eroded - w.field.sediment_eroded_cover;
        }
        let [fel, maf] = cover_mass(&w);
        assert!(made_from_basement > 0.0);
        // Cover is only made by turning basement to silt: what is stripped off
        // the cover is laid down again.
        assert!(
            ((fel + maf) / made_from_basement - 1.0).abs() < 1e-3,
            "cover holds {}, basement shed {made_from_basement}",
            fel + maf
        );
    }

    #[test]
    fn cover_is_stripped_before_the_basement() {
        let mut w = world();
        w.t.lithification_depth = 1e9;
        let peak = (0..w.mesh.cell_count)
            .max_by(|&a, &b| w.z(a).total_cmp(&w.z(b)))
            .unwrap();
        // 60 km of crust, 3 km of it mafic silt, over 57 km of felsic basement.
        w.data.thickness[peak] = 60.0;
        w.data.lanes.set(peak, [0.0, 0.05, 0.0, 0.95, 0.0, 0.05, 0.0, 0.0, 0.0, 0.0]);
        w.rise();
        let (h0, basement0) = (w.data.thickness[peak] as f64, 0.95 * 60.0);
        w.step(2.0);
        let h1 = w.data.thickness[peak] as f64;
        assert!(h1 < h0, "the peak did not erode");
        let lanes = w.data.lanes.get(peak);
        let basement = lanes[LANE_FELSIC] as f64 * h1;
        assert!((basement - basement0).abs() < 1e-3, "basement lost {}", basement0 - basement);
        let cover = lanes[LANE_COVER_MAFIC] as f64 * h1;
        assert!((cover - (3.0 - (h0 - h1))).abs() < 1e-3, "cover is {cover} km");
        // And sediment still counts the cover.
        assert!((lanes[LANE_SEDIMENT] as f64 * h1 - cover).abs() < 1e-3);
    }

    /// The facies of the rock now at the surface of cell `i`.
    fn facies_at(w: &World, i: usize) -> Facies {
        facies(
            &w.t,
            w.data.lanes.lane(LANE_PEAK_TEMP)[i] as f64,
            w.data.lanes.lane(LANE_PEAK_PRESSURE)[i] as f64,
        )
    }

    /// A cell of 60 km of felsic crust, as an orogen leaves behind.
    fn orogen(w: &mut World) -> usize {
        let cell = (0..w.mesh.cell_count)
            .max_by(|&a, &b| w.z(a).total_cmp(&w.z(b)))
            .unwrap();
        w.data.thickness[cell] = 60.0;
        w.data.lanes.set(cell, {
            let mut lanes = [0.0; LANES];
            lanes[LANE_FELSIC] = 1.0;
            lanes
        });
        cell
    }

    #[test]
    fn deep_erosion_exposes_hot_rock_and_shallow_erosion_does_not() {
        let mut w = world();
        let cell = orogen(&mut w);
        let area = w.mesh.area(cell);
        // At 70 mW/m² the geotherm puts the rock 2 km down far below zeolite.
        strip(&mut w.data, cell, 2.0, area, 70.0, MANTLE_C, &w.t);
        assert_eq!(w.data.lanes.lane(LANE_EXHUMED)[cell], 2.0);
        assert_eq!(facies_at(&w, cell), Facies::None);
        // 25 km down it is amphibolite: 15 + 70 * 25 / 2.5 - 1 * 25² / 5 = 590 °C.
        strip(&mut w.data, cell, 23.0, area, 70.0, MANTLE_C, &w.t);
        assert_eq!(w.data.lanes.lane(LANE_EXHUMED)[cell], 25.0);
        let temp = w.data.lanes.lane(LANE_PEAK_TEMP)[cell];
        assert!((temp - 590.0).abs() < 1.0, "peak {temp} °C");
        assert_eq!(facies_at(&w, cell), Facies::Amphibolite);
    }

    #[test]
    fn peaks_only_rise_and_cover_does_not_exhume() {
        let mut w = world();
        let cell = orogen(&mut w);
        let area = w.mesh.area(cell);
        strip(&mut w.data, cell, 10.0, area, 90.0, MANTLE_C, &w.t);
        let hot = w.data.lanes.lane(LANE_PEAK_TEMP)[cell];
        // The same cut again at a lower heat flow reaches deeper but cooler
        // rock than the record: the peak stays.
        strip(&mut w.data, cell, 1.0, area, 30.0, MANTLE_C, &w.t);
        assert!(w.data.lanes.lane(LANE_PEAK_TEMP)[cell] >= hot);
        assert_eq!(w.data.lanes.lane(LANE_EXHUMED)[cell], 11.0);
        // Stripping loose cover is not exhumation.
        let mut w = world();
        let cell = orogen(&mut w);
        w.data.lanes.set(cell, {
            let mut lanes = [0.0; LANES];
            lanes[LANE_FELSIC] = 0.9;
            lanes[LANE_SEDIMENT] = 0.1;
            lanes[LANE_COVER_FELSIC] = 0.1;
            lanes
        });
        strip(&mut w.data, cell, 4.0, w.mesh.area(cell), 70.0, MANTLE_C, &w.t);
        assert_eq!(w.data.lanes.lane(LANE_EXHUMED)[cell], 0.0);
        assert_eq!(w.data.lanes.lane(LANE_PEAK_TEMP)[cell], 0.0);
    }

    #[test]
    fn exhumation_only_rises_as_land_erodes() {
        let mut w = ranged_world();
        let mut last = w.data.lanes.lane(LANE_EXHUMED).to_vec();
        for _ in 0..40 {
            w.step(2.0);
            let now = w.data.lanes.lane(LANE_EXHUMED);
            for i in 0..w.mesh.cell_count {
                assert!(now[i] >= last[i], "cell {i} exhumed {} then {}", last[i], now[i]);
            }
            last = now.to_vec();
        }
        assert!(last.iter().any(|&x| x > 0.0), "nothing was exhumed");
    }

    #[test]
    fn once_cover_is_buried_deep_enough_it_becomes_basement() {
        let mut w = world();
        // Far from any coast: a south-polar cell with 5 km of silt on 2 km of basalt.
        let cell = (0..w.mesh.cell_count)
            .min_by(|&a, &b| w.z(a).total_cmp(&w.z(b)))
            .unwrap();
        w.data.thickness[cell] = 7.0;
        w.data.lanes.set(cell, [0.0, 5.0 / 7.0, 2.0 / 7.0, 0.0, 0.0, 5.0 / 7.0, 0.0, 0.0, 0.0, 0.0]);
        w.step(2.0);
        let lanes = w.data.lanes.get(cell);
        let cover = lanes[LANE_COVER_MAFIC] as f64 * 7.0;
        assert!((cover - w.t.lithification_depth).abs() < 1e-4, "cover is {cover} km");
        // The silt that turned to rock lay 5 km down.
        let temp = w.data.lanes.lane(LANE_PEAK_TEMP)[cell] as f64;
        assert!((temp - temperature_at(&w.t, 70.0, 5.0)).abs() < 1e-3, "peak {temp} °C");
        // It is still sediment, now part of the basement.
        assert!((lanes[LANE_SEDIMENT] as f64 * 7.0 - 5.0).abs() < 1e-4);
        assert_eq!(w.data.thickness[cell], 7.0);
    }
}
