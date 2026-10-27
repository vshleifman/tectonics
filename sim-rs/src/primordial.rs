//! Mantle plumes, and the primordial era (`primordial_heat` > 0, on by
//! default): a young, hot mantle that makes continent over plumes rather than
//! at arcs, cooling into the modern regime, where plumes still rise but build
//! only ocean plateaus and hotspots. Cratonic keels grow only while the
//! primordial era lasts (`craton.rs`).
//!
//! Early Archean mantle ran some 200-300 C hotter. Plume heads built thick
//! basaltic plateaus whose hydrated roots melted into the first continental
//! crust (the tonalite-trondhjemite-granodiorite suites), leaving a depleted,
//! buoyant mantle residue beneath: a cratonic keel from the start, not one
//! grown over hundreds of Myr. Meanwhile hot, weak, buoyant slabs made arcs
//! poor builders of continent. As the mantle cooled, plumes waned and
//! subduction took over, by somewhere around 3.2-2.5 Ga.
//!
//! The mantle starts `primordial_heat` hotter than today's and cools as the
//! planet's heat budget lets it (see `heat.rs`). Plumes are fed by the heat
//! the core passes up, which wanes far more slowly than the mantle cools, so
//! they rise at `plume_rate` per Myr scaled by the core's heat flow against
//! today's, in every era. They rise at the edges of the slab piles in the
//! lower mantle (see `mantle.rs`), and each thickens a round patch of the
//! crust above it over `plume_duration`, riding with the plate it rises under
//! so the plateau stays round rather than smearing into a track. How thick
//! depends on how hot the mantle is, rising faster than its excess: from
//! `plume_thickness` at full excess, enough to melt into continent, down to
//! `plume_thickness_cold` in a mantle as cool as today's, an ocean plateau
//! that stays ocean. A plume's tail stays in the mantle instead, carried by
//! the flow beneath it (`plume_drift`), while the plate moves over it.
//! Arc accretion is scaled down by `hot_arc_suppression` times the excess,
//! and the mafic part of ocean floor thickened anywhere but over a plume drips
//! back into the mantle, `hot_drip` times the excess of its extra thickness
//! each step (plume-made crust, by the `plume_share` tracer, and sediment, by
//! `sediment_share`, are too buoyant to
//! founder), so
//! early continent comes from plumes rather than from crust piling up at
//! convergent margins.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData, mixed_share};
use crate::heat::{PlanetHeat, modern_core_tw};
use crate::mantle::MantleFlow;
use crate::math::{Vec3, clamp01, normalise, read3, rotate};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::rng::Rng;
use crate::tuning::Tuning;
use serde::{Deserialize, Serialize};

/// Share of a plume's radius over which the plateau is at full thickness,
/// tapering to ordinary ocean floor at its edge.
const PLATEAU_TOP: f64 = 0.6;

#[derive(Clone, Serialize, Deserialize)]
struct Plume {
    /// The head, which rides with the plate it builds its plateau on.
    centre: Vec3,
    elapsed: f64,
    /// The tail, born where the head was and carried by the mantle flow
    /// (`plume_drift`), not by the plate. Its footprint is a set of mesh cells
    /// worked out where the tail stood when it was last laid: cells with their
    /// share of the tail's thickness, and of its swell.
    tail_elapsed: f64,
    tail_centre: Vec3,
    /// Where `tail_cells` and `swell_cells` were laid out, and the cell
    /// nearest it, whose mantle flow carries the tail.
    laid_at: Vec3,
    tail_cell: usize,
    tail_cells: Vec<(u32, f32)>,
    swell_cells: Vec<(u32, f32)>,
}

impl Plume {
    fn head_live(&self, duration: f64) -> bool {
        self.elapsed < duration
    }

    /// Work out the tail's footprint and swell where it stands now.
    fn lay_tail(&mut self, mesh: &Mesh, t: &Tuning) {
        let tail_radius = t.plume_tail_radius.max(1e-6);
        let swell_radius = t.plume_swell_radius.max(1e-6);
        let (tail_cells, nearest) = footprint(mesh, self.tail_centre, tail_radius, tail_profile);
        self.tail_cells = tail_cells;
        self.tail_cell = nearest;
        self.swell_cells = footprint(mesh, self.tail_centre, swell_radius, |r| {
            0.5 * (1.0 + (std::f64::consts::PI * r.min(1.0)).cos())
        })
        .0;
        self.laid_at = self.tail_centre;
    }

