//! One simulated world: the mesh, both cell buffers, plates, ledgers and
//! scratch, with the same tick as `TsWorld.ts`.

use crate::advect::{AdvectionField, advect};
use crate::boundaries::{BoundaryField, classify};
use crate::cell_data::{CellData, total_crust_mass};
use crate::cohesion::{keep_continents_whole, keep_cratons_whole};
use crate::craton::update_cratons;
use crate::crust_lifecycle::{apply_crust_lifecycle, infill_enclosed_basins};
use crate::elevation::apply_elevation;
use crate::mantle::MantleFlow;
use crate::math::{len3, read3};
use std::collections::HashMap;
use crate::mesh::Mesh;
use crate::metrics::{PlateSummary, WorldMetrics, compute_metrics, summaries};
use crate::passive_margins::collapse_passive_margins;
use crate::primordial::{Primordial, effective_tuning, mantle_excess};
use crate::plate_forces::update_plate_motion;
use crate::plate_lifecycle::update_plate_identity;
use crate::plates::{PlateData, initialise_crust, reconcile};
use crate::recycling::{recycle_continental_margins, track_continental_growth};
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
/// Most steps one tick is split into, however fast the plates.
const MAX_SUBSTEPS: usize = 64;
/// Share of the globe a plate must cover for its speed to set how finely a
/// tick is split: a sliver spinning fast is left to lag rather than slow
/// every tick down.
const SUBSTEP_MIN_PLATE_SHARE: f64 = 0.002;

