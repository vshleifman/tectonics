//! One simulated world: the mesh, both cell buffers, plates, ledgers and
//! scratch, with the same tick as `TsWorld.ts`.

use crate::advect::{AdvectionField, advect};
use crate::boundaries::{BoundaryField, classify};
use crate::cell_data::{CellData, total_crust_mass};
use crate::crust_lifecycle::apply_crust_lifecycle;
use crate::elevation::apply_elevation;
use crate::math::read3;
use crate::mesh::Mesh;
use crate::metrics::{PlateSummary, WorldMetrics, compute_metrics, summaries};
use crate::plate_forces::update_plate_motion;
use crate::plate_lifecycle::update_plate_identity;
use crate::plates::{PlateData, initialise_crust, reconcile};
use crate::registry::{Registry, assign_plates};
use crate::rifting::update_rifting;
use crate::rng::Rng;
use crate::scratch::Scratch;
use crate::slabs::{seed_slabs, update_slabs};
use crate::sutures::update_sutures;
use crate::timer::now_ms;
use crate::tuning::{SimOptions, Tuning};

/// Passes timed per tick, in order; matches `PASS_NAMES` on the TypeScript side.
pub const PASS_COUNT: usize = 9;

pub struct Sim {
    pub mesh: Mesh,
    front: CellData,
    back: CellData,
    plates: PlateData,
    registry: Registry,
    boundaries: BoundaryField,
    advection: AdvectionField,
    scratch: Scratch,
    raw_labels: Vec<i32>,
    pub tuning: Tuning,
    pub options: SimOptions,
    initial_mass: f64,
    elapsed: f64,
    timings: [f64; PASS_COUNT],
}

impl Sim {
    /// Build a world of the given subdivision level. Every buffer the tick
    /// needs is allocated here.
    pub fn new(level: u32) -> Sim {
        let mesh = Mesh::icosphere(level);
        let n = mesh.cell_count;
        let arcs = mesh.arc_count();
        Sim {
            front: CellData::new(n),
            back: CellData::new(n),
            plates: PlateData::default(),
            registry: Registry::default(),
            boundaries: BoundaryField::new(n, arcs),
            advection: AdvectionField::new(n),
            scratch: Scratch::new(n),
            raw_labels: vec![0; n],
            tuning: Tuning::default(),
            options: SimOptions::default(),
            initial_mass: 0.0,
            elapsed: 0.0,
            timings: [0.0; PASS_COUNT],
            mesh,
        }
    }

    pub fn reassign_plates(&mut self, cracked: &[u8], init_crust: bool) -> usize {
        let count = assign_plates(&self.mesh, cracked, &mut self.raw_labels);
        let report = self
            .registry
            .relabel(&self.raw_labels, count, &mut self.front.plate_id);
        reconcile(&mut self.plates, &report);
        if init_crust {
            initialise_crust(&self.tuning, &mut self.front, "tectonics");
            self.initial_mass = total_crust_mass(&self.mesh.areas, &self.front);
            self.advection.live_mass = self.initial_mass;
            self.advection.subducted_total = 0.0;
            self.advection.created_total = 0.0;
            self.elapsed = 0.0;
        }
        seed_slabs(&self.mesh, &mut self.front, &self.boundaries, &self.tuning);
        let dt = self.tuning.dt_myr;
        update_plate_motion(
            &self.mesh,
            &self.front,
            &mut self.plates,
            &self.boundaries,
            &mut self.scratch,
            &self.tuning,
            dt,
        );
        classify(
            &self.mesh,
            &self.front,
            &self.plates,
            &mut self.boundaries,
            &mut self.scratch,
            &self.tuning,
        );
        count
    }

    pub fn tick(&mut self) {
        let Sim {
            mesh,
            front,
            back,
            plates,
            registry,
            boundaries,
            advection,
            scratch,
            tuning: t,
            options,
            timings,
            ..
        } = self;
        let dt = t.dt_myr;
        let mut mark = now_ms();
        let mut lap = |pass: usize| {
            let now = now_ms();
            timings[pass] = now - mark;
            mark = now;
        };

        update_slabs(mesh, front, boundaries, scratch, t, dt);
        update_plate_motion(mesh, front, plates, boundaries, scratch, t, dt);
        lap(0);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(1);
        update_sutures(mesh, front, plates, boundaries, dt, scratch, t);
        lap(2);
        advect(
            mesh, front, back, plates, dt, boundaries, advection, scratch, t,
        );
        lap(3);
        update_plate_identity(mesh, back, plates, registry, scratch, t);
        lap(4);
        update_rifting(mesh, back, plates, dt, registry, scratch, t, options);
        lap(5);
        apply_crust_lifecycle(&mesh.areas, back, dt, advection, t);
        lap(6);
        apply_elevation(&mesh.arc_cells, back, dt, boundaries, t);
        lap(7);
        // Advection writes every field of every cell, so the buffers trade.
        std::mem::swap(front, back);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(8);
        self.elapsed += dt;
    }