    /// Carry the tail `plume_drift` of the way with the mantle flow beneath
    /// it, and lay its footprint again once it has moved far enough to matter.
    fn drift(&mut self, mesh: &Mesh, mantle: &MantleFlow, dt_myr: f64, t: &Tuning) {
        if t.plume_drift <= 0.0 {
            return;
        }
        let wind = mantle.wind_at(self.tail_cell);
        let step = t.plume_drift * dt_myr;
        let moved = [
            self.tail_centre[0] + wind[0] * step,
            self.tail_centre[1] + wind[1] * step,
            self.tail_centre[2] + wind[2] * step,
        ];
        self.tail_centre = normalise(moved);
        let off = dot(self.tail_centre, self.laid_at).clamp(-1.0, 1.0).acos();
        if off >= REPLANT * t.plume_tail_radius.max(1e-6) {
            self.lay_tail(mesh, t);
        }
    }
}

/// Share of the tail's radius it may drift from where its footprint was laid
/// before the footprint is laid again.
const REPLANT: f64 = 0.25;

/// Cells within `radius` of `centre`, each with `profile(r)` for `r` the
/// distance over `radius`, and the cell nearest the centre, which is always in.
fn footprint(
    mesh: &Mesh,
    centre: Vec3,
    radius: f64,
    profile: impl Fn(f64) -> f64,
) -> (Vec<(u32, f32)>, usize) {
    let reach = radius.cos();
    let mut nearest = (0usize, f64::NEG_INFINITY);
    let mut cells = Vec::new();
    for i in 0..mesh.cell_count {
        let cos = dot(read3(&mesh.centres, i), centre);
        if cos > nearest.1 {
            nearest = (i, cos);
        }
        if cos >= reach {
            let r = cos.clamp(-1.0, 1.0).acos() / radius;
            cells.push((i as u32, profile(r) as f32));
        }
    }
    if !cells.iter().any(|&(i, _)| i as usize == nearest.0) {
        cells.push((nearest.0 as u32, profile(0.0) as f32));
    }
    (cells, nearest.0)
}

/// The tail's thickness profile over its footprint.
fn tail_profile(r: f64) -> f64 {
    if r <= PLATEAU_TOP {
        1.0
    } else {
        ((1.0 - r) / (1.0 - PLATEAU_TOP)).max(0.0)
    }
}

