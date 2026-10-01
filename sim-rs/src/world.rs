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
            self.tuning.dt_myr,
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
}
