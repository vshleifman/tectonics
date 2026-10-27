//! One simulated world: the mesh, both cell buffers, plates, ledgers and
//! scratch, with the same tick as `TsWorld.ts`.

use crate::advect::{AdvectionField, advect};
use crate::arcs::extend_back_arcs;
use crate::boundaries::{BoundaryField, classify};
use crate::cell_data::{CellData, total_crust_mass};
use crate::cohesion::{keep_continents_whole, keep_cratons_whole};
use crate::craton::update_cratons;
use crate::crust_lifecycle::{apply_crust_lifecycle, infill_enclosed_basins, lay_new_floor};
use crate::elevation::{apply_elevation, dynamic_topography};
use crate::erosion::erode_and_deposit;
use crate::heat::{PlanetHeat, effective_tuning};
use crate::lithosphere::update_lithosphere;
use crate::mantle::MantleFlow;
use crate::math::read3;
use crate::mesh::Mesh;
use crate::metrics::{PlateSummary, WorldMetrics, compute_metrics, summaries};
use crate::par;
use crate::passive_margins::collapse_passive_margins;
use crate::plate_forces::update_plate_motion;
use crate::plate_lifecycle::update_plate_identity;
use crate::plates::{PlateData, initialise_crust, reconcile};
use crate::primordial::Primordial;
use crate::recycling::{recycle_continental_margins, steer_accretion, track_continental_growth};
use crate::registry::{Registry, assign_plates};
use crate::rifting::update_rifting;
use crate::rng::Rng;
use crate::scratch::Scratch;
use crate::slabs::{seed_slabs, update_slabs};
use crate::sutures::update_sutures;
use crate::timer::now_ms;
use crate::tuning::{SimOptions, Tuning};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;

/// Opens every saved world, ahead of the hash of the source that wrote it.
const STATE_MAGIC: [u8; 4] = *b"TECT";
/// Magic, source hash (u64), level (u32).
const STATE_HEADER: usize = 16;
/// Hash of `sim-rs/src` when this build was made (see `build.rs`).
const SOURCE_HASH: u64 = {
    let digits = env!("SIM_SOURCE_HASH").as_bytes();
    let mut hash = 0u64;
    let mut i = 0;
    while i < digits.len() {
        hash = hash * 10 + (digits[i] - b'0') as u64;
        i += 1;
    }
    hash
};

/// Everything of a [`Sim`] that carries history. What is left out is the mesh,
/// which `level` rebuilds, the buffer the tick writes into (a copy of the front
/// one on load), and scratch space.
#[derive(Serialize, Deserialize)]
struct State<'a> {
    front: Cow<'a, CellData>,
    plates: Cow<'a, PlateData>,
    registry: Cow<'a, Registry>,
    boundaries: Cow<'a, BoundaryField>,
    advection: Cow<'a, AdvectionField>,
    mantle: Cow<'a, MantleFlow>,
    primordial: Cow<'a, Primordial>,
    heat: Cow<'a, PlanetHeat>,
    dyn_topo: Cow<'a, [f32]>,
    sea_level: f64,
    tuning: Cow<'a, Tuning>,
    options: Cow<'a, SimOptions>,
    initial_mass: f64,
    elapsed: f64,
    substeps: usize,
}

/// Passes timed per tick, in order; matches `PASS_NAMES` on the TypeScript side.
pub const PASS_COUNT: usize = 10;
/// Most steps one tick is split into, however fast the plates.
const MAX_SUBSTEPS: usize = 64;
/// Share of the globe whose plates must keep up when a tick is split: the
/// few small plates spinning faster than the rest lag at a cell a step rather
/// than slow every tick down.
const SUBSTEP_PLATE_SHARE: f64 = 0.95;

