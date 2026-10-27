//! The planet's heat budget: what sets the mantle's temperature, and through
//! it how hot a world the plates are moving over.
//!
//! The mantle gains heat from the decay of uranium, thorium and potassium and
//! from the core below it, and loses it through the surface. Almost all of
//! that loss is through the ocean floor, as each strip of it cools from the
//! ridge: young floor loses heat fast, and old floor, cooled through, hardly
//! at all (heat flow falls as the inverse square root of age, as a cooling
//! half-space does, until the plate flattens out at ~100 Myr). Continents
//! pass less, and less still where a cratonic keel or a blanket of trapped
//! heat holds it in. So the map controls the cooling: a world of fast plates
//! and young sea floor sheds its heat, and one of slow plates and old floor,
//! or with a supercontinent sitting on its mantle, keeps it.
//!
//! The excess over the modern mantle is in the units the primordial era has
//! always used, 1 being about 250 K hotter. A hotter mantle is less viscous,
//! so it drags less on the plates and they answer the forces on them faster.
//! It melts more beneath a ridge, so it lays thicker ocean crust and the
//! ridge itself stands higher. It also raises the plumes, holds back arcs,
//! drips thickened floor and grows cratonic keels (see `primordial.rs` and
//! `craton.rs`).
//!
//! Earth today loses ~46 TW through its surface, against ~20 TW of
//! radioactive heat and ~10 TW from the core, so its mantle cools by some
//! 50-100 K per Gyr.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::clamp01;
use crate::mesh::Mesh;
use crate::par;
use crate::tuning::Tuning;
use serde::{Deserialize, Serialize};

/// Surface area of Earth (m^2), which the unit sphere stands in for.
const EARTH_AREA_M2: f64 = 5.10e14;
/// Joules in a terawatt sustained for a million years.
const TW_MYR_J: f64 = 1e12 * 3.156e13;
/// Heat capacity of the mantle (J/K): 4.0e24 kg at ~1250 J/kg/K.
const MANTLE_HEAT_CAPACITY: f64 = 1.0e28;
/// Effective heat capacity of the core (J/K), counting the latent and
/// gravitational energy released as the inner core freezes, which roughly
/// doubles what its iron alone would give.
const CORE_HEAT_CAPACITY: f64 = 4.0e27;
/// Potential temperature (K) of today's upper mantle.
pub const MODERN_MANTLE_K: f64 = 1600.0;
/// Temperature (K) at the top of today's core.
pub const MODERN_CORE_K: f64 = 4000.0;
const SURFACE_K: f64 = 273.0;
/// Kelvin per unit of excess.
pub const EXCESS_SCALE_K: f64 = 300.0;

/// Heat flow (mW/m^2) through ocean floor 1 Myr old; it falls as the inverse
/// square root of age.
const OCEAN_HEAT_FLOW: f64 = 490.0;
/// Heat flow (mW/m^2) through ocean floor that has cooled through.
const OLD_FLOOR_HEAT_FLOW: f64 = 48.0;
/// Youngest age (Myr) a cell's floor is taken to have: a cell is a strip of
/// floor a whole cell wide, not the ridge axis alone.
const YOUNGEST_FLOOR_MYR: f64 = 1.0;
/// Share of a continent's heat flow that trapped mantle heat holds in at full.
const TRAPPED_SHARE: f64 = 0.5;
/// Half-life (Gyr) of each heat-producing isotope in the mantle and crust,
/// with its share of the heat they produce today: uranium-238, uranium-235,
/// thorium-232 and potassium-40, for a K/U of 1.27e4 and Th/U of 4.
const ISOTOPES: [(f64, f64); 4] = [(4.47, 0.374), (0.704, 0.016), (14.0, 0.421), (1.25, 0.189)];

/// Radioactive heat (TW) at `age_ga` Gyr before the present.
pub fn radiogenic_tw(t: &Tuning, age_ga: f64) -> f64 {
    t.radiogenic_heat_now
        * ISOTOPES
            .iter()
            .map(|&(half_life, share)| share * (age_ga / half_life).exp2())
            .sum::<f64>()
}

/// Heat (TW) today's core passes to today's mantle.
pub fn modern_core_tw(t: &Tuning) -> f64 {
    (t.core_coupling * (MODERN_CORE_K - MODERN_MANTLE_K)).max(0.0)
}

