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
    pub ridge_weight: f64,
    pub mantle_drag: f64,
    pub continental_drag: f64,
    pub collision_drag: f64,
    pub collision_lock_gain: f64,
    pub suture_coupling: f64,
    pub suture_merge_time: f64,
    pub boundary_death_time: f64,
    pub merge_quiet_speed: f64,

    pub max_oceanic_age: f64,
    pub continental_density: f64,
    pub oceanic_density_young: f64,
    pub oceanic_density_old: f64,
    pub oceanic_thickness: f64,
    pub continental_thickness_threshold: f64,
    pub max_crust_thickness: f64,
    pub delamination_rate: f64,

    pub accretion_fraction: f64,
    pub max_thickening_per_myr: f64,
    pub crust_spread_start: f64,
    pub crust_spread_rate: f64,
    pub subduction_erosion: f64,
    pub rift_density: f64,
    pub rift_thickness_floor: f64,

    pub rift_strength: f64,
    pub suture_weakness: f64,
    pub rift_margin_weakness: f64,
    pub weakness_decay_time: f64,
    pub weakness_relief: f64,
    pub min_rift_area: f64,

    pub insulation_rate: f64,
    pub insulation_cooling: f64,
    pub insulation_weakening: f64,
    pub insulation_drive: f64,

    pub min_plate_area: f64,

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
            ridge_weight: 0.05,
            mantle_drag: 30.0,
            continental_drag: 3.0,
            collision_drag: 400.0,
            collision_lock_gain: 10.0,
            suture_coupling: 0.05,
            suture_merge_time: 80.0,
            boundary_death_time: 120.0,
            merge_quiet_speed: 0.002,

            max_oceanic_age: 200.0,
            continental_density: 2.7,
            oceanic_density_young: 2.9,
            oceanic_density_old: 3.0,
            oceanic_thickness: 7.0,
            continental_thickness_threshold: 15.0,
            max_crust_thickness: 60.0,
            delamination_rate: 0.3,

            accretion_fraction: 0.2,
            max_thickening_per_myr: 0.5,
            subduction_erosion: 0.05,
            crust_spread_start: 16.0,
            crust_spread_rate: 0.05,
            rift_density: 2.9,
            rift_thickness_floor: 2.0,

            rift_strength: 1.1,
            suture_weakness: 1.0,
            rift_margin_weakness: 0.6,
            weakness_decay_time: 2000.0,
            weakness_relief: 0.7,
            min_rift_area: 0.03,

            insulation_rate: 0.004,
            insulation_cooling: 0.01,
            insulation_weakening: 0.6,
            insulation_drive: 1.2,

            min_plate_area: 0.0005,

            active_speed_eps: 1e-5,
            dt_myr: 2.0,
        }
    }
}

/// Modelling choices that are on or off rather than a number to dial.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SimOptions {
    pub supercontinent_insulation: bool,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            supercontinent_insulation: true,
        }
    }
}