/// Add the time since `mark` to `pass`, and restart the clock.
fn lap(timings: &mut [f64; PASS_COUNT], mark: &mut f64, pass: usize) {
    let now = now_ms();
    timings[pass] += now - *mark;
    *mark = now;
}

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
    heat: PlanetHeat,
    /// Dynamic topography (km) over each cell: where the surface stands for
    /// the mantle's flow, not its crust, so it is not carried with the plates.
    dyn_topo: Vec<f32>,
    /// Sea level (km above the datum) that the planet's water fills the basins
    /// to, as of the last tick.
    sea_level: f64,
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
        let tuning = Tuning::default();
        Sim {
            front: CellData::new(n),
            back: CellData::new(n),
            plates: PlateData::default(),
            registry: Registry::default(),
            boundaries: BoundaryField::new(n, arcs),
            advection: AdvectionField::new(n),
            mantle: MantleFlow::new(&mesh),
            primordial: Primordial::default(),
            heat: PlanetHeat::new(n, &tuning),
            dyn_topo: vec![0.0; n],
            sea_level: 0.0,
            scratch: Scratch::new(n),
            raw_labels: vec![0; n],
            tuning,
            options: SimOptions::default(),
            initial_mass: 0.0,
            elapsed: 0.0,
            timings: [0.0; PASS_COUNT],
            substeps: 1,
            mesh,
        }
    }

    /// Subdivision level the mesh was built at.
    pub fn level(&self) -> u32 {
        // An icosphere of level L has 10 * 4^L + 2 cells.
        ((self.mesh.cell_count - 2) / 10).ilog(4)
    }

    /// The world as bytes, for [`Sim::from_state`].
    pub fn save_state(&self) -> Vec<u8> {
        let state = State {
            front: Cow::Borrowed(&self.front),
            plates: Cow::Borrowed(&self.plates),
            registry: Cow::Borrowed(&self.registry),
            boundaries: Cow::Borrowed(&self.boundaries),
            advection: Cow::Borrowed(&self.advection),
            mantle: Cow::Borrowed(&self.mantle),
            primordial: Cow::Borrowed(&self.primordial),
            heat: Cow::Borrowed(&self.heat),
            dyn_topo: Cow::Borrowed(&self.dyn_topo),
            sea_level: self.sea_level,
            tuning: Cow::Borrowed(&self.tuning),
            options: Cow::Borrowed(&self.options),
            initial_mass: self.initial_mass,
            elapsed: self.elapsed,
            substeps: self.substeps,
        };
        let mut out = Vec::with_capacity(STATE_HEADER + self.mesh.cell_count * 240);
        out.extend_from_slice(&STATE_MAGIC);
        out.extend_from_slice(&SOURCE_HASH.to_le_bytes());
        out.extend_from_slice(&self.level().to_le_bytes());
        postcard::to_extend(&state, out).expect("a world serialises")
    }

    /// The level a saved world was built at, if `bytes` is a save made by this
    /// build of the simulation.
    pub fn state_level(bytes: &[u8]) -> Result<u32, String> {
        if bytes.len() < STATE_HEADER || bytes[..4] != STATE_MAGIC {
            return Err("not a saved world".into());
        }
        let hash = u64::from_le_bytes(bytes[4..12].try_into().expect("8 bytes"));
        if hash != SOURCE_HASH {
            return Err("saved by a different build of the simulation".into());
        }
        Ok(u32::from_le_bytes(bytes[12..16].try_into().expect("4 bytes")))
    }

    /// Rebuild a world from [`Sim::save_state`].
    pub fn from_state(bytes: &[u8]) -> Result<Sim, String> {
        let level = Sim::state_level(bytes)?;
        let state: State = postcard::from_bytes(&bytes[STATE_HEADER..])
            .map_err(|e| format!("saved world is unreadable: {e}"))?;
        let mut sim = Sim::new(level);
        let n = sim.mesh.cell_count;
        if state.front.elevation.len() != n || state.dyn_topo.len() != n {
            return Err("saved world does not fit its mesh".into());
        }
        sim.back = state.front.as_ref().clone();
        sim.front = state.front.into_owned();
        sim.plates = state.plates.into_owned();
        sim.registry = state.registry.into_owned();
        sim.boundaries = state.boundaries.into_owned();
        sim.advection = state.advection.into_owned();
        sim.mantle = state.mantle.into_owned();
        sim.primordial = state.primordial.into_owned();
        sim.heat = state.heat.into_owned();
        sim.dyn_topo = state.dyn_topo.into_owned();
        sim.sea_level = state.sea_level;
        sim.tuning = state.tuning.into_owned();
        sim.options = state.options.into_owned();
        sim.initial_mass = state.initial_mass;
        sim.elapsed = state.elapsed;
        sim.substeps = state.substeps;
        Ok(sim)
    }

    pub fn reassign_plates(&mut self, cracked: &[u8], init_crust: bool) -> usize {
        let count = assign_plates(&self.mesh, cracked, &mut self.raw_labels);
        let report = self
            .registry
            .relabel(&self.raw_labels, count, &mut self.front.plate_id);
        reconcile(&mut self.plates, &report);
        if init_crust {
            initialise_crust(
                &self.tuning,
                &self.mesh.centres,
                &mut self.front,
                "tectonics",
            );
            self.initial_mass = total_crust_mass(&self.mesh.areas, &self.front);
            self.advection.live_mass = self.initial_mass;
            self.advection.subducted_total = 0.0;
            self.advection.created_total = 0.0;
            self.advection.recycle_debt = 0.0;
            self.advection.continental_growth = 0.0;
            self.advection.continental_share_before = None;
            self.mantle.reset();
            self.dyn_topo.fill(0.0);
            self.primordial.reset();
            self.heat.reset(&self.tuning);
            self.elapsed = 0.0;
            self.sea_level = 0.0;
            self.refresh_surface();
        }
        seed_slabs(&self.mesh, &mut self.front, &self.boundaries, &self.tuning);
        let dt = self.tuning.dt_myr;
        let eff = self.effective_tuning();
        update_plate_motion(
            &self.mesh,
            &self.front,
            &mut self.plates,
            &self.boundaries,
            &self.mantle,
            &mut self.scratch,
            &eff,
            dt,
            &self.options,
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
    /// would carry a plate further than one cell moves them in enough shorter
    /// steps that none does. Only the transport needs the finer steps: the
    /// forces, rifting and the life of the crust act over the whole tick.
    pub fn tick(&mut self) {
        self.timings = [0.0; PASS_COUNT];
        self.drive_plates();
        let steps = self.substeps();
        self.substeps = steps;
        let moved = self.move_plates(steps);
        for x in self.advection.step_tallies_mut() {
            *x = 0.0;
        }
        self.evolve_crust();
        for (x, sum) in self.advection.step_tallies_mut().into_iter().zip(moved) {
            *x += sum;
        }
    }

    /// Steps this tick needs for the plates covering `SUBSTEP_PLATE_SHARE` of
    /// the globe, slowest first, to move at most one cell per step.
    fn substeps(&mut self) -> usize {
        let n = self.mesh.cell_count;
        let width = (4.0 * std::f64::consts::PI / n as f64).sqrt();
        let mut cells: HashMap<i32, usize> = HashMap::new();
        for &p in &self.front.plate_id {
            *cells.entry(p).or_default() += 1;
        }
        let s = &mut self.scratch;
        s.pack_plates(&self.plates);
        s.pack_surface_speeds(&self.mesh, &self.front.plate_id);
        let mut by_speed: Vec<(f64, usize)> = self
            .plates
            .by_id
            .keys()
            .map(|id| (s.plate_speed[*id as usize], *cells.get(id).unwrap_or(&0)))
            .collect();
        by_speed.sort_by(|a, b| a.0.total_cmp(&b.0));
        let covered = (SUBSTEP_PLATE_SHARE * n as f64).ceil() as usize;
        let (mut seen, mut speed) = (0usize, 0.0f64);
        for (v, c) in by_speed {
            speed = v;
            seen += c;
            if seen >= covered {
                break;
            }
        }
        ((speed * self.tuning.dt_myr / width).ceil() as usize).clamp(1, MAX_SUBSTEPS)
    }

    /// Steps the last tick was taken in.
    pub fn last_substeps(&self) -> usize {
        self.substeps
    }

    /// The tuning as the world sees it now, over the mantle as it is now.
    fn effective_tuning(&self) -> Tuning {
        let mut eff = effective_tuning(&self.tuning, self.elapsed, self.heat.excess());
        steer_accretion(&mut eff, &self.options, &self.advection);
        eff
    }

    /// The forces on the plates over the coming tick, and the boundaries and
    /// welds they make.
    fn drive_plates(&mut self) {
        let t = &self.effective_tuning();
        let dt = t.dt_myr;
        let Sim {
            mesh,
            front,
            plates,
            boundaries,
            mantle,
            dyn_topo,
            primordial,
            scratch,
            options,
            timings,
            ..
        } = self;
        let mut mark = now_ms();
        update_slabs(mesh, front, boundaries, scratch, t, dt);
        mantle.update(mesh, front, boundaries, dt, t, options);
        {
            let mantle = &*mantle;
            let balance = mantle.mean_sink();
            par::fill(dyn_topo, |i| {
                dynamic_topography(t, mantle.upper_temp_at(i), mantle.sink_at(i) - balance) as f32
            });
        }
        primordial.add_swell(dyn_topo, t);
        update_plate_motion(
            mesh, front, plates, boundaries, mantle, scratch, t, dt, options,
        );
        lap(timings, &mut mark, 0);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(timings, &mut mark, 1);
        update_sutures(mesh, front, plates, boundaries, dt, scratch, t, options);
        lap(timings, &mut mark, 2);
    }

    /// Carry the crust through the tick in `steps` equal steps, laying fresh
    /// floor in the gaps each opens. Returns the steps' tallies, summed.
    fn move_plates(&mut self, steps: usize) -> [f64; 18] {
        let t = &self.effective_tuning();
        let dt = t.dt_myr / steps as f64;
        let Sim {
            mesh,
            front,
            back,
            plates,
            boundaries,
            advection,
            scratch,
            timings,
            ..
        } = self;
        let mut mark = now_ms();
        let mut moved = [0.0f64; 18];
        for step in 0..steps {
            for x in advection.step_tallies_mut() {
                *x = 0.0;
            }
            advect(
                mesh, front, back, plates, dt, boundaries, advection, scratch, t,
            );
            lay_new_floor(mesh, back, advection, -((step + 1) as f64) * dt, t);
            for (sum, x) in moved.iter_mut().zip(advection.step_tallies_mut()) {
                *sum += *x;
            }
            // Advection writes every field of every cell, so the buffers trade.
            std::mem::swap(front, back);
            if step + 1 < steps {
                classify(mesh, front, plates, boundaries, scratch, t);
            }
        }
        lap(timings, &mut mark, 3);
        moved
    }

    /// Everything that happens to the crust over the tick but its transport,
    /// and the heat the planet loses through it.
    fn evolve_crust(&mut self) {
        let t = &self.effective_tuning();
        let dt = t.dt_myr;
        let excess = self.heat.excess();
        let Sim {
            mesh,
            front,
            plates,
            registry,
            boundaries,
            advection,
            mantle,
            scratch,
            primordial,
            heat,
            dyn_topo,
            sea_level,
            options,
            timings,
            elapsed,
            ..
        } = self;
        let mut mark = now_ms();
        update_plate_identity(mesh, front, plates, registry, scratch, t);
        lap(timings, &mut mark, 4);
        update_rifting(
            mesh, front, plates, mantle, dt, registry, scratch, t, options,
        );
        collapse_passive_margins(mesh, front, plates, registry, scratch, t, options);
        lap(timings, &mut mark, 5);
        recycle_continental_margins(mesh, front, dt, advection, t, options);
        infill_enclosed_basins(mesh, front, dt, advection, scratch, t);
        advection.back_arc_thinned = extend_back_arcs(mesh, front, plates, boundaries, dt, t);
        lap(timings, &mut mark, 6);
        erode_and_deposit(mesh, front, advection, dt, t);
        lap(timings, &mut mark, 9);
        primordial.update(mesh, front, plates, advection, mantle, heat, dt, t);
        apply_crust_lifecycle(mesh, front, dt, advection, scratch, t);
        keep_continents_whole(mesh, front, advection, scratch, t);
        track_continental_growth(mesh, front, dt, advection);
        update_cratons(mesh, front, dt, excess, scratch, t);
        keep_cratons_whole(mesh, front, scratch, t);
        update_lithosphere(front, advection, dt, t);
        heat.update(mesh, front, mantle.cell_temps(), dt, *elapsed, t);
        lap(timings, &mut mark, 6);
        *sea_level = apply_elevation(mesh, front, dyn_topo, boundaries, *sea_level, t);
        lap(timings, &mut mark, 7);
        classify(mesh, front, plates, boundaries, scratch, t);
        lap(timings, &mut mark, 8);
        *elapsed += dt;
    }

    /// Excess of the mantle's temperature over today's, 1 being about 250 K
    /// hotter.
    pub fn mantle_excess(&self) -> f64 {
        self.heat.excess()
    }

    pub fn heat(&self) -> &PlanetHeat {
        &self.heat
    }

    #[cfg(test)]
    pub fn plant_tail(&mut self, centre: crate::math::Vec3) {
        let t = self.effective_tuning();
        self.primordial.plant_tail(&self.mesh, centre, &t);
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

    /// Dynamic topography (km) over each cell, as of the last tick.
    pub fn dyn_topo(&self) -> &[f32] {
        &self.dyn_topo
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

    pub fn fragment_count(&self) -> u64 {
        self.plates.fragments
    }

    pub fn absorbed_fragment_count(&self) -> u64 {
        self.plates.absorbed_fragments
    }

    pub fn weld_merge_count(&self) -> u64 {
        self.plates.weld_merges
    }

    pub fn quiet_merge_count(&self) -> u64 {
        self.plates.quiet_merges
    }

    pub fn craton_merge_count(&self) -> u64 {
        self.plates.craton_merges
    }

    pub fn consumed_count(&self) -> u64 {
        self.plates.consumed
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

    /// Sea level (km above the datum) the water fills the basins to.
    pub fn sea_level(&self) -> f64 {
        self.sea_level
    }

    /// Re-solve sea level and rewrite every elevation from the current crust,
    /// with no transport and no time passing, so a change to the ocean volume
    /// shows at once.
    pub fn refresh_surface(&mut self) {
        let t = self.effective_tuning();
        self.sea_level = apply_elevation(
            &self.mesh,
            &mut self.front,
            &self.dyn_topo,
            &self.boundaries,
            self.sea_level,
            &t,
        );
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
        m.sea_level = self.sea_level;
        m.mantle_excess = self.mantle_excess();
        m.active_plumes = self.active_plumes();
        let h = &self.heat;
        m.surface_heat_tw = h.last_surface_tw;
        m.radiogenic_tw = h.last_radiogenic_tw;
        m.core_heat_tw = h.last_core_tw;
        m.mantle_temp = h.mantle_temp;
        m.core_temp = h.core_temp;
        m.mantle_cooling = -h.rate() * 1000.0;
        let (coolest, hottest, mean) = self.mantle.anomaly_range();
        m.mantle_anomaly_min = coolest;
        m.mantle_anomaly_max = hottest;
        m.mantle_anomaly_mean = mean;
        m.pile_mass = self.mantle.pile_mass();
        m.plume_weight_peak = self.mantle.plume_peak();
        let (low, high) = self
            .dyn_topo
            .iter()
            .fold((0.0f32, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        m.dyn_topo_low = low as f64;
        m.dyn_topo_high = high as f64;
        m
    }

    pub fn summaries(&self) -> Vec<PlateSummary> {
        summaries(&self.mesh, &self.front, &self.plates)
    }

    /// A hash of every cell field, every plate's motion and the mass ledger:
    /// two runs that agree on it made the same world, bit for bit.
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |x: u64| hash = (hash ^ x).wrapping_mul(0x0000_0100_0000_01b3);
        let d = &self.front;
        for field in [
            &d.elevation,
            &d.age,
            &d.density,
            &d.thickness,
            &d.weakness,
            &d.mantle_heat,
            &d.slab,
            &d.keel,
            &d.lithosphere,
            &d.plume_share,
            &d.sediment_share,
        ] {
            field.iter().for_each(|x| eat(x.to_bits() as u64));
        }
        d.plate_id.iter().for_each(|&x| eat(x as u32 as u64));
        d.crust_type
            .iter()
            .chain(&d.rift_side)
            .for_each(|&x| eat(x as u64));
        for (&id, p) in &self.plates.by_id {
            eat(id as u32 as u64);
            p.omega.iter().for_each(|x| eat(x.to_bits()));
        }
        let a = &self.advection;
        let h = &self.heat;
        h.heat_flow
            .iter()
            .chain(self.mantle.cell_temps())
            .chain(self.mantle.cell_piles())
            .for_each(|x| eat(x.to_bits() as u64));
        for x in [
            a.live_mass,
            a.subducted_total,
            a.created_total,
            self.elapsed,
            h.mantle_temp,
            h.core_temp,
        ] {
            eat(x.to_bits());
        }
        hash
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

    /// A world that has run a few ticks, for tests of the surface.
    fn settled_world() -> Sim {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(3, "sea level");
        for _ in 0..3 {
            sim.tick();
        }
        sim
    }

    fn land_share(sim: &Sim) -> f64 {
        let land: f64 = (0..sim.mesh.cell_count)
            .filter(|&i| sim.front.elevation[i] > 0.0)
            .map(|i| sim.mesh.area(i))
            .sum();
        land / (0..sim.mesh.cell_count).map(|i| sim.mesh.area(i)).sum::<f64>()
    }

    #[test]
    fn a_restored_world_ticks_exactly_as_the_original_does() {
        let mut original = Sim::new(4);
        original.crack_great_circles(4, "round trip");
        for _ in 0..30 {
            original.tick();
        }
        let saved = original.save_state();
        let mut restored = Sim::from_state(&saved).expect("restores");
        assert_eq!(restored.level(), 4);
        assert_eq!(restored.save_state(), saved, "restoring changed the state");
        for tick in 0..20 {
            original.tick();
            restored.tick();
            assert_eq!(
                original.front.elevation, restored.front.elevation,
                "elevation diverged {tick} ticks after the restore"
            );
        }
        assert_eq!(
            original.save_state(),
            restored.save_state(),
            "state diverged after the restore"
        );
    }

    #[test]
    fn a_save_from_another_build_is_refused() {
        let saved = settled_world().save_state();
        let mut other_build = saved.clone();
        other_build[4] ^= 1;
        assert!(Sim::from_state(&other_build).is_err());
        assert!(Sim::from_state(&saved[..8]).is_err());
        assert!(Sim::from_state(b"not a world at all").is_err());
        let mut truncated = saved;
        truncated.truncate(truncated.len() / 2);
        assert!(Sim::from_state(&truncated).is_err());
    }

    #[test]
    fn erosion_deposits_everything_it_removes_every_tick() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "erosion ledger");
        let mut eroded = 0.0;
        for _ in 0..80 {
            sim.tick();
            let f = &sim.advection;
            assert!(
                (f.sediment_eroded - f.sediment_deposited).abs() <= 1e-9 * f.sediment_eroded.max(1.0),
                "eroded {} but deposited {}",
                f.sediment_eroded,
                f.sediment_deposited
            );
            eroded += f.sediment_eroded;
        }
        assert!(eroded > 0.0, "nothing eroded");
        assert!(sim.mass_drift().abs() < 1e-6, "mass drifted {}", sim.mass_drift());
    }

    #[test]
    fn more_ocean_water_raises_the_sea_and_leaves_less_land() {
        let mut sim = settled_world();
        let (mut sea, mut land) = (f64::NEG_INFINITY, f64::INFINITY);
        for volume in [1.0, 2.6, 5.0] {
            sim.tuning.ocean_volume = volume;
            sim.refresh_surface();
            assert!(sim.sea_level() > sea, "{volume} km of water: sea at {}", sim.sea_level());
            assert!(land_share(&sim) <= land);
            sea = sim.sea_level();
            land = land_share(&sim);
        }
    }

    #[test]
    fn younger_ocean_floor_raises_the_sea() {
        let mut sim = settled_world();
        sim.refresh_surface();
        let old = sim.sea_level();
        let land = land_share(&sim);
        let d = &mut sim.front;
        for i in 0..d.age.len() {
            if d.crust_type[i] != crate::cell_data::CONTINENTAL {
                d.age[i] *= 0.1;
            }
        }
        crate::lithosphere::settle_lithosphere(d, &sim.tuning);
        sim.refresh_surface();
        assert!(sim.sea_level() > old, "young floor: sea {} against {old}", sim.sea_level());
        assert!(land_share(&sim) <= land);
    }

    #[test]
    fn refreshing_the_surface_changes_nothing_but_elevation() {
        let mut sim = settled_world();
        let (age, thickness, plate) = (
            sim.front.age.clone(),
            sim.front.thickness.clone(),
            sim.front.plate_id.clone(),
        );
        let elapsed = sim.elapsed;
        sim.tuning.ocean_volume = 4.0;
        sim.refresh_surface();
        assert_eq!((sim.front.age, sim.front.thickness, sim.front.plate_id), (age, thickness, plate));
        assert_eq!(sim.elapsed, elapsed);
    }

    /// A level 5 world after `ticks`, run on a pool of `threads`, at a tick
    /// long enough to be split into several steps.
    #[cfg(feature = "parallel")]
    fn run_on(threads: usize, ticks: usize) -> Sim {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut sim = Sim::new(5);
            sim.tuning.dt_myr = 6.0;
            sim.crack_great_circles(5, "threads");
            for _ in 0..ticks {
                sim.tick();
            }
            sim
        })
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn the_world_is_the_same_on_one_thread_and_on_four() {
        let (one, four) = (run_on(1, 40), run_on(4, 40));
        assert!(one.last_substeps() > 1, "every tick was a single step");
        let (a, b) = (&one.front, &four.front);
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        for (name, x, y) in [
            ("elevation", &a.elevation, &b.elevation),
            ("age", &a.age, &b.age),
            ("density", &a.density, &b.density),
            ("thickness", &a.thickness, &b.thickness),
            ("weakness", &a.weakness, &b.weakness),
            ("mantle heat", &a.mantle_heat, &b.mantle_heat),
            ("slab", &a.slab, &b.slab),
            ("keel", &a.keel, &b.keel),
            ("lithosphere", &a.lithosphere, &b.lithosphere),
            ("plume share", &a.plume_share, &b.plume_share),
            ("sediment share", &a.sediment_share, &b.sediment_share),
        ] {
            assert!(bits(x) == bits(y), "{name} differs");
        }
        assert!(a.plate_id == b.plate_id, "plates differ");
        assert!(a.crust_type == b.crust_type, "crust types differ");
        assert!(a.rift_side == b.rift_side, "rift sides differ");
        assert_eq!(one.live_mass().to_bits(), four.live_mass().to_bits());
        assert_eq!(one.mass_drift().to_bits(), four.mass_drift().to_bits());
        assert_eq!(one.fingerprint(), four.fingerprint());
    }

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
        sim.options.hold_continental_area = true;
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

    /// With the target not held, nothing reads it: the continents grow from
    /// the balance of everything else.
    #[test]
    fn the_target_is_not_read_unless_it_is_held() {
        let run = |target: f64| {
            let mut sim = Sim::new(4);
            sim.tuning.primordial_heat = 0.0;
            sim.tuning.continental_area_target = target;
            sim.crack_great_circles(4, "natural");
            for _ in 0..300 {
                sim.tick();
            }
            sim.fingerprint()
        };
        assert_eq!(run(0.05), run(0.6));
    }

    #[test]
    fn dynamic_topography_moves_the_surface_and_nothing_else() {
        let run = |on: bool| {
            let mut sim = Sim::new(4);
            // Erosion reads elevation, so with it the surface moves the crust.
            sim.tuning.erosion_time = 0.0;
            if !on {
                sim.tuning.dyn_topo_gain = 0.0;
                sim.tuning.plume_swell = 0.0;
                sim.tuning.sink_topo_gain = 0.0;
            }
            sim.crack_great_circles(4, "dynamic topography");
            for _ in 0..60 {
                sim.tick();
            }
            sim
        };
        let (on, off) = (run(true), run(false));
        assert!(off.dyn_topo.iter().all(|&x| x == 0.0));
        let limit = on.tuning.dyn_topo_max as f32;
        assert!(
            on.dyn_topo.iter().any(|&x| x.abs() > 0.1),
            "no relief from the mantle"
        );
        assert!(on.dyn_topo.iter().all(|&x| x.abs() <= limit));
        let (a, b) = (&on.front, &off.front);
        assert!(a.elevation != b.elevation, "elevation unmoved");
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        for (name, x, y) in [
            ("age", &a.age, &b.age),
            ("thickness", &a.thickness, &b.thickness),
            ("keel", &a.keel, &b.keel),
            ("lithosphere", &a.lithosphere, &b.lithosphere),
            ("mantle heat", &a.mantle_heat, &b.mantle_heat),
        ] {
            assert!(bits(x) == bits(y), "{name} differs");
        }
        assert!(a.plate_id == b.plate_id && a.crust_type == b.crust_type);
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
        // Recycling wears the coast, which jostles the plates a little.
        sim.tuning.continental_recycling_rate = 0.0;
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
                d.mantle_heat[i] = heat;
            }
        }
        crate::lithosphere::settle_lithosphere(d, t);
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
        d.density[pocket] = sim.tuning.oceanic_density as f32;
        d.lithosphere[pocket] = crate::lithosphere::ocean_lithosphere(&sim.tuning, 150.0) as f32;
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
        let any = if a[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
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
        crate::lithosphere::settle_lithosphere(d, &sim.tuning);
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
        let t = Tuning::default();
        for excess in [0.0, 1.0] {
            let at = |elapsed: f64| effective_tuning(&t, elapsed, excess).ridge_weight;
            assert_eq!(at(0.0), 0.0);
            assert!((at(0.5 * t.seafloor_flattening_age) - 0.5 * t.ridge_weight).abs() < 1e-12);
            assert_eq!(at(3.0 * t.seafloor_flattening_age), t.ridge_weight);
        }
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
                d.density[i] = t.oceanic_density as f32;
            }
        }
        let coast: Vec<usize> = (0..n)
            .filter(|&i| {
                d.plate_id[i] == continent
                    && sim
                        .mesh
                        .nbrs(i)
                        .any(|k| d.plate_id[sim.mesh.nbr[k] as usize] != continent)
            })
            .collect();
        for &i in &coast {
            d.thickness[i] = t.oceanic_thickness as f32;
        }
        crate::lithosphere::settle_lithosphere(d, &t);
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
        let neutral =
            crate::lithosphere::neutral_age(&sim.tuning, sim.tuning.oceanic_thickness) as f32;
        for &i in &coast {
            assert_eq!(
                sim.front.crust_type[i], OCEANIC,
                "thinned coast at {i} still continent"
            );
            assert_eq!(
                sim.front.age[i], neutral,
                "reverted crust at {i} is {} Myr old",
                sim.front.age[i]
            );
            let b = crate::elevation::column_buoyancy(&sim.tuning, &sim.front, i);
            assert!(b.abs() < 1e-3, "reverted crust at {i} has buoyancy {b:.3}");
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
                    && mesh
                        .nbrs(i)
                        .any(|k| d.plate_id[mesh.nbr[k] as usize] == continent)
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
        assert!(
            mean < 0.1 * noise,
            "neighbours differ by {mean:.1} Myr on average"
        );
        let ages: Vec<f32> = d.age.clone();
        let span = ages.iter().copied().fold(f32::MIN, f32::max)
            - ages.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            span as f64 > 0.9 * sim.tuning.max_oceanic_age,
            "ages span only {span:.0} Myr"
        );
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
        let any = if a[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
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
                assert_eq!(
                    sim.front.weakness[i], 0.0,
                    "new sea floor at {i} came out weak"
                );
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
        let any = if a[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
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
        crate::lithosphere::settle_lithosphere(d, &sim.tuning);
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