/// Heat flow (mW/m^2) through one cell at today's mantle temperature.
pub fn cell_heat_flow(t: &Tuning, crust_type: u8, age: f64, keel: f32, mantle_heat: f32) -> f64 {
    if crust_type == CONTINENTAL {
        let keel = 1.0 - clamp01(t.keel_insulation) * clamp01(keel as f64);
        let trapped = 1.0 - TRAPPED_SHARE * clamp01(mantle_heat as f64);
        t.continental_heat_flow * keel * trapped
    } else {
        (OCEAN_HEAT_FLOW / age.max(YOUNGEST_FLOOR_MYR).sqrt()).max(OLD_FLOOR_HEAT_FLOW)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PlanetHeat {
    /// Potential temperature of the mantle (K).
    pub mantle_temp: f64,
    /// Temperature at the top of the core (K).
    pub core_temp: f64,
    pub last_surface_tw: f64,
    pub last_radiogenic_tw: f64,
    pub last_core_tw: f64,
    /// How fast the mantle is warming (K/Myr), relaxed toward what the budget
    /// gives over `heat_response_time`; `None` until the first step.
    rate: Option<f64>,
    /// Heat flow out through each cell (mW/m^2), as of the last step.
    pub heat_flow: Vec<f32>,
}

impl PlanetHeat {
    pub fn new(n: usize, t: &Tuning) -> PlanetHeat {
        let start = EXCESS_SCALE_K * t.primordial_heat;
        PlanetHeat {
            mantle_temp: MODERN_MANTLE_K + start,
            core_temp: MODERN_CORE_K + start,
            last_surface_tw: 0.0,
            last_radiogenic_tw: radiogenic_tw(t, t.start_age_ga),
            last_core_tw: 0.0,
            rate: None,
            heat_flow: vec![0.0; n],
        }
    }

    /// Back to the start of the world's history: the mantle and core at
    /// `primordial_heat` of excess.
    pub fn reset(&mut self, t: &Tuning) {
        *self = PlanetHeat::new(self.heat_flow.len(), t);
    }

    /// Excess over the modern mantle, 1 being about 250 K hotter; below 0
    /// once cooler than today's.
    pub fn excess(&self) -> f64 {
        (self.mantle_temp - MODERN_MANTLE_K) / EXCESS_SCALE_K
    }

    /// Heat (TW) the core is passing up into the mantle as the two stand.
    pub fn core_tw(&self, t: &Tuning) -> f64 {
        (t.core_coupling * (self.core_temp - self.mantle_temp)).max(0.0)
    }

    /// Warming of the mantle over the last step (K/Myr); negative cooling.
    pub fn rate(&self) -> f64 {
        self.rate.unwrap_or(0.0)
    }

    /// Run the budget over a step ending `elapsed + dt_myr` Myr in, over an
    /// upper mantle `upper_temp` (K) hotter or cooler than the mantle's
    /// temperature beneath each cell.
    pub fn update(
        &mut self,
        mesh: &Mesh,
        data: &CellData,
        upper_temp: &[f32],
        dt_myr: f64,
        elapsed: f64,
        t: &Tuning,
    ) {
        let n = mesh.cell_count;
        let globe = par::sum(n, |i| mesh.area(i));
        // The anomalies only move heat around within the mantle, whose
        // temperature already holds it all: heat trapped under a continent has
        // already warmed it by not leaving. So each cell loses heat by how far
        // it is above or below the mean anomaly, not the anomaly itself.
        let mean = par::sum(n, |i| upper_temp[i] as f64 * mesh.area(i)) / globe.max(1e-12);
        // A conductive lid passes heat in proportion to the temperature drop
        // across it.
        let sensitivity = t.heat_flow_sensitivity.max(0.0);
        let mantle = self.mantle_temp - mean;
        let (crust_type, age, keel, heat) = (&data.crust_type, &data.age, &data.keel, &data.mantle_heat);
        par::fill(&mut self.heat_flow, |i| {
            let local = mantle + upper_temp[i] as f64;
            let drop = ((local - SURFACE_K) / (MODERN_MANTLE_K - SURFACE_K))
                .max(0.0)
                .powf(sensitivity);
            (drop * cell_heat_flow(t, crust_type[i], age[i] as f64, keel[i], heat[i])) as f32
        });
        let flow = &self.heat_flow;
        let mean_mw = par::sum(n, |i| flow[i] as f64 * mesh.area(i)) / globe.max(1e-12);
        let surface = mean_mw * 1e-3 * EARTH_AREA_M2 / 1e12;
        let radiogenic = radiogenic_tw(t, t.start_age_ga - (elapsed + dt_myr) / 1000.0);
        let core = self.core_tw(t);

        let raw = (radiogenic + core - surface) * TW_MYR_J / MANTLE_HEAT_CAPACITY;
        let rate = match self.rate {
            None => raw,
            Some(r) if t.heat_response_time > 0.0 => {
                r + (raw - r) * (1.0 - (-dt_myr / t.heat_response_time).exp())
            }
            Some(_) => raw,
        };
        self.rate = Some(rate);
        self.mantle_temp += rate * dt_myr;
        self.core_temp -= core * TW_MYR_J / CORE_HEAT_CAPACITY * dt_myr;
        self.last_surface_tw = surface;
        self.last_radiogenic_tw = radiogenic;
        self.last_core_tw = core;
    }
}

/// The tuning as the world sees it `elapsed` Myr in over a mantle `excess`
/// hotter than today's (see the module docs), and with ridge push coming in
/// over the first `seafloor_flattening_age`: the sea floor a world starts
/// with has ages that no ridge laid down, so a push down their slopes would
/// steer the plates by the starting pattern until floor from real ridges
/// replaces it.
pub fn effective_tuning(t: &Tuning, elapsed: f64, excess: f64) -> Tuning {
    let mut eff = t.clone();
    if excess > 0.0 {
        eff.accretion_fraction *= clamp01(1.0 - t.hot_arc_suppression * excess);
    }
    let thinner = (-t.viscosity_sensitivity * excess).exp();
    eff.mantle_drag *= thinner;
    eff.motion_response_time *= thinner;
    eff.ridge_elevation += t.ridge_heat_uplift * excess;
    // Kept short of the continental threshold, or fresh floor beside a
    // continent would turn continental as it was laid.
    let melt = (1.0 + t.melt_gain * excess).max(MIN_MELT);
    eff.ridge_crust_thickness =
        (t.oceanic_thickness * melt).min(RIDGE_CRUST_CEILING * t.continental_thickness_threshold);
    eff.ridge_weight *= clamp01(elapsed / t.seafloor_flattening_age.max(1e-6));
    eff
}

/// Least share of `oceanic_thickness` a cold mantle still melts at a ridge.
const MIN_MELT: f64 = 0.2;
/// Most a ridge lays, as a share of `continental_thickness_threshold`.
const RIDGE_CRUST_CEILING: f64 = 0.9;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::OCEANIC;
    use crate::world::Sim;

    #[test]
    fn radioactive_heat_was_about_four_times_todays_when_the_earth_formed() {
        let t = Tuning::default();
        let now = radiogenic_tw(&t, 0.0);
        assert!((now - t.radiogenic_heat_now).abs() < 1e-9, "today {now} TW");
        let ratio = radiogenic_tw(&t, 4.5) / now;
        assert!((4.0..5.5).contains(&ratio), "4.5 Ga at {ratio:.2}x today's");
    }

    #[test]
    fn young_floor_loses_more_heat_than_old() {
        let t = Tuning::default();
        let flow = |age: f64| cell_heat_flow(&t, OCEANIC, age, 0.0, 0.0);
        assert!(flow(5.0) > 2.0 * flow(50.0));
        assert!(flow(50.0) > flow(150.0));
        assert_eq!(flow(150.0), OLD_FLOOR_HEAT_FLOW);
    }

    /// One step of the budget over a level 3 world all of one crust.
    fn one_step(fill: impl Fn(&mut CellData, usize)) -> PlanetHeat {
        let mesh = Mesh::icosphere(3);
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        for i in 0..n {
            fill(&mut data, i);
        }
        let t = Tuning::default();
        let mut heat = PlanetHeat::new(n, &t);
        heat.update(&mesh, &data, &vec![0.0; n], t.dt_myr, 0.0, &t);
        heat
    }

    #[test]
    fn a_world_of_old_floor_cools_more_slowly_than_one_of_young() {
        let ocean = |age: f32| {
            one_step(move |d, i| {
                d.crust_type[i] = OCEANIC;
                d.age[i] = age;
            })
        };
        let (young, old) = (ocean(10.0), ocean(150.0));
        assert!(young.last_surface_tw > 2.0 * old.last_surface_tw);
        assert!(young.rate() < old.rate(), "young {} K/Myr, old {}", young.rate(), old.rate());
    }

    #[test]
    fn an_insulated_continent_holds_its_heat_in() {
        let continent = |keel: f32, trapped: f32| {
            one_step(move |d, i| {
                d.crust_type[i] = CONTINENTAL;
                d.keel[i] = keel;
                d.mantle_heat[i] = trapped;
            })
        };
        let open = continent(0.0, 0.0).last_surface_tw;
        let keeled = continent(1.0, 0.0).last_surface_tw;
        let blanketed = continent(0.0, 1.0).last_surface_tw;
        assert!(keeled < 0.9 * open, "keel: {keeled} of {open} TW");
        assert!(blanketed < 0.9 * open, "trapped heat: {blanketed} of {open} TW");
    }

    #[test]
    fn upper_mantle_anomalies_move_heat_loss_around_without_adding_to_it() {
        let mesh = Mesh::icosphere(3);
        let n = mesh.cell_count;
        let mut data = CellData::new(n);
        data.age.fill(50.0);
        let t = Tuning::default();
        let run = |anomaly: &[f32]| {
            let mut heat = PlanetHeat::new(n, &t);
            heat.update(&mesh, &data, anomaly, t.dt_myr, 0.0, &t);
            heat
        };
        let still = run(&vec![0.0; n]);
        let uniform = run(&vec![100.0; n]);
        assert!((still.last_surface_tw - uniform.last_surface_tw).abs() < 1e-6 * still.last_surface_tw);
        let split: Vec<f32> = (0..n)
            .map(|i| if mesh.centres[i * 3 + 2] > 0.0 { 100.0 } else { -100.0 })
            .collect();
        let patchy = run(&split);
        let (hot, cold) = (0..n).fold((0usize, 0usize), |(h, c), i| {
            if split[i] > 0.0 { (i, c) } else { (h, i) }
        });
        assert!(patchy.heat_flow[hot] > still.heat_flow[hot]);
        assert!(patchy.heat_flow[cold] < still.heat_flow[cold]);
        let change = patchy.last_surface_tw / still.last_surface_tw - 1.0;
        assert!(change.abs() < 0.05, "patchy anomalies changed the total by {:.1}%", change * 100.0);
    }

    #[test]
    fn a_hotter_mantle_lays_thicker_crust_on_a_higher_ridge_under_thinner_drag() {
        let t = Tuning::default();
        let (cold, hot) = (effective_tuning(&t, 0.0, 0.0), effective_tuning(&t, 0.0, 1.0));
        assert_eq!(cold.ridge_crust_thickness, t.oceanic_thickness);
        assert!(hot.ridge_crust_thickness > cold.ridge_crust_thickness);
        assert!(hot.ridge_crust_thickness < t.continental_thickness_threshold);
        assert!(hot.ridge_elevation > cold.ridge_elevation);
        assert!(hot.mantle_drag < cold.mantle_drag);
        assert!(hot.motion_response_time < cold.motion_response_time);
    }

    #[test]
    fn the_mantle_cools_over_a_default_run() {
        let mut sim = Sim::new(4);
        sim.crack_great_circles(4, "cooling");
        let start = sim.mantle_excess();
        let ticks = (1500.0 / sim.tuning.dt_myr) as usize;
        for _ in 0..ticks {
            sim.tick();
        }
        let m = sim.metrics();
        assert!(m.mantle_excess < start - 0.2, "excess {start:.2} to {:.2}", m.mantle_excess);
        assert!((20.0..120.0).contains(&m.surface_heat_tw), "surface {:.1} TW", m.surface_heat_tw);
        assert!(sim.mass_drift().abs() < 1e-3, "drift {}", sim.mass_drift());
    }
}
