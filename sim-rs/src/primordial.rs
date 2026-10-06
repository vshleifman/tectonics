//! The primordial era (opt-in, `primordial_heat` > 0): a young, hot mantle
//! that makes continent over plumes rather than at arcs, cooling into the
//! modern regime.
//!
//! Early Archean mantle ran some 200-300 C hotter. Plume heads built thick
//! basaltic plateaus whose hydrated roots melted into the first continental
//! crust (the tonalite-trondhjemite-granodiorite suites), leaving a depleted,
//! buoyant mantle residue beneath: a cratonic keel from the start, not one
//! grown over hundreds of Myr. Meanwhile hot, weak, buoyant slabs made arcs
//! poor builders of continent. As the mantle cooled, plumes waned and
//! subduction took over, by somewhere around 3.2-2.5 Ga.
//!
//! The mantle's excess temperature decays as `primordial_heat` times
//! `exp(-t / primordial_cooling_time)`. Plumes rise at `plume_rate` times that
//! per Myr, at random, and each thickens a round patch of the crust above it
//! toward `plume_thickness` over `plume_duration`, riding with the plate it
//! rises under so the plateau stays round rather than smearing into a track.
//! Arc accretion is scaled down by `hot_arc_suppression` times the excess,
//! and the mafic part of ocean floor thickened anywhere but over a plume drips
//! back into the mantle, `hot_drip` times the excess of its extra thickness
//! each step (plume-made crust, by the `plume_share` tracer, is too buoyant to
//! founder), so
//! early continent comes from plumes rather than from crust piling up at
//! convergent margins.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData, mixed_share};
use crate::math::{Vec3, clamp01, normalise, read3, rotate};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::rng::Rng;
use crate::tuning::Tuning;

/// Share of a plume's radius over which the plateau is at full thickness,
/// tapering to ordinary ocean floor at its edge.
const PLATEAU_TOP: f64 = 0.6;

struct Plume {
    centre: Vec3,
    elapsed: f64,
}

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

/// Excess mantle temperature at `elapsed` Myr, in units of `primordial_heat`'s
/// scale: 0 with the era off.
pub fn mantle_excess(t: &Tuning, elapsed: f64) -> f64 {
    if t.primordial_heat <= 0.0 {
        return 0.0;
    }
    t.primordial_heat * (-elapsed / t.primordial_cooling_time.max(1e-6)).exp()
}

/// The tuning as the world sees it `elapsed` Myr in: a hot mantle's arcs
/// build less continent, and ridge push comes in over the first
/// `seafloor_flattening_age`. The sea floor a world starts with has ages
/// that no ridge laid down, so a push down their slopes would steer the
/// plates by the starting pattern until floor from real ridges replaces it.
pub fn effective_tuning(t: &Tuning, elapsed: f64) -> Tuning {
    let mut eff = t.clone();
    let excess = mantle_excess(t, elapsed);
    if excess > 0.0 {
        eff.accretion_fraction *= clamp01(1.0 - t.hot_arc_suppression * excess);
    }
    eff.ridge_weight *= clamp01(elapsed / t.seafloor_flattening_age.max(1e-6));
    eff
}

impl Primordial {
    pub fn reset(&mut self) {
        *self = Primordial::default();
    }

    pub fn active_plumes(&self) -> usize {
        self.plumes.len()
    }

    /// Raise new plumes, carry the live ones with their plates, and thicken
    /// the crust over them. New crust goes into the ledger as created.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        mesh: &Mesh,
        data: &mut CellData,
        plates: &PlateData,
        field: &mut AdvectionField,
        dt_myr: f64,
        elapsed: f64,
        t: &Tuning,
    ) {
        field.plume_in.fill(0.0);
        field.plume_created_this_step = 0.0;
        let excess = mantle_excess(t, elapsed);
        if excess <= 0.0 && self.plumes.is_empty() {
            return;
        }

        let expected = t.plume_rate * excess * dt_myr;
        let mut count = expected.floor() as usize;
        if self.rng.next_f64() < expected.fract() {
            count += 1;
        }
        for _ in 0..count {
            let z = 2.0 * self.rng.next_f64() - 1.0;
            let phi = 2.0 * std::f64::consts::PI * self.rng.next_f64();
            let r = (1.0 - z * z).max(0.0).sqrt();
            self.plumes.push(Plume {
                centre: [r * phi.cos(), r * phi.sin(), z],
                elapsed: 0.0,
            });
        }

        let n = mesh.cell_count;
        let radius = t.plume_radius.max(1e-6);
        let reach = radius.cos();
        let duration = t.plume_duration.max(dt_myr);
        let mut created = 0.0;
        for plume in &mut self.plumes {
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
                let profile = if r <= PLATEAU_TOP {
                    1.0
                } else {
                    ((1.0 - r) / (1.0 - PLATEAU_TOP)).max(0.0)
                };
                let target =
                    t.oceanic_thickness + (t.plume_thickness - t.oceanic_thickness) * profile;
                let h = data.thickness[i] as f64;
                let gain = (target - h).max(0.0) * share;
                if gain <= 0.0 {
                    continue;
                }
                let before = data.thickness[i];
                data.thickness[i] = (h + gain) as f32;
                let added = (data.thickness[i] - before) as f64;
                data.plume_share[i] = mixed_share(data.plume_share[i], before as f64, added, 1.0);
                field.plume_in[i] += added as f32;
                created += added * mesh.area(i);
                // The melt residue is the keel: it forms with the crust.
                if h + gain >= t.continental_thickness_threshold {
                    let keel = (t.plume_keel * profile) as f32;
                    data.keel[i] = data.keel[i].max(keel);
                }
            }
            plume.elapsed += dt_myr;
        }
        self.plumes.retain(|p| p.elapsed < duration);
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
                let excess_h = before as f64 - t.oceanic_thickness;
                if excess_h <= 0.0 {
                    continue;
                }
                // Only the mafic part founders: felsic crust a plume made is
                // too buoyant to sink, wherever the grid has smeared it.
                let share = data.plume_share[i] as f64;
                let removed = (drip * excess_h * (1.0 - share)).min(before as f64 * (1.0 - share));
                data.thickness[i] = (before as f64 - removed) as f32;
                let after = data.thickness[i] as f64;
                if after > 0.0 {
                    data.plume_share[i] = (share * before as f64 / after).clamp(0.0, 1.0) as f32;
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
    use crate::cell_data::CONTINENTAL;
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
    fn a_cold_mantle_raises_nothing() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "primordial");
        for _ in 0..30 {
            sim.tick();
        }
        assert_eq!(sim.advection().plume_created_this_step, 0.0);
    }
}