    pub fn front(&self) -> &CellData {
        &self.front
    }

    pub fn boundaries(&self) -> &BoundaryField {
        &self.boundaries
    }

    pub fn advection(&self) -> &AdvectionField {
        &self.advection
    }

    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    pub fn timings(&self) -> &[f64; PASS_COUNT] {
        &self.timings
    }

    pub fn plate_count(&self) -> usize {
        self.plates.by_id.len()
    }

    pub fn rift_count(&self) -> u64 {
        self.plates.rifts
    }

    pub fn failed_rift_count(&self) -> u64 {
        self.plates.failed_rifts
    }

    pub fn plate_omegas(&self) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.plates.by_id.len() * 4);
        for (&id, p) in &self.plates.by_id {
            out.extend_from_slice(&[id as f64, p.omega[0], p.omega[1], p.omega[2]]);
        }
        out
    }

    pub fn live_mass(&self) -> f64 {
        self.advection.live_mass
    }

    /// Conservation drift as a fraction of the initial crust mass.
    pub fn mass_drift(&self) -> f64 {
        let a = &self.advection;
        let tracked = a.live_mass + a.subducted_total - a.created_total;
        if self.initial_mass > 0.0 {
            (tracked - self.initial_mass) / self.initial_mass
        } else {
            0.0
        }
    }

    pub fn metrics(&self) -> WorldMetrics {
        compute_metrics(
            &self.mesh,
            &self.front,
            &self.plates,
            &self.boundaries,
            &self.advection,
            &self.tuning,
            self.elapsed,
        )
    }

    pub fn summaries(&self) -> Vec<PlateSummary> {
        summaries(&self.mesh, &self.front, &self.plates)
    }

    /// Crack every arc whose two cells lie on opposite sides of one of
    /// `planes` random great circles, then build the world on the resulting
    /// plates. A stand-in for the TypeScript crack walk, for native tests and
    /// profiling only.
    pub fn crack_great_circles(&mut self, planes: usize, seed: &str) {
        let mut rng = Rng::new(seed);
        let normals: Vec<[f64; 3]> = (0..planes)
            .map(|_| {
                let v = [
                    rng.next_f64() - 0.5,
                    rng.next_f64() - 0.5,
                    rng.next_f64() - 0.5,
                ];
                crate::math::normalise(v)
            })
            .collect();
        let mut cracked = vec![0u8; self.mesh.arc_count()];
        for (a, flag) in cracked.iter_mut().enumerate() {
            let p = read3(&self.mesh.centres, self.mesh.arc_cells[a * 2] as usize);
            let q = read3(&self.mesh.centres, self.mesh.arc_cells[a * 2 + 1] as usize);
            if normals
                .iter()
                .any(|n| crate::math::dot(*n, p).signum() != crate::math::dot(*n, q).signum())
            {
                *flag = 1;
            }
        }
        self.reassign_plates(&vec![0u8; self.mesh.arc_count()], true);
        self.reassign_plates(&cracked, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_ledger_closes() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "ledger");
        for _ in 0..60 {
            sim.tick();
        }
        let drift = sim.mass_drift();
        assert!(drift.abs() < 1e-4, "drift {drift}");
        assert!(sim.plate_count() > 1);
    }

    #[test]
    fn world_keeps_moving() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(3, "moving");
        for _ in 0..40 {
            sim.tick();
        }
        let m = sim.metrics();
        assert!(m.elapsed_myr > 0.0);
        assert!(m.boundary_share[0] > 0.0, "no convergence at all");
    }

    /// A hemisphere of continent at rest over mantle heat just past what it
    /// takes to tear intact continent, beside ocean too young to pull: the only
    /// thing that can break it is the heat.
    fn hot_continent(tune: impl FnOnce(&mut Tuning)) -> Sim {
        let mut sim = Sim::new(4);
        tune(&mut sim.tuning);
        sim.crack_great_circles(1, "dome");
        let continent = sim.front.plate_id[0];
        let t = &sim.tuning;
        let threshold =
            t.rift_strength / (t.insulation_drive + t.rift_strength * t.insulation_weakening);
        let heat = (threshold + 0.05) as f32;
        let d = &mut sim.front;
        for i in 0..sim.mesh.cell_count {
            d.slab[i] = 0.0;
            d.age[i] = 0.0;
            if d.plate_id[i] == continent {
                d.crust_type[i] = crate::cell_data::CONTINENTAL;
                d.thickness[i] = 35.0;
                d.density[i] = t.continental_density as f32;
                d.elevation[i] = t.continent_base as f32;
                d.mantle_heat[i] = heat;
            }
        }
        for p in sim.plates.by_id.values_mut() {
            p.omega = [0.0; 3];
        }
        sim
    }

    /// Ticks until the first breakup, then `after` more; the relative angular
    /// speed of the continent's two pieces, and the breakups in all.
    fn after_first_rift(sim: &mut Sim, after: usize) -> (f64, u64) {
        let stretching = (sim.tuning.rift_duration / sim.tuning.dt_myr).ceil() as usize;
        for _ in 0..stretching + 5 {
            sim.tick();
            if sim.rift_count() > 0 {
                break;
            }
        }
        assert_eq!(sim.rift_count(), 1, "the hot continent never rifted");
        for _ in 0..after {
            sim.tick();
        }
        let mut pieces: Vec<i32> = (0..sim.mesh.cell_count)
            .filter(|&i| sim.front.crust_type[i] == crate::cell_data::CONTINENTAL)
            .map(|i| sim.front.plate_id[i])
            .collect();
        pieces.sort_unstable();
        pieces.dedup();
        assert!(pieces.len() >= 2, "continent is one plate again");
        let w = |id: i32| sim.plates.by_id[&id].omega;
        let (a, b) = (w(pieces[0]), w(pieces[1]));
        let d = crate::math::sub(a, b);
        (crate::math::len3(d[0], d[1], d[2]), sim.rift_count())
    }

    #[test]
    fn heat_alone_rifts_a_continent_and_pushes_it_apart() {
        // Hemmed in by ocean that cannot subduct, the pieces cannot get far,
        // but the dome push is the only thing that moves them at all.
        let (apart, _) = after_first_rift(&mut hot_continent(|_| {}), 10);
        let (still, _) = after_first_rift(&mut hot_continent(|t| t.dome_push = 0.0), 10);
        assert!(still < 1e-6, "pieces separated with no dome push: {still}");
        assert!(apart > 5e-5, "pieces barely moving apart: {apart}");
    }

    /// Ticks until a rift has begun to stretch on the hot continent.
    fn until_stretching(sim: &mut Sim) {
        for _ in 0..5 {
            sim.tick();
            if !sim.plates.opening.is_empty() {
                return;
            }
        }
        panic!("the hot continent never began to rift");
    }

    #[test]
    fn a_stretching_rift_sinks_between_raised_shoulders_and_loses_no_crust() {
        let mut sim = hot_continent(|_| {});
        until_stretching(&mut sim);
        // Building the continent by hand put crust outside the ledger, so it
        // is the drift while stretching that has to stay at zero.
        let drift = sim.mass_drift();
        let ticks = (0.6 * sim.tuning.rift_duration / sim.tuning.dt_myr) as usize;
        for _ in 0..ticks {
            sim.tick();
        }
        assert_eq!(sim.rift_count(), 0, "broke up before it had stretched");
        let d = &sim.front;
        let zone: Vec<usize> = (0..sim.mesh.cell_count)
            .filter(|&i| {
                d.rift_side[i] != 0
                    && sim.mesh.nbrs(i).any(|k| {
                        let m = sim.mesh.nbr[k] as usize;
                        d.rift_side[m] != 0 && d.rift_side[m] != d.rift_side[i]
                    })
            })
            .collect();
        assert!(!zone.is_empty());
        let mean = zone.iter().map(|&i| d.thickness[i] as f64).sum::<f64>() / zone.len() as f64;
        assert!(mean < 30.0, "zone barely thinned: {mean:.1} km");
        let thickest = d.thickness.iter().copied().fold(0.0f32, f32::max);
        assert!(
            thickest > 36.0,
            "no shoulders: thickest crust {thickest:.1} km"
        );
        let change = sim.mass_drift() - drift;
        assert!(
            change.abs() < 1e-4,
            "crust lost or gained while stretching: {change}"
        );
    }

    #[test]
    fn a_rift_that_loses_its_drive_fails_and_stays_weak() {
        let mut sim = hot_continent(|_| {});
        until_stretching(&mut sim);
        sim.tick();
        let zone: Vec<usize> = (0..sim.mesh.cell_count)
            .filter(|&i| sim.front.rift_side[i] != 0 && sim.front.weakness[i] > 0.0)
            .collect();
        assert!(!zone.is_empty(), "stretching left no mark");
        sim.front.mantle_heat.fill(0.0);
        sim.tick();
        assert_eq!(sim.failed_rift_count(), 1);
        assert!(sim.plates.opening.is_empty());
        assert!(
            sim.front.rift_side.iter().all(|&s| s == 0),
            "marks left behind"
        );
        let margin = sim.tuning.rift_margin_weakness as f32;
        let scarred = zone
            .iter()
            .filter(|&&i| sim.front.weakness[i] >= 0.99 * margin)
            .count();
        assert!(scarred > 0, "failed rift left no weak scar");
        for _ in 0..30 {
            sim.tick();
        }
        assert_eq!(sim.rift_count(), 0, "a failed rift still broke the plate");
    }

    #[test]
    fn breakup_vents_heat_so_a_continent_does_not_shatter() {
        let (_, rifts) = after_first_rift(&mut hot_continent(|_| {}), 20);
        assert_eq!(rifts, 1, "broke up again within 40 Myr");
        let (_, unvented) = after_first_rift(&mut hot_continent(|t| t.rift_heat_release = 0.0), 20);
        assert!(unvented > 1, "no cascade even without venting: {unvented}");
    }
}