/// Thicken cell `i` toward `target` by `share` of the shortfall, booking the
/// crust as plume-made. Returns the new thickness if any was added.
fn raise(
    mesh: &Mesh,
    data: &mut CellData,
    field: &mut AdvectionField,
    i: usize,
    target: f64,
    share: f64,
    created: &mut f64,
) -> Option<f64> {
    let h = data.thickness[i] as f64;
    let gain = (target - h).max(0.0) * share;
    if gain <= 0.0 {
        return None;
    }
    let before = data.thickness[i];
    data.thickness[i] = (h + gain) as f32;
    let added = (data.thickness[i] - before) as f64;
    data.plume_share[i] = mixed_share(data.plume_share[i], before as f64, added, 1.0);
    field.plume_in[i] += added as f32;
    *created += added * mesh.area(i);
    Some(h + gain)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Primordial {
    plumes: Vec<Plume>,
    rng: Rng,
}

impl Default for Primordial {
    fn default() -> Self {
        Primordial {
            plumes: Vec::new(),
            rng: Rng::new("plumes"),
        }
    }
}

impl Primordial {
    pub fn reset(&mut self) {
        *self = Primordial::default();
    }

    /// Plumes with a head or a tail still live.
    pub fn active_plumes(&self) -> usize {
        self.plumes.len()
    }

    /// Lift `dyn_topo` over each live tail by `plume_swell`, tapering to
    /// nothing at `plume_swell_radius`, then clamp to `dyn_topo_max` as the
    /// rest of it is. The swell stays where the tail stands while the plate
    /// moves off it.
    pub fn add_swell(&self, dyn_topo: &mut [f32], t: &Tuning) {
        if t.plume_swell == 0.0 || self.plumes.is_empty() {
            return;
        }
        let limit = t.dyn_topo_max.max(0.0) as f32;
        for plume in &self.plumes {
            for &(i, w) in &plume.swell_cells {
                let x = &mut dyn_topo[i as usize];
                *x = (*x + (t.plume_swell as f32) * w).clamp(-limit, limit);
            }
        }
    }

    #[cfg(test)]
    pub fn plant_tail(&mut self, mesh: &Mesh, centre: Vec3, t: &Tuning) {
        self.plumes.push(Self::born(mesh, centre, t));
    }

    fn born(mesh: &Mesh, centre: Vec3, t: &Tuning) -> Plume {
        let mut plume = Plume {
            centre,
            elapsed: 0.0,
            tail_elapsed: 0.0,
            tail_centre: centre,
            laid_at: centre,
            tail_cell: 0,
            tail_cells: Vec::new(),
            swell_cells: Vec::new(),
        };
        plume.lay_tail(mesh, t);
        plume
    }

    /// Raise new plumes where the lower mantle sends them up, carry the live
    /// ones with their plates, and thicken the crust over them as hot as the
    /// mantle now is. New crust goes into the ledger as created.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        mesh: &Mesh,
        data: &mut CellData,
        plates: &PlateData,
        field: &mut AdvectionField,
        mantle: &MantleFlow,
        heat: &PlanetHeat,
        dt_myr: f64,
        t: &Tuning,
    ) {
        field.plume_in.fill(0.0);
        field.plume_created_this_step = 0.0;
        field.dripped_this_step = 0.0;
        let excess = heat.excess();
        let modern = modern_core_tw(t);
        let core = if modern > 0.0 {
            heat.core_tw(t) / modern
        } else {
            0.0
        };
        let expected = t.plume_rate.max(0.0) * core * dt_myr;
        if expected <= 0.0 && excess <= 0.0 && self.plumes.is_empty() {
            return;
        }

        let mut count = expected.floor() as usize;
        if self.rng.next_f64() < expected.fract() {
            count += 1;
        }
        for _ in 0..count {
            let site = mantle.pick_plume_site(mesh, self.rng.next_f64());
            self.plumes.push(Self::born(mesh, site, t));
        }

        let n = mesh.cell_count;
        let radius = t.plume_radius.max(1e-6);
        let reach = radius.cos();
        let duration = t.plume_duration.max(dt_myr);
        // Both how deep a plume head starts to melt and how much of it melts
        // grow with its excess heat, so its melt grows faster than the excess.
        // Linear, mid-era plumes make so much keeled continent the plates
        // weld into one; squared, too few cratons form.
        let melt = clamp01(excess).powf(1.5);
        let peak = t.plume_thickness_cold + (t.plume_thickness - t.plume_thickness_cold) * melt;
        let mut created = 0.0;
        // A tail never thickens floor to continent by itself.
        let tail_peak = t
            .plume_tail_thickness
            .min(t.continental_thickness_threshold - 1.0 - t.ridge_crust_thickness)
            .max(0.0);
        let tail_duration = t.plume_tail_duration.max(0.0);
        // A tail builds at the head's pace, so a plate sliding over it is
        // thickened for as long as it takes a head to build its plateau.
        let tail_share = (dt_myr / duration).min(1.0);
        for plume in &mut self.plumes {
            if plume.head_live(duration) {
                // Ride with the plate the plateau is building on.
                let under = (0..n)
                    .max_by(|&a, &b| {
                        let pa = read3(&mesh.centres, a);
                        let pb = read3(&mesh.centres, b);
                        dot(pa, plume.centre).total_cmp(&dot(pb, plume.centre))
                    })
                    .unwrap_or(0);
                if let Some(p) = plates.by_id.get(&data.plate_id[under]) {
                    plume.centre = normalise(rotate(plume.centre, p.omega, dt_myr));
                }
                let remaining = (duration - plume.elapsed).max(dt_myr);
                let share = (dt_myr / remaining).min(1.0);
                for i in 0..n {
                    let cos = dot(read3(&mesh.centres, i), plume.centre);
                    // A cell rifted open this step is about to be laid fresh.
                    if cos < reach || field.rift_cell[i] != 0 {
                        continue;
                    }
                    let r = cos.clamp(-1.0, 1.0).acos() / radius;
                    let profile = tail_profile(r);
                    let base = t.ridge_crust_thickness;
                    let target = base + (peak - base) * profile;
                    let Some(grown) = raise(mesh, data, field, i, target, share, &mut created)
                    else {
                        continue;
                    };
                    // The melt residue is the keel: it forms with the crust.
                    if grown >= t.continental_thickness_threshold {
                        let keel = (t.plume_keel * profile) as f32;
                        data.keel[i] = data.keel[i].max(keel);
                    }
                }
                plume.elapsed += dt_myr;
            }
            // The tail stays in the mantle, which carries it slowly, and the
            // plate moves over it.
            if plume.tail_elapsed < tail_duration {
                plume.drift(mesh, mantle, dt_myr, t);
                for &(i, profile) in &plume.tail_cells {
                    let i = i as usize;
                    if field.rift_cell[i] != 0 {
                        continue;
                    }
                    let target = t.ridge_crust_thickness + tail_peak * profile as f64;
                    raise(mesh, data, field, i, target, tail_share, &mut created);
                }
                plume.tail_elapsed += dt_myr;
            }
        }
        self.plumes
            .retain(|p| p.head_live(duration) || p.tail_elapsed < tail_duration);
        field.plume_created_this_step = created;
        field.created_total += created;

        // Drip: in a mantle this hot, mafic crust thickened anywhere but over
        // a plume turns to eclogite and founders before it can melt into
        // continent.
        let drip = clamp01(t.hot_drip * excess);
        let mut dripped = 0.0;
        if drip > 0.0 {
            for i in 0..n {
                if data.crust_type[i] == CONTINENTAL
                    || field.plume_in[i] > 0.0
                    || field.rift_cell[i] != 0
                {
                    continue;
                }
                let before = data.thickness[i];
                let excess_h = before as f64 - t.ridge_crust_thickness;
                if excess_h <= 0.0 {
                    continue;
                }
                // Only the mafic part founders: felsic crust a plume made is
                // too buoyant to sink, wherever the grid has smeared it.
                // Nor does sediment, however it got there.
                let share = data.plume_share[i] as f64;
                let silt = data.sediment_share[i] as f64;
                let mafic = (1.0 - share - silt).max(0.0);
                let removed = (drip * excess_h * mafic).min(before as f64 * mafic);
                data.thickness[i] = (before as f64 - removed) as f32;
                let after = data.thickness[i] as f64;
                if after > 0.0 {
                    data.plume_share[i] = (share * before as f64 / after).clamp(0.0, 1.0) as f32;
                    data.sediment_share[i] = (silt * before as f64 / after).clamp(0.0, 1.0) as f32;
                }
                dripped += (before as f64 - after) * mesh.area(i);
            }
        }
        field.dripped_this_step = dripped;
        field.subducted_total += dripped;
    }
}

