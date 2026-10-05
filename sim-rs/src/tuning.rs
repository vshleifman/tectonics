//! Every tunable constant, mirrored from `src/sim/tuning.ts`. The UI owns the
//! values and forwards them whole; field names match the TypeScript ones so the
//! object deserialises straight across. See the TypeScript for what each means.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tuning {
    pub ridge_elevation: f64,
    pub abyss_elevation: f64,
    pub continent_base: f64,
    pub continent_thickness_ref: f64,
    pub continent_thickness_gain: f64,
    pub collision_uplift: f64,
    pub arc_uplift: f64,
    pub trench_deepen: f64,
    pub ridge_pull: f64,
    pub relax_rate: f64,
    pub elevation_min: f64,
    pub elevation_max: f64,

    pub slab_weight: f64,
    pub slab_pull_onset_age: f64,
    pub slab_pull_saturation_age: f64,
    pub slab_full_length: f64,
    pub slab_decay_time: f64,
    pub slab_initiation_time: f64,
    pub slab_initiation_age: f64,
    pub trench_suction: f64,
    pub dome_push_duration: f64,
    pub max_turn_rate: f64,
    pub motion_response_time: f64,
    pub ridge_weight: f64,
    pub mantle_drag: f64,
    pub continental_drag: f64,
    pub collision_drag: f64,
    pub collision_lock_gain: f64,
    pub suture_coupling: f64,
    pub suture_merge_time: f64,
    pub boundary_death_time: f64,
    pub merge_quiet_speed: f64,
    pub mantle_flow_coupling: f64,
    pub downwelling_memory: f64,
    pub upwelling_gain: f64,

    pub max_oceanic_age: f64,
    pub continental_density: f64,
    pub oceanic_density_young: f64,
    pub oceanic_density_old: f64,
    pub oceanic_thickness: f64,
    pub continental_thickness_threshold: f64,
    pub continental_revert_thickness: f64,
    pub continental_nucleation_thickness: f64,
    pub max_crust_thickness: f64,
    pub delamination_rate: f64,

    pub accretion_fraction: f64,
    pub max_thickening_per_myr: f64,
    pub crust_spread_start: f64,
    pub crust_spread_rate: f64,
    pub subduction_erosion: f64,
    pub margin_erosion: f64,
    pub rift_density: f64,
    pub rift_thickness_floor: f64,
    pub basin_infill_rate: f64,
    pub basin_infill_max_area: f64,
    pub min_continent_area: f64,
    pub cohesion_reach: f64,
    pub continental_area_target: f64,
    pub continental_recycling_rate: f64,
    pub craton_min_age: f64,
    pub craton_inland: f64,
    pub craton_growth_time: f64,
    pub craton_decay_time: f64,
    pub craton_strength: f64,
    pub craton_keel_gain: f64,
    pub craton_durability: f64,
    pub craton_merge_time: f64,

    pub rift_strength: f64,
    pub suture_weakness: f64,
    pub rift_margin_weakness: f64,
    pub weakness_decay_time: f64,
    pub weakness_relief: f64,
    pub min_rift_area: f64,
    pub max_rift_aspect: f64,
    pub oceanic_strength_young: f64,
    pub oceanic_strength_old: f64,
    pub rift_duration: f64,
    pub rift_stretch: f64,

    pub insulation_rate: f64,
    pub insulation_cooling: f64,
    pub insulation_weakening: f64,
    pub insulation_drive: f64,
    pub insulation_reach: f64,
    pub dome_push: f64,
    pub rift_heat_release: f64,

    pub min_plate_area: f64,
    pub passive_margin_collapse_age: f64,

    pub primordial_heat: f64,
    pub primordial_cooling_time: f64,
    pub plume_rate: f64,
    pub plume_radius: f64,
    pub plume_duration: f64,
    pub plume_thickness: f64,
    pub plume_keel: f64,
    pub hot_arc_suppression: f64,
    pub hot_drip: f64,

    pub active_speed_eps: f64,
    pub dt_myr: f64,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            ridge_elevation: -2.5,
            abyss_elevation: -6.0,
            continent_base: 0.6,
            continent_thickness_ref: 7.0,
            continent_thickness_gain: 0.1,
            collision_uplift: 4.0,
            arc_uplift: 3.0,
            trench_deepen: 3.5,
            ridge_pull: 0.05,
            relax_rate: 0.02,
            elevation_min: -10.0,
            elevation_max: 10.0,

            slab_weight: 1.0,
            slab_pull_onset_age: 25.0,
            slab_pull_saturation_age: 80.0,
            slab_full_length: 0.1,
            slab_decay_time: 30.0,
            slab_initiation_time: 20.0,
            slab_initiation_age: 0.0,
            trench_suction: 0.0,
            dome_push_duration: 60.0,
            max_turn_rate: 0.0,
            motion_response_time: 40.0,
            ridge_weight: 0.05,
            mantle_drag: 30.0,
            continental_drag: 3.0,
            collision_drag: 400.0,
            collision_lock_gain: 10.0,
            suture_coupling: 0.05,
            suture_merge_time: 80.0,
            boundary_death_time: 120.0,
            merge_quiet_speed: 0.002,
            mantle_flow_coupling: 30.0,
            downwelling_memory: 800.0,
            upwelling_gain: 0.0,

            max_oceanic_age: 200.0,
            continental_density: 2.7,
            oceanic_density_young: 2.9,
            oceanic_density_old: 3.0,
            oceanic_thickness: 7.0,
            continental_thickness_threshold: 15.0,
            continental_revert_thickness: 10.0,
            continental_nucleation_thickness: 20.0,
            max_crust_thickness: 60.0,
            delamination_rate: 0.3,

            accretion_fraction: 0.2,
            max_thickening_per_myr: 0.5,
            subduction_erosion: 0.05,
            margin_erosion: 0.12,
            crust_spread_start: 35.0,
            crust_spread_rate: 0.05,
            rift_density: 2.9,
            rift_thickness_floor: 2.0,
            basin_infill_rate: 1.0,
            basin_infill_max_area: 0.0005,
            min_continent_area: 0.00005,
            cohesion_reach: 2.0,
            continental_area_target: 0.4,
            continental_recycling_rate: 0.0004,
            craton_min_age: 500.0,
            craton_inland: 0.02,
            craton_growth_time: 400.0,
            craton_decay_time: 100.0,
            craton_strength: 2.0,
            craton_keel_gain: 1.0,
            craton_durability: 0.75,
            craton_merge_time: 60.0,

            rift_strength: 0.65,
            suture_weakness: 1.0,
            rift_margin_weakness: 0.6,
            weakness_decay_time: 2000.0,
            weakness_relief: 0.7,
            min_rift_area: 0.015,
            max_rift_aspect: 3.0,
            oceanic_strength_young: 0.6,
            oceanic_strength_old: 1.5,
            rift_duration: 20.0,
            rift_stretch: 3.0,

            insulation_rate: 0.004,
            insulation_cooling: 0.01,
            insulation_weakening: 0.6,
            insulation_drive: 1.2,
            insulation_reach: 0.1,
            dome_push: 0.5,
            rift_heat_release: 0.5,

            min_plate_area: 0.0005,
            passive_margin_collapse_age: 0.0,

            primordial_heat: 0.0,
            primordial_cooling_time: 1000.0,
            plume_rate: 0.1,
            plume_radius: 0.08,
            plume_duration: 20.0,
            plume_thickness: 35.0,
            plume_keel: 1.0,
            hot_arc_suppression: 1.0,
            hot_drip: 1.0,

            active_speed_eps: 1e-5,
            dt_myr: 2.0,
        }
    }
}

/// Keel at and above which continental crust counts as craton, and is
/// protected by `craton_durability`.
pub const CRATON_KEEL: f32 = 0.5;

impl Tuning {
    /// Factor by which anything that wears crust away (trench and margin
    /// erosion, delamination, loss of keel) acts on a cell with this keel:
    /// `1 - craton_durability` on craton, 1 elsewhere.
    #[inline]
    pub fn craton_wear(&self, keel: f32) -> f64 {
        if keel >= CRATON_KEEL {
            (1.0 - self.craton_durability).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// Modelling choices that are on or off rather than a number to dial.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SimOptions {
    pub supercontinent_insulation: bool,
    /// Weld two plates only where their continents are closing on balance
    /// along all their contact, rather than wherever any stretch of it closes.
    /// A continent rifting apart by rotation closes at one end of the rift as
    /// it opens along the rest, which otherwise welds it shut at once.
    pub weld_on_net_closing: bool,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            supercontinent_insulation: true,
            weld_on_net_closing: false,
        }
    }
}