pub struct Sim {
    pub mesh: Mesh,
    front: CellData,
    back: CellData,
    plates: PlateData,
    registry: Registry,
    boundaries: BoundaryField,
    advection: AdvectionField,
    mantle: MantleFlow,
    primordial: Primordial,
    scratch: Scratch,
    raw_labels: Vec<i32>,
    pub tuning: Tuning,
    pub options: SimOptions,
    initial_mass: f64,
    elapsed: f64,
    timings: [f64; PASS_COUNT],
    substeps: usize,
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
            mantle: MantleFlow::new(&mesh),
            primordial: Primordial::default(),
            scratch: Scratch::new(n),
            raw_labels: vec![0; n],
            tuning: Tuning::default(),
            options: SimOptions::default(),
            initial_mass: 0.0,
            elapsed: 0.0,
            timings: [0.0; PASS_COUNT],
            substeps: 1,
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
            initialise_crust(&self.tuning, &self.mesh.centres, &mut self.front, "tectonics");
            self.initial_mass = total_crust_mass(&self.mesh.areas, &self.front);
            self.advection.live_mass = self.initial_mass;
            self.advection.subducted_total = 0.0;
            self.advection.created_total = 0.0;
            self.advection.recycle_debt = 0.0;
            self.advection.continental_growth = 0.0;
            self.advection.continental_share_before = None;
            self.mantle.reset();
            self.primordial.reset();
            self.elapsed = 0.0;
        }
        seed_slabs(&self.mesh, &mut self.front, &self.boundaries, &self.tuning);
        let dt = self.tuning.dt_myr;
        let eff = effective_tuning(&self.tuning, self.elapsed);
        update_plate_motion(
            &self.mesh,
            &self.front,
            &mut self.plates,
            &self.boundaries,
            &self.mantle,
            &mut self.scratch,
            &eff,
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

    /// Advance `dt_myr`. Plates move a whole cell at a time, so a tick that
    /// would carry a plate further than one cell is taken in enough shorter
    /// steps that none does.
    pub fn tick(&mut self) {
        let steps = self.substeps();
        self.substeps = steps;
        let dt = self.tuning.dt_myr / steps as f64;
        self.timings = [0.0; PASS_COUNT];
        let mut totals = [0.0f64; 18];
        for _ in 0..steps {
            self.step(dt);
            for (sum, x) in totals.iter_mut().zip(self.advection.step_tallies_mut()) {
                *sum += *x;
            }
        }
        for (x, sum) in self.advection.step_tallies_mut().into_iter().zip(totals) {
            *x = sum;
        }
    }

    /// Steps this tick needs for the fastest plate of any size to move at
    /// most one cell per step.
    fn substeps(&self) -> usize {
        let n = self.mesh.cell_count;
        let width = (4.0 * std::f64::consts::PI / n as f64).sqrt();
        let mut cells: HashMap<i32, usize> = HashMap::new();
        for &p in &self.front.plate_id {
            *cells.entry(p).or_default() += 1;
        }
        let least = (SUBSTEP_MIN_PLATE_SHARE * n as f64).ceil() as usize;
        let fastest = self
            .plates
            .by_id
            .iter()
            .filter(|(id, _)| cells.get(id).is_some_and(|&c| c >= least))
            .map(|(_, p)| len3(p.omega[0], p.omega[1], p.omega[2]))
            .fold(0.0f64, f64::max);
        ((fastest * self.tuning.dt_myr / width).ceil() as usize).clamp(1, MAX_SUBSTEPS)
    }

    /// Steps the last tick was taken in.
    pub fn last_substeps(&self) -> usize {
        self.substeps
    }

    fn step(&mut self, dt: f64) {
        let Sim {
            mesh,
            front,
            back,
            plates,
            registry,
            boundaries,
            advection,
            mantle,
            scratch,
            primordial,
            tuning,
            options,
            timings,
            elapsed,
            ..
        } = self;
        let eff = effective_tuning(tuning, *elapsed);
        let t = &eff;
        let mut mark = now_ms();
        let mut lap = |pass: usize| {
            let now = now_ms();
            timings[pass] += now - mark;
            mark = now;
        };

        update_slabs(mesh, front, boundaries, scratch, t, dt);
        mantle.update(mesh, front, boundaries, dt, t);
        update_plate_motion(mesh, front, plates, boundaries, mantle, scratch, t, dt);
        lap(0);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(1);
        update_sutures(mesh, front, plates, boundaries, dt, scratch, t, options);
        lap(2);
        advect(
            mesh, front, back, plates, dt, boundaries, advection, scratch, t,
        );
        lap(3);
        update_plate_identity(mesh, back, plates, registry, scratch, t);
        lap(4);
        update_rifting(mesh, back, plates, dt, registry, scratch, t, options);
        collapse_passive_margins(mesh, back, plates, registry, scratch, t);
        lap(5);
        recycle_continental_margins(mesh, back, dt, advection, t);
        infill_enclosed_basins(mesh, back, dt, advection, scratch, t);
        primordial.update(mesh, back, plates, advection, dt, *elapsed, t);
        apply_crust_lifecycle(mesh, back, dt, advection, scratch, t);
        keep_continents_whole(mesh, back, advection, scratch, t);
        track_continental_growth(mesh, back, dt, advection);
        update_cratons(mesh, back, dt, *elapsed, scratch, t);
        keep_cratons_whole(mesh, back, scratch, t);
        lap(6);
        apply_elevation(&mesh.arc_cells, back, dt, boundaries, t);
        lap(7);
        // Advection writes every field of every cell, so the buffers trade.
        std::mem::swap(front, back);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(8);
        *elapsed += dt;
    }

    /// Excess mantle temperature now, from the primordial era; 0 with it off.
    pub fn mantle_excess(&self) -> f64 {
        mantle_excess(&self.tuning, self.elapsed)
    }

    pub fn active_plumes(&self) -> usize {
        self.primordial.active_plumes()
    }

    pub fn front(&self) -> &CellData {
        &self.front
    }

    pub fn plates(&self) -> &PlateData {
        &self.plates
    }

    pub fn mantle(&self) -> &MantleFlow {
        &self.mantle
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
        let mut m = compute_metrics(
            &self.mesh,
            &self.front,
            &self.plates,
            &self.boundaries,
            &self.advection,
            &self.tuning,
            self.elapsed,
        );
        m.mantle_excess = self.mantle_excess();
        m.active_plumes = self.active_plumes();
        m
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

    /// Continental share of the globe after `myr`, sampled every 100 Myr over
    /// the last `window` Myr.
    fn continental_area_after(target: f64, myr: f64, window: f64) -> Vec<f64> {
        let mut sim = Sim::new(4);
        sim.tuning.primordial_heat = 0.0;
        sim.tuning.continental_area_target = target;
        sim.crack_great_circles(4, "growth");
        let ticks = (myr / sim.tuning.dt_myr) as usize;
        let every = (100.0 / sim.tuning.dt_myr) as usize;
        let from = ((myr - window) / sim.tuning.dt_myr) as usize;
        let mut samples = Vec::new();
        for tick in 1..=ticks {
            sim.tick();
            if tick >= from && tick % every == 0 {
                samples.push(sim.metrics().continental_area);
            }
        }
        let drift = sim.mass_drift();
        assert!(drift.abs() < 1e-3, "drift {drift}");
        samples
    }

    /// Two targets under the natural ceiling, which on a mesh this coarse is
    /// only ~0.27.
    #[test]
    fn continental_growth_levels_off_at_the_target() {
        for target in [0.12, 0.22] {
            let samples = continental_area_after(target, 5000.0, 1000.0);
            let mean = samples.iter().sum::<f64>() / samples.len() as f64;
            assert!(
                (mean - target).abs() < 0.04,
                "target {target}: levelled off at {mean:.3} ({samples:.3?})"
            );
        }
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
    /// takes to tear intact continent, beside ocean too young to pull and with
    /// no mantle flow to carry it: the only thing that can break it is the
    /// heat.
    fn hot_continent(tune: impl FnOnce(&mut Tuning)) -> Sim {
        let mut sim = Sim::new(4);
        sim.tuning.mantle_flow_coupling = 0.0;
        sim.tuning.primordial_heat = 0.0;
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
                d.elevation[i] =
                    crate::elevation::baseline_elevation(t, crate::cell_data::CONTINENTAL, 0.0, 35.0)
                        as f32;
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
    fn a_drifting_continent_opens_no_ocean_inside_it() {
        use crate::math::{cross, normalise, scale};
        let mut sim = hot_continent(|_| {});
        sim.options.supercontinent_insulation = false;
        sim.front.mantle_heat.fill(0.0);
        let continent = sim.front.plate_id[0];
        let n = sim.mesh.cell_count;
        let c = &sim.mesh.centres;
        let mut centre = [0.0f64; 3];
        for i in (0..n).filter(|&i| sim.front.plate_id[i] == continent) {
            for (axis, p) in centre.iter_mut().enumerate() {
                *p += c[i * 3 + axis] as f64;
            }
        }
        let centre = normalise(centre);
        let across = normalise(cross(centre, [0.0, 0.0, 1.0]));
        // A pole out in the ocean, so the whole continent drifts across the
        // mesh rather than wheeling about a point of itself.
        let (sin, cos) = 120f64.to_radians().sin_cos();
        let pole = normalise([
            cos * centre[0] + sin * across[0],
            cos * centre[1] + sin * across[1],
            cos * centre[2] + sin * across[2],
        ]);
        let omega = scale(pole, 0.03);
        // Building the continent by hand put crust outside the ledger, so the
        // drift is measured from the first tick on.
        sim.tick();
        let drift = sim.mass_drift();
        let mut holes = 0.0;
        for _ in 0..40 {
            if let Some(p) = sim.plates.by_id.get_mut(&continent) {
                p.omega = omega;
            }
            sim.tick();
            holes += sim.advection.interior_holes;
            // Ocean made this tick with continent all round it.
            let d = &sim.front;
            let continental = |i: usize| d.crust_type[i] == crate::cell_data::CONTINENTAL;
            let opened = (0..n)
                .filter(|&i| {
                    !continental(i)
                        && d.age[i] as f64 <= sim.tuning.dt_myr
                        && sim
                            .mesh
                            .nbrs(i)
                            .all(|k| continental(sim.mesh.nbr[k] as usize))
                })
                .count();
            assert_eq!(opened, 0, "ocean opened inside the continent");
        }
        assert!(holes > 0.0, "the transport tore no gaps to close");
        let change = sim.mass_drift() - drift;
        assert!(
            change.abs() < 1e-4,
            "closing gaps lost or made crust: {change}"
        );
    }

    #[test]
    fn ocean_trapped_inside_a_continent_fills_in() {
        use crate::cell_data::{CONTINENTAL, OCEANIC};
        let mut sim = hot_continent(|_| {});
        sim.options.supercontinent_insulation = false;
        sim.front.mantle_heat.fill(0.0);
        let continent = sim.front.plate_id[0];
        let mesh = &sim.mesh;
        let deep = |i: usize| {
            mesh.nbrs(i).all(|k| {
                let m = mesh.nbr[k] as usize;
                mesh.nbrs(m)
                    .all(|j| sim.front.plate_id[mesh.nbr[j] as usize] == continent)
            })
        };
        let pocket = (0..mesh.cell_count)
            .find(|&i| sim.front.plate_id[i] == continent && deep(i))
            .expect("no cell deep inside the continent");
        // Building the world by hand put crust outside the ledger, so one
        // tick takes it in before the drift is measured.
        sim.tick();
        let drift = sim.mass_drift();
        let planted =
            sim.mesh.area(pocket) * (9.0 - sim.front.thickness[pocket] as f64) / sim.initial_mass;
        let d = &mut sim.front;
        d.crust_type[pocket] = OCEANIC;
        d.thickness[pocket] = 9.0;
        d.age[pocket] = 150.0;
        d.density[pocket] = sim.tuning.oceanic_density_old as f32;
        for p in sim.plates.by_id.values_mut() {
            p.omega = [0.0; 3];
        }
        sim.tick();
        assert_eq!(
            sim.front.crust_type[pocket], CONTINENTAL,
            "a pinhole of ocean was not filled at once"
        );
        let change = sim.mass_drift() - drift - planted;
        assert!(
            change.abs() < 1e-4,
            "filling in lost or made crust: {change}"
        );
    }

    /// Speed (rad/Myr) toward `e` at the centroid of a hemisphere of ocean
    /// floor ageing toward `e`, after a few ticks with nothing but ridge push
    /// to drive it.
    fn drift_down_the_cooling_slope(ridge_weight: f64) -> f64 {
        use crate::cell_data::OCEANIC;
        use crate::math::{cross, dot, normalise};
        let mut sim = Sim::new(4);
        sim.tuning.ridge_weight = ridge_weight;
        sim.tuning.mantle_flow_coupling = 0.0;
        sim.tuning.slab_initiation_age = 1e9;
        sim.options.supercontinent_insulation = false;
        sim.crack_great_circles(1, "ridge push");
        let n = sim.mesh.cell_count;
        let id = sim.front.plate_id[0];
        let mut a = [0.0f64; 3];
        for i in (0..n).filter(|&i| sim.front.plate_id[i] == id) {
            let p = read3(&sim.mesh.centres, i);
            a = [a[0] + p[0], a[1] + p[1], a[2] + p[2]];
        }
        let a = normalise(a);
        let any = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let e = normalise(cross(a, any));
        let d = &mut sim.front;
        for i in 0..n {
            let p = read3(&sim.mesh.centres, i);
            d.crust_type[i] = OCEANIC;
            d.thickness[i] = sim.tuning.oceanic_thickness as f32;
            d.age[i] = (75.0 * (1.0 + dot(p, e))) as f32;
            d.slab[i] = 0.0;
            d.mantle_heat[i] = 0.0;
        }
        for p in sim.plates.by_id.values_mut() {
            p.omega = [0.0; 3];
        }
        sim.elapsed = 10.0 * sim.tuning.seafloor_flattening_age;
        for _ in 0..5 {
            sim.front.slab.fill(0.0);
            sim.tick();
        }
        let omega = sim.plates.by_id[&id].omega;
        dot(cross(omega, a), e)
    }

    #[test]
    fn ridge_push_slides_a_plate_toward_its_old_floor() {
        let pushed = drift_down_the_cooling_slope(Tuning::default().ridge_weight.max(0.05));
        let still = drift_down_the_cooling_slope(0.0);
        assert!(still.abs() < 1e-6, "moved with no ridge push: {still}");
        assert!(pushed > 1e-4, "ridge push barely moved the plate: {pushed}");
    }

    #[test]
    fn ridge_push_comes_in_as_the_starting_sea_floor_is_replaced() {
        use crate::primordial::effective_tuning;
        let t = Tuning::default();
        assert_eq!(effective_tuning(&t, 0.0).ridge_weight, 0.0);
        let half = effective_tuning(&t, 0.5 * t.seafloor_flattening_age).ridge_weight;
        assert!((half - 0.5 * t.ridge_weight).abs() < 1e-12);
        assert_eq!(effective_tuning(&t, 3.0 * t.seafloor_flattening_age).ridge_weight, t.ridge_weight);
    }

    /// A hemisphere of old continent beside a hemisphere of old ocean, at
    /// rest with no slabs, its coast thinned as recycling leaves it.
    fn thinned_coast() -> (Sim, i32, Vec<usize>) {
        use crate::cell_data::{CONTINENTAL, OCEANIC};
        let mut sim = Sim::new(4);
        sim.tuning.mantle_flow_coupling = 0.0;
        sim.options.supercontinent_insulation = false;
        sim.crack_great_circles(1, "coast");
        let n = sim.mesh.cell_count;
        let continent = sim.front.plate_id[0];
        let t = sim.tuning.clone();
        let d = &mut sim.front;
        for i in 0..n {
            d.slab[i] = 0.0;
            d.mantle_heat[i] = 0.0;
            d.age[i] = 150.0;
            if d.plate_id[i] == continent {
                d.crust_type[i] = CONTINENTAL;
                d.thickness[i] = 35.0;
                d.density[i] = t.continental_density as f32;
                d.age[i] = 1000.0;
            } else {
                d.crust_type[i] = OCEANIC;
                d.density[i] = crate::plates::oceanic_density_from_age(&t, 150.0) as f32;
            }
        }
        let coast: Vec<usize> = (0..n)
            .filter(|&i| {
                d.plate_id[i] == continent
                    && sim.mesh.nbrs(i).any(|k| d.plate_id[sim.mesh.nbr[k] as usize] != continent)
            })
            .collect();
        for &i in &coast {
            d.thickness[i] = t.oceanic_thickness as f32;
        }
        for p in sim.plates.by_id.values_mut() {
            p.omega = [0.0; 3];
        }
        (sim, continent, coast)
    }

    #[test]
    fn thinned_continent_reverts_to_mature_ocean_not_ridge_crust() {
        use crate::cell_data::OCEANIC;
        let (mut sim, _, coast) = thinned_coast();
        sim.tick();
        let onset = sim.tuning.slab_pull_onset_age as f32;
        for &i in &coast {
            assert_eq!(sim.front.crust_type[i], OCEANIC, "thinned coast at {i} still continent");
            assert_eq!(sim.front.age[i], onset, "reverted crust at {i} is {} Myr old", sim.front.age[i]);
        }
    }

    #[test]
    fn a_thinned_coast_facing_old_ocean_stays_on_top() {
        let (mut sim, continent, coast) = thinned_coast();
        let ticks = (20.0 / sim.tuning.dt_myr) as usize;
        for _ in 0..ticks {
            for p in sim.plates.by_id.values_mut() {
                p.omega = [0.0; 3];
            }
            sim.tick();
        }
        let d = &sim.front;
        let mesh = &sim.mesh;
        let over_coast = coast.iter().filter(|&&i| d.slab[i] > 0.0).count();
        let over_ocean = (0..mesh.cell_count)
            .filter(|&i| {
                d.plate_id[i] != continent
                    && d.slab[i] > 0.0
                    && mesh.nbrs(i).any(|k| d.plate_id[mesh.nbr[k] as usize] == continent)
            })
            .count();
        assert!(over_coast > 0, "the old ocean never began to founder");
        assert_eq!(over_ocean, 0, "the thinned coast went down under the ocean");
    }

    #[test]
    fn the_starting_sea_floor_varies_smoothly_in_age() {
        let mut sim = Sim::new(5);
        sim.crack_great_circles(4, "smooth");
        let (d, mesh) = (&sim.front, &sim.mesh);
        let (mut diff, mut pairs) = (0.0f64, 0usize);
        for i in 0..mesh.cell_count {
            for k in mesh.nbrs(i) {
                diff += (d.age[i] - d.age[mesh.nbr[k] as usize]).abs() as f64;
                pairs += 1;
            }
        }
        let mean = diff / pairs as f64;
        // Independent uniform ages would differ by a third of the range.
        let noise = sim.tuning.max_oceanic_age / 3.0;
        assert!(mean < 0.1 * noise, "neighbours differ by {mean:.1} Myr on average");
        let ages: Vec<f32> = d.age.clone();
        let span = ages.iter().copied().fold(f32::MIN, f32::max) - ages.iter().copied().fold(f32::MAX, f32::min);
        assert!(span as f64 > 0.9 * sim.tuning.max_oceanic_age, "ages span only {span:.0} Myr");
    }

    #[test]
    fn new_sea_floor_inherits_no_weakness() {
        use crate::math::{cross, normalise, scale};
        let mut sim = Sim::new(4);
        sim.tuning.mantle_flow_coupling = 0.0;
        // A breakup stamps weakness on the margins it leaves, new floor and
        // all; only what is inherited is under test.
        sim.tuning.rift_strength = 1e9;
        sim.crack_great_circles(1, "fresh floor");
        let n = sim.mesh.cell_count;
        sim.front.slab.fill(0.0);
        sim.front.weakness.fill(1.0);
        let ids: Vec<i32> = sim.plates.by_id.keys().copied().collect();
        assert_eq!(ids.len(), 2);
        let centroid = |sim: &Sim, id: i32| {
            let mut c = [0.0f64; 3];
            for i in (0..n).filter(|&i| sim.front.plate_id[i] == id) {
                let p = read3(&sim.mesh.centres, i);
                c = [c[0] + p[0], c[1] + p[1], c[2] + p[2]];
            }
            normalise(c)
        };
        // One hemisphere turns about an axis in the boundary's plane, so half
        // the boundary spreads and the other half closes.
        let a = centroid(&sim, ids[0]);
        let any = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let omegas = [
            (ids[0], scale(normalise(cross(a, any)), 0.02)),
            (ids[1], [0.0; 3]),
        ];
        let mut opened = 0usize;
        for _ in 0..10 {
            for (id, w) in &omegas {
                if let Some(p) = sim.plates.by_id.get_mut(id) {
                    p.omega = *w;
                }
            }
            sim.tick();
            for i in (0..n).filter(|&i| sim.advection.rift_cell[i] != 0) {
                opened += 1;
                assert_eq!(sim.front.weakness[i], 0.0, "new sea floor at {i} came out weak");
            }
        }
        assert!(opened > 0, "the boundary never spread");
    }

    #[test]
    fn a_ridge_shares_its_new_floor_between_both_plates() {
        use crate::math::{cross, normalise, scale};
        let mut sim = Sim::new(4);
        sim.tuning.mantle_flow_coupling = 0.0;
        sim.tuning.rift_strength = 1e9;
        sim.crack_great_circles(1, "fresh floor");
        let n = sim.mesh.cell_count;
        sim.front.slab.fill(0.0);
        let ids: Vec<i32> = sim.plates.by_id.keys().copied().collect();
        let mut a = [0.0f64; 3];
        for i in (0..n).filter(|&i| sim.front.plate_id[i] == ids[0]) {
            let p = read3(&sim.mesh.centres, i);
            a = [a[0] + p[0], a[1] + p[1], a[2] + p[2]];
        }
        let a = normalise(a);
        let any = if a[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        // Only one plate moves; half the boundary spreads behind it.
        let omega = scale(normalise(cross(a, any)), 0.02);
        let mut floor = [0usize; 2];
        for _ in 0..20 {
            for (id, p) in sim.plates.by_id.iter_mut() {
                p.omega = if *id == ids[0] { omega } else { [0.0; 3] };
            }
            sim.tick();
            for i in (0..n).filter(|&i| sim.advection.rift_cell[i] != 0) {
                floor[(sim.front.plate_id[i] != ids[0]) as usize] += 1;
            }
        }
        let total = floor[0] + floor[1];
        assert!(total > 0, "the boundary never spread");
        let still = floor[1] as f64 / total as f64;
        assert!(
            (0.3..=0.7).contains(&still),
            "the plate at rest took {:.0}% of the new floor ({floor:?})",
            still * 100.0
        );
    }

    #[test]
    fn keels_grow_in_quiet_interiors_and_not_at_margins() {
        use crate::cell_data::{CONTINENTAL, OCEANIC};
        let mut sim = Sim::new(4);
        sim.options.supercontinent_insulation = false;
        sim.tuning.mantle_flow_coupling = 0.0;
        // Recycling would eat the coast back to the cratons, which it spares.
        sim.tuning.continental_area_target = 0.0;
        sim.tuning.continental_recycling_rate = 0.0;
        // The hot mantle that grows keels, without plumes raising keeled
        // plateaus of their own along the coast.
        sim.tuning.plume_rate = 0.0;
        sim.crack_great_circles(1, "craton");
        let continent = sim.front.plate_id[0];
        let d = &mut sim.front;
        for i in 0..sim.mesh.cell_count {
            d.mantle_heat[i] = 0.0;
            d.slab[i] = 0.0;
            d.age[i] = 1000.0;
            if d.plate_id[i] == continent {
                d.crust_type[i] = CONTINENTAL;
                d.thickness[i] = 38.0;
                d.density[i] = sim.tuning.continental_density as f32;
            } else {
                d.crust_type[i] = OCEANIC;
            }
        }
        let ticks = (1.5 * sim.tuning.craton_growth_time / sim.tuning.dt_myr) as usize;
        for _ in 0..ticks {
            for p in sim.plates.by_id.values_mut() {
                p.omega = [0.0; 3];
            }
            sim.tick();
        }
        let d = &sim.front;
        let mesh = &sim.mesh;
        let (mut deep, mut edge) = ((0.0f32, 0usize), (0.0f32, 0usize));
        for i in 0..mesh.cell_count {
            if d.crust_type[i] != CONTINENTAL {
                continue;
            }
            let at_edge = mesh.nbrs(i).any(|k| {
                let m = mesh.nbr[k] as usize;
                d.crust_type[m] != CONTINENTAL || d.plate_id[m] != d.plate_id[i]
            });
            let bucket = if at_edge { &mut edge } else { &mut deep };
            bucket.0 += d.keel[i];
            bucket.1 += 1;
        }
        let deep = deep.0 / deep.1.max(1) as f32;
        let edge = edge.0 / edge.1.max(1) as f32;
        assert!(deep > 0.5, "interior barely cratonised: {deep}");
        assert!(edge < 0.1, "margins cratonised: {edge}");
    }

    #[test]
    fn breakup_vents_heat_so_a_continent_does_not_shatter() {
        let (_, rifts) = after_first_rift(&mut hot_continent(|_| {}), 20);
        assert_eq!(rifts, 1, "broke up again within 40 Myr");
        let (_, unvented) = after_first_rift(&mut hot_continent(|t| t.rift_heat_release = 0.0), 20);
        assert!(unvented > 1, "no cascade even without venting: {unvented}");
    }
}