#[inline]
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::{Primordial, dot};
    use crate::cell_data::CONTINENTAL;
    use crate::mantle::MantleFlow;
    use crate::math::{normalise, read3};
    use crate::mesh::Mesh;
    use crate::tuning::Tuning;
    use crate::world::Sim;

    #[test]
    fn early_continent_comes_from_plumes() {
        let mut sim = Sim::new(5);
        sim.tuning.primordial_heat = 1.0;
        sim.tuning.plume_rate = 0.2;
        sim.crack_great_circles(4, "primordial");
        let (mut plume, mut other) = (0.0, 0.0);
        for _ in 0..50 {
            sim.tick();
            let a = sim.advection();
            plume += a.continentalised_by_plume;
            other += a.continentalised_by_accretion
                + a.continentalised_by_piling
                + a.continentalised_by_spreading;
        }
        assert!(plume > 2.0 * other, "plumes {plume}, everything else {other}");
        assert!(sim.mass_drift().abs() < 1e-4, "drift {}", sim.mass_drift());
    }

    #[test]
    fn a_hot_mantle_raises_round_keeled_continent_and_keeps_the_ledger() {
        let mut sim = Sim::new(5);
        sim.tuning.primordial_heat = 1.0;
        sim.tuning.plume_rate = 0.2;
        sim.crack_great_circles(4, "primordial");
        for _ in 0..60 {
            sim.tick();
        }
        let d = sim.front();
        let keeled = (0..sim.mesh.cell_count)
            .filter(|&i| d.crust_type[i] == CONTINENTAL && d.keel[i] > 0.5)
            .count();
        assert!(keeled > 0, "no plume crust became keeled continent");
        assert!(sim.mass_drift().abs() < 1e-4, "drift {}", sim.mass_drift());
    }

    #[test]
    fn the_tracer_never_holds_more_plume_crust_than_plumes_made() {
        let mut sim = Sim::new(5);
        sim.tuning.primordial_heat = 1.0;
        sim.tuning.plume_rate = 0.2;
        sim.crack_great_circles(4, "primordial");
        let mut made = 0.0;
        for _ in 0..100 {
            sim.tick();
            made += sim.advection().plume_created_this_step;
            let d = sim.front();
            let traced: f64 = (0..sim.mesh.cell_count)
                .map(|i| sim.mesh.area(i) * d.thickness[i] as f64 * d.plume_share[i] as f64)
                .sum();
            assert!(traced <= made * 1.001 + 1e-9, "traced {traced} of {made} made");
        }
        let d = sim.front();
        let traced: f64 = (0..sim.mesh.cell_count)
            .map(|i| sim.mesh.area(i) * d.thickness[i] as f64 * d.plume_share[i] as f64)
            .sum();
        assert!(traced > 0.5 * made, "only {traced} of {made} plume crust left after 200 Myr");
    }

    #[test]
    fn a_cool_mantle_raises_ocean_plateaus_not_continent() {
        let mut sim = Sim::new(4);
        sim.tuning.primordial_heat = 0.0;
        sim.tuning.start_age_ga = 0.0;
        sim.tuning.plume_rate = 0.5;
        sim.crack_great_circles(4, "primordial");
        let (mut made, mut continent, mut risen) = (0.0, 0.0, 0usize);
        for _ in 0..40 {
            sim.tick();
            assert!(sim.mantle_excess() <= 0.0, "today's mantle warmed");
            made += sim.advection().plume_created_this_step;
            continent += sim.advection().continentalised_by_plume;
            risen = risen.max(sim.active_plumes());
            let d = sim.front();
            for i in 0..sim.mesh.cell_count {
                if sim.advection().plume_in[i] > 0.0 && d.crust_type[i] != CONTINENTAL {
                    assert!(
                        (d.thickness[i] as f64) < sim.tuning.continental_thickness_threshold,
                        "a plateau reached {:.1} km",
                        d.thickness[i]
                    );
                }
            }
        }
        assert!(risen > 0 && made > 0.0, "a cool mantle raised no plumes");
        assert_eq!(continent, 0.0, "plumes made continent in a cool mantle");
    }

    #[test]
    fn plumes_rise_with_the_core_s_heat() {
        let mut sim = Sim::new(4);
        sim.tuning.primordial_heat = 0.0;
        sim.tuning.start_age_ga = 0.0;
        sim.tuning.plume_rate = 0.5;
        sim.tuning.core_coupling = 0.0;
        sim.crack_great_circles(4, "primordial");
        for _ in 0..30 {
            sim.tick();
            assert_eq!(sim.active_plumes(), 0, "plumes rose off a core passing no heat");
        }
    }

    #[test]
    fn a_tail_footprint_is_fixed_and_swell_stays_within_the_clamp() {
        let mut sim = Sim::new(5);
        sim.tuning.primordial_heat = 0.0;
        sim.tuning.start_age_ga = 0.0;
        sim.tuning.plume_rate = 0.0;
        sim.crack_great_circles(4, "primordial");
        let at = [0.0, 0.0, 1.0];
        sim.plant_tail(at);
        sim.tick();
        let t = sim.tuning.clone();
        let swell: Vec<f32> = sim.dyn_topo().to_vec();
        let zs: Vec<f64> = (0..sim.mesh.cell_count)
            .map(|i| sim.mesh.centres[i * 3 + 2] as f64)
            .collect();
        let cos = |i: usize| zs[i];
        let limit = t.dyn_topo_max as f32;
        for i in 0..sim.mesh.cell_count {
            assert!(swell[i].abs() <= limit);
            if cos(i).clamp(-1.0, 1.0).acos() > t.plume_swell_radius + 0.1 {
                assert!(swell[i].abs() < 0.2, "swell {} far off at cell {i}", swell[i]);
            }
        }
        let peak = (0..sim.mesh.cell_count).max_by(|&a, &b| swell[a].total_cmp(&swell[b])).unwrap();
        assert!(cos(peak).clamp(-1.0, 1.0).acos() < t.plume_swell_radius);
        // Ten Myr on, the swell has not moved.
        for _ in 0..10 {
            sim.tick();
        }
        let after = (0..sim.mesh.cell_count)
            .max_by(|&a, &b| sim.dyn_topo()[a].total_cmp(&sim.dyn_topo()[b]))
            .unwrap();
        assert!(cos(after).clamp(-1.0, 1.0).acos() < t.plume_swell_radius);
    }

    #[test]
    fn a_tail_thickens_the_floor_but_never_makes_it_continental() {
        let mut sim = Sim::new(5);
        sim.tuning.primordial_heat = 0.0;
        sim.tuning.start_age_ga = 0.0;
        sim.tuning.plume_rate = 0.0;
        sim.tuning.plume_tail_thickness = 40.0;
        sim.crack_great_circles(4, "primordial");
        sim.plant_tail([0.0, 0.0, 1.0]);
        let mut made = 0.0;
        for _ in 0..60 {
            sim.tick();
            made += sim.advection().plume_created_this_step;
            let d = sim.front();
            for i in 0..sim.mesh.cell_count {
                if d.plume_share[i] > 0.9 && d.crust_type[i] != CONTINENTAL {
                    assert!(
                        (d.thickness[i] as f64) < sim.tuning.continental_thickness_threshold,
                        "a tail reached {:.1} km",
                        d.thickness[i]
                    );
                }
            }
        }
        assert!(made > 0.0, "the tail built nothing");
        assert!(sim.mass_drift().abs() < 1e-4, "drift {}", sim.mass_drift());
    }

    #[test]
    fn a_tail_drifts_with_the_mantle_flow_and_its_footprint_follows() {
        let mesh = Mesh::icosphere(5);
        let mut mantle = MantleFlow::new(&mesh);
        let (sites, _) = mantle.sinks();
        let sink = sites[0];
        mantle.set_sink_for_test(0, 0.5);
        // Start a quarter turn from the downwelling, which draws it in.
        let start = normalise([sink[1] + 1.0e-3, -sink[0] + 1.0e-3, sink[2] + 1.0]);
        let mut t = Tuning::default();
        let mut fixed = Primordial::born(&mesh, start, &t);
        t.plume_drift = 0.5;
        let mut drifting = Primordial::born(&mesh, start, &t);
        let before = fixed.tail_cells.clone();
        for _ in 0..40 {
            fixed.drift(&mesh, &mantle, 2.0, &Tuning { plume_drift: 0.0, ..t.clone() });
            drifting.drift(&mesh, &mantle, 2.0, &t);
        }
        assert_eq!(fixed.tail_cells, before, "a tail with no drift moved");
        let gap = |p: [f64; 3]| dot(p, sink).clamp(-1.0, 1.0).acos();
        assert!(
            gap(drifting.tail_centre) < gap(start) - 0.05,
            "the tail did not move toward the downwelling: {:.3} from {:.3}",
            gap(drifting.tail_centre),
            gap(start)
        );
        // Every cell of the footprint lies within the radius of the tail, give
        // or take how far it may drift before the footprint is laid again.
        let reach = (1.0 + super::REPLANT) * t.plume_tail_radius;
        assert!(
            drifting
                .tail_cells
                .iter()
                .all(|&(i, _)| dot(read3(&mesh.centres, i as usize), drifting.tail_centre)
                    >= (reach + 1e-9).cos()),
            "the footprint was left behind"
        );
        assert_ne!(drifting.tail_cell, fixed.tail_cell, "the footprint never moved");
        // The swell goes with it.
        assert!(drifting.swell_cells.iter().all(|&(i, _)| {
            dot(read3(&mesh.centres, i as usize), drifting.tail_centre)
                >= (t.plume_swell_radius + 0.5 * t.plume_tail_radius).cos()
        }));
    }
}
