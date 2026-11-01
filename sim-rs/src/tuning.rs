//! Every tunable constant, mirrored from `src/sim/tuning.ts`. The UI owns the
//! values and forwards them whole; field names match the TypeScript ones so the
//! object deserialises straight across. See the TypeScript for what each means.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tuning {
    pub ridge_elevation: f64,
    pub abyss_elevation: f64,
    pub seafloor_flattening_age: f64,
    pub mantle_density: f64,
    pub water_density: f64,
    pub ocean_volume: f64,
    pub trench_depth: f64,
    pub erosion_time: f64,
    pub shelf_depth: f64,
    pub slope_capture: f64,
    pub sediment_reach: f64,
    /// Share of the sediment reaching the overriding edge of a trench that goes
    /// down with the plate rather than building the accretionary prism. About
    /// 70% of the incoming section subducts beneath large accretionary prisms
    /// and nearly all of it at non-accretionary margins (von Huene & Scholl
    /// 1991, Rev. Geophys. 29); most trench sediment lies at the former.
    pub trench_sediment_subduction: f64,
    pub elevation_min: f64,
    pub elevation_max: f64,

    pub slab_weight: f64,
    pub slab_pull_saturation_buoyancy: f64,
    pub slab_full_length: f64,
    pub slab_decay_time: f64,
    pub slab_initiation_time: f64,
    pub slab_initiation_age: f64,
    pub trench_suction: f64,
    pub dome_push_duration: f64,
    pub max_turn_rate: f64,
    pub motion_response_time: f64,
    pub microplate_area: f64,
    pub microplate_damping: f64,
    pub ridge_weight: f64,
    pub mantle_drag: f64,
    pub continental_drag: f64,
    pub collision_drag: f64,
    pub collision_lock_gain: f64,
    pub suture_coupling: f64,
    /// How strongly a plate's target motion is drawn toward each neighbour's,
    /// per unit of shared boundary over the plate's width. Small plates, with
    /// a long boundary for their area, are carried along and then merge.
    /// Even a small value locks neighbouring large plates together over a
    /// supercontinent cycle, so the default leaves every plate to its own forces.
    pub edge_coupling: f64,
    pub suture_merge_time: f64,
    pub boundary_death_time: f64,
    pub merge_quiet_speed: f64,
    pub mantle_flow_coupling: f64,
    pub downwelling_memory: f64,
    pub upwelling_gain: f64,
    pub slab_cooling: f64,
    pub site_insulation: f64,
    pub site_diffusion: f64,
    pub upper_relax_time: f64,
    pub upper_temp_max: f64,
    pub insulation_site_gain: f64,
    pub slab_transit_time: f64,
    pub slab_pile_lifetime: f64,
    pub lower_diffusion: f64,
    pub pile_cooling: f64,
    pub plume_floor: f64,
    pub dyn_topo_gain: f64,
    pub sink_topo_gain: f64,
    pub dyn_topo_max: f64,

    pub max_oceanic_age: f64,
    pub mafic_density: f64,
    pub felsic_density: f64,
    pub metasediment_density: f64,
    pub cover_density: f64,
    pub eclogite_density: f64,
    pub eclogite_depth: f64,
    /// Time (Myr) over which mafic rock that has turned to eclogite at the
    /// base of the crust founders into the mantle it is denser than. Dense
    /// roots under arcs and orogens drip off in a few Myr where the lower
    /// crust is hot and weak, longer where it is cold (Jull & Kelemen 2001;
    /// Jagoutz & Behn 2013): ~10.
    pub eclogite_founder_time: f64,
    pub oceanic_thickness: f64,
    pub continental_thickness_threshold: f64,
    pub continental_revert_thickness: f64,
    pub continental_nucleation_thickness: f64,
    /// Share of a cell's basement (felsic rock plus sediment buried into it,
    /// not loose cover) that must be evolved for ocean crust to count as
    /// continental. Bulk continental crust is andesitic, ~57-61% SiO2 (Rudnick
    /// and Gao 2003, Treatise on Geochemistry 3.01), which on a mix of basalt
    /// (~50%) and granite (~72%) is roughly 35-50% felsic; this is the lower
    /// edge. Fixed from composition, not tuned against outcomes.
    pub continental_felsic_share: f64,
    /// Evolved share below which continental crust reverts to ocean. Below
    /// `continental_felsic_share` so a cell near the line does not flicker; the
    /// gap is hysteresis, not a physical claim.
    pub oceanic_felsic_share: f64,
    /// Temperature (°C) at the base of crust over active arc magmatism. Basalt
    /// from the mantle wedge ponds and intrudes there, holding the lower crust
    /// near its own temperature whatever the surface loses: deep crustal hot
    /// zones run ~900-1100 °C (Annen, Blundy & Sparks 2006; the MASH zone of
    /// Hildreth & Moorbath 1988). Heat carried in by magma, not conducted, so
    /// the surface heat flow says little about it. It is what lets an arc melt
    /// its base and evolve toward continental crust.
    pub arc_base_temp: f64,
    /// Myr over which that extra heat fades once arc magmatism stops: the
    /// conductive time of a ~30 km crust (L^2 / kappa, about 30 Myr), taken up
    /// to 50 for the lower crust a pulse has to warm.
    pub arc_heat_duration: f64,
    pub max_crust_thickness: f64,
    pub delamination_rate: f64,

    pub lithosphere_growth: f64,
    pub plate_thickness: f64,
    pub continental_lithosphere: f64,
    pub keel_lithosphere: f64,
    pub rift_lithosphere: f64,
    pub lithosphere_growth_time: f64,
    pub lithosphere_thinning_time: f64,
    pub thermal_expansion: f64,
    pub lid_delta_t: f64,
    pub keel_depletion: f64,

    /// Share of the ocean crust going down at a trench that the arc behind it
    /// keeps. Earth's arcs add ~2.5–3 km³/yr (Scholl & von Huene 2007; Clift
    /// et al. 2009) while ~24 km³/yr of ocean crust goes down (~3.4 km²/yr of
    /// sea floor made, Rowley 2002, at ~7 km): ~0.11. The model's arcs lose
    /// almost nothing to foundering later, so this is net crust kept.
    pub accretion_fraction: f64,
    /// Share of the sediment on crust going down a trench that is scraped off
    /// onto the plate above as an accretionary wedge; the rest goes down with
    /// the plate. Most trench sediment is subducted, accretionary margins
    /// keeping part of it (von Huene & Scholl 1991; Clift & Vannucchi 2004):
    /// roughly a quarter.
    pub sediment_offscrape: f64,
    pub arc_setback: f64,
    pub back_arc_width: f64,
    pub back_arc_extension: f64,
    pub max_thickening_per_myr: f64,
    pub crust_spread_start: f64,
    pub crust_spread_rate: f64,
    pub subduction_erosion: f64,
    pub margin_erosion: f64,
    pub rift_thickness_floor: f64,
    pub basin_infill_rate: f64,
    pub basin_infill_max_area: f64,
    pub min_continent_area: f64,
    pub min_craton_area: f64,
    pub cohesion_reach: f64,
    pub continental_area_target: f64,
    pub continental_target_time: f64,
    pub continental_target_gain: f64,
    /// Share of the continental area recycled per Myr at active margins, when
    /// no area target is held. 0 by default: sediment subducted at trenches
    /// and subduction erosion of margins and arcs are modelled where they
    /// happen, and a fixed rate on top would count them twice.
    pub continental_recycling_rate: f64,
    pub craton_min_age: f64,
    pub craton_inland: f64,
    pub craton_growth_time: f64,
    pub craton_decay_time: f64,
    pub craton_strength: f64,
    pub craton_keel_gain: f64,
    pub craton_durability: f64,
    pub craton_merge_time: f64,
    /// Share of the way two welded plates whose cratons touch are drawn to a
    /// common target motion each tick, on top of `suture_coupling`. Once the
    /// same number as `craton_durability`.
    pub craton_weld_coupling: f64,

    pub rift_strength: f64,
    pub suture_weakness: f64,
    pub rift_margin_weakness: f64,
    pub weakness_decay_time: f64,
    /// Myr over which weakness in oceanic lithosphere fades. Ocean cools faster
    /// than a continental suture, but a heal of about 100 Myr stalls rifts that
    /// have to cross young ocean and the plates then weld into one. The default
    /// matches `weakness_decay_time`, so a rift can finish.
    pub ocean_weakness_decay_time: f64,
    pub weakness_relief: f64,
    pub min_rift_area: f64,
    pub max_rift_aspect: f64,
    pub oceanic_strength_young: f64,
    pub oceanic_strength_old: f64,
    pub rift_duration: f64,
    /// Myr any rift stretches before the plate can break, even through ocean.
    /// A positive floor turns a spike of tension into a stall. The default is
    /// 0: a few Myr of required stretch lets craton welds outrun new rifts.
    pub min_rift_duration: f64,
    pub rift_stretch: f64,

    pub insulation_rate: f64,
    pub insulation_cooling: f64,
    pub insulation_weakening: f64,
    pub insulation_drive: f64,
    pub insulation_reach: f64,
    pub dome_push: f64,
    pub rift_heat_release: f64,

    pub min_plate_area: f64,
    /// Smallest share of the globe a piece pinched off a plate must cover
    /// before it becomes a plate. Below it, the piece joins the neighbour it
    /// shares the most boundary with.
    pub min_fragment_area: f64,
    /// Myr ocean must have aged against a continent on the same plate before
    /// the margin founders and the ocean breaks away to start a trench; 0
    /// turns collapse off. Earth's oldest sea floor is under 200 Myr, so a
    /// plate whose ocean reaches this has stopped recycling it.
    pub passive_margin_collapse_age: f64,

    pub primordial_heat: f64,
    pub start_age_ga: f64,
    pub radiogenic_heat_now: f64,
    pub core_coupling: f64,
    pub heat_response_time: f64,
    pub heat_flow_sensitivity: f64,
    pub continental_heat_flow: f64,
    pub lithification_depth: f64,
    pub island_arc_felsic: f64,
    pub continental_arc_felsic: f64,
    pub differentiation_rate: f64,
    pub mafic_solidus: f64,
    pub melting_span: f64,
    pub surface_temp: f64,
    pub crust_conductivity: f64,
    pub crust_heat_production: f64,
    pub facies_zeolite_temp: f64,
    pub facies_greenschist_temp: f64,
    pub facies_amphibolite_temp: f64,
    pub facies_granulite_temp: f64,
    pub facies_blueschist_pressure: f64,
    pub facies_blueschist_max_temp: f64,
    pub facies_eclogite_pressure: f64,
    pub subduction_peak_pressure: f64,
    pub subduction_peak_temp: f64,
    pub resource_arc_age_full: f64,
    pub resource_arc_age_zero: f64,
    pub porphyry_min_depth: f64,
    pub porphyry_max_depth: f64,
    pub epithermal_max_depth: f64,
    pub vms_max_age: f64,
    pub vms_max_temp: f64,
    pub chromite_max_temp: f64,
    pub gold_min_temp: f64,
    pub gold_max_temp: f64,
    pub granite_min_temp: f64,
    pub kimberlite_min_age: f64,
    pub keel_insulation: f64,
    pub viscosity_sensitivity: f64,
    pub ridge_heat_uplift: f64,
    pub melt_gain: f64,
    pub plume_rate: f64,
    pub plume_radius: f64,
    pub plume_duration: f64,
    pub plume_thickness: f64,
    pub plume_thickness_cold: f64,
    pub plume_keel: f64,
    pub plume_tail_duration: f64,
    pub plume_tail_radius: f64,
    pub plume_tail_thickness: f64,
    pub plume_swell: f64,
    pub plume_swell_radius: f64,
    pub plume_drift: f64,
    pub hot_arc_suppression: f64,
    pub hot_drip: f64,
    /// Mantle excess (see `heat.rs`) at which slabs become strong enough for
    /// sustained modern-style subduction, so that arcs build continent freely
    /// and thickened floor no longer founders. Models of subduction in a hot
    /// mantle put the change from the pre-subduction regime at 250-200 K above
    /// today's, abrupt by 175-160 K (Sizova et al. 2010, Lithos 116); weak,
    /// hot slabs break off too often to sustain it (van Hunen and van den Berg
    /// 2008, Lithos 103). 0.68 is ~205 K, the middle of that range. Fixed from
    /// those sources, not tuned against outcomes.
    pub modern_subduction_excess: f64,
    /// How wide that change is, in the same units: the brakes are full half this
    /// far above `modern_subduction_excess` and gone half this far below it.
    /// 0.3 is ~90 K, the 250 K to 160 K span of the same models.
    pub modern_subduction_width: f64,

    pub active_speed_eps: f64,
    pub dt_myr: f64,

    /// Thickness (km) of the crust a ridge lays over the mantle as it is now.
    /// Derived, not tuned: only `heat::effective_tuning` sets it.
    #[serde(skip)]
    pub ridge_crust_thickness: f64,
    /// Whether crust type follows composition (`SimOptions::composition_crust_type`).
    /// Derived, not tuned: only `Sim::effective_tuning` sets it.
    #[serde(skip)]
    pub composition_crust_type: bool,
}

impl Tuning {
    /// Density (g/cm³) of crust that is half mafic and half felsic, about what
    /// ordinary continental crust comes to. For calibrations and tests that
    /// need a continent without building its lanes.
    pub fn mean_continental_density(&self) -> f64 {
        0.5 * (self.mafic_density + self.felsic_density)
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            ridge_elevation: -2.5,
            abyss_elevation: -5.5,
            seafloor_flattening_age: 100.0,
            mantle_density: 3.3,
            water_density: 1.03,
            ocean_volume: 2.6,
            trench_depth: 3.0,
            erosion_time: 150.0,
            shelf_depth: -0.15,
            slope_capture: 0.5,
            sediment_reach: 3.0,
            trench_sediment_subduction: 0.7,
            elevation_min: -10.0,
            elevation_max: 10.0,

            slab_weight: 1.0,
            slab_pull_saturation_buoyancy: 0.7,
            slab_full_length: 0.1,
            slab_decay_time: 30.0,
            slab_initiation_time: 20.0,
            slab_initiation_age: 0.0,
            trench_suction: 0.0,
            dome_push_duration: 60.0,
            max_turn_rate: 0.0,
            motion_response_time: 40.0,
            microplate_area: 0.01,
            microplate_damping: 5.0,
            ridge_weight: 0.058333333,
            mantle_drag: 30.0,
            continental_drag: 3.0,
            collision_drag: 400.0,
            collision_lock_gain: 10.0,
            suture_coupling: 0.05,
            edge_coupling: 0.0,
            suture_merge_time: 80.0,
            boundary_death_time: 120.0,
            merge_quiet_speed: 0.002,
            mantle_flow_coupling: 30.0,
            downwelling_memory: 800.0,
            upwelling_gain: 0.0,
            slab_cooling: 25.0,
            site_insulation: 1.0,
            site_diffusion: 0.01,
            upper_relax_time: 150.0,
            upper_temp_max: 300.0,
            insulation_site_gain: 1.0,
            slab_transit_time: 100.0,
            slab_pile_lifetime: 700.0,
            lower_diffusion: 0.002,
            pile_cooling: 0.005,
            plume_floor: 0.1,
            dyn_topo_gain: 0.01,
            sink_topo_gain: 20.0,
            dyn_topo_max: 1.5,

            max_oceanic_age: 200.0,
            mafic_density: 2.9,
            felsic_density: 2.7,
            metasediment_density: 2.7,
            cover_density: 2.4,
            eclogite_density: 3.45,
            eclogite_depth: 45.0,
            eclogite_founder_time: 10.0,
            oceanic_thickness: 7.0,
            continental_thickness_threshold: 15.0,
            continental_revert_thickness: 10.0,
            continental_nucleation_thickness: 20.0,
            continental_felsic_share: 0.35,
            oceanic_felsic_share: 0.20,
            arc_base_temp: 1000.0,
            arc_heat_duration: 50.0,
            max_crust_thickness: 60.0,
            delamination_rate: 0.3,

            lithosphere_growth: 9.0,
            plate_thickness: 100.0,
            continental_lithosphere: 150.0,
            keel_lithosphere: 100.0,
            rift_lithosphere: 30.0,
            lithosphere_growth_time: 200.0,
            lithosphere_thinning_time: 10.0,
            thermal_expansion: 3e-5,
            lid_delta_t: 1300.0,
            keel_depletion: 0.025,

            accretion_fraction: 0.11,
            sediment_offscrape: 0.25,
            arc_setback: 0.025,
            back_arc_width: 0.05,
            back_arc_extension: 0.25,
            max_thickening_per_myr: 0.5,
            subduction_erosion: 0.05,
            margin_erosion: 0.12,
            crust_spread_start: 35.0,
            crust_spread_rate: 0.05,
            rift_thickness_floor: 2.0,
            basin_infill_rate: 1.0,
            basin_infill_max_area: 0.0005,
            min_continent_area: 0.00005,
            min_craton_area: 0.0003,
            cohesion_reach: 2.0,
            continental_area_target: 0.4,
            continental_target_time: 200.0,
            continental_target_gain: 4.0,
            continental_recycling_rate: 0.0,
            craton_min_age: 500.0,
            craton_inland: 0.02,
            craton_growth_time: 400.0,
            craton_decay_time: 100.0,
            craton_strength: 2.0,
            craton_keel_gain: 1.0,
            craton_durability: 0.75,
            craton_merge_time: 60.0,
            craton_weld_coupling: 0.75,

            rift_strength: 0.65,
            suture_weakness: 1.0,
            rift_margin_weakness: 0.6,
            weakness_decay_time: 2000.0,
            ocean_weakness_decay_time: 2000.0,
            weakness_relief: 0.7,
            min_rift_area: 0.015,
            max_rift_aspect: 3.0,
            oceanic_strength_young: 0.7,
            oceanic_strength_old: 1.5,
            rift_duration: 20.0,
            min_rift_duration: 0.0,
            rift_stretch: 3.0,

            insulation_rate: 0.004,
            insulation_cooling: 0.01,
            insulation_weakening: 0.6,
            insulation_drive: 1.2,
            insulation_reach: 0.1,
            dome_push: 0.5,
            rift_heat_release: 0.5,

            min_plate_area: 0.0005,
            min_fragment_area: 0.002,
            passive_margin_collapse_age: 200.0,

            primordial_heat: 1.0,
            start_age_ga: 3.0,
            radiogenic_heat_now: 20.0,
            core_coupling: 0.004,
            heat_response_time: 50.0,
            heat_flow_sensitivity: 4.0,
            continental_heat_flow: 65.0,
            lithification_depth: 2.5,
            island_arc_felsic: 0.35,
            continental_arc_felsic: 0.6,
            differentiation_rate: 0.002,
            mafic_solidus: 850.0,
            melting_span: 150.0,
            surface_temp: 15.0,
            crust_conductivity: 2.5,
            crust_heat_production: 1.0,
            facies_zeolite_temp: 150.0,
            facies_greenschist_temp: 300.0,
            facies_amphibolite_temp: 500.0,
            facies_granulite_temp: 700.0,
            facies_blueschist_pressure: 6.0,
            facies_blueschist_max_temp: 450.0,
            facies_eclogite_pressure: 12.0,
            subduction_peak_pressure: 8.0,
            subduction_peak_temp: 250.0,
            resource_arc_age_full: 30.0,
            resource_arc_age_zero: 80.0,
            porphyry_min_depth: 1.0,
            porphyry_max_depth: 5.0,
            epithermal_max_depth: 1.5,
            vms_max_age: 30.0,
            vms_max_temp: 450.0,
            chromite_max_temp: 600.0,
            gold_min_temp: 300.0,
            gold_max_temp: 600.0,
            granite_min_temp: 650.0,
            kimberlite_min_age: 1500.0,
            keel_insulation: 0.4,
            viscosity_sensitivity: 0.1,
            ridge_heat_uplift: 0.1,
            melt_gain: 0.1,
            plume_rate: 0.1,
            plume_radius: 0.08,
            plume_duration: 20.0,
            plume_thickness: 35.0,
            plume_thickness_cold: 13.0,
            plume_keel: 1.0,
            plume_tail_duration: 80.0,
            plume_tail_radius: 0.02,
            plume_tail_thickness: 4.0,
            plume_swell: 1.0,
            plume_swell_radius: 0.1,
            plume_drift: 1.0,
            hot_arc_suppression: 1.0,
            hot_drip: 1.0,
            modern_subduction_excess: 0.68,
            modern_subduction_width: 0.3,

            active_speed_eps: 1e-5,
            dt_myr: 2.0,

            ridge_crust_thickness: 7.0,
            composition_crust_type: false,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SimOptions {
    pub supercontinent_insulation: bool,
    /// Weld two plates only where their continents are closing on balance
    /// along all their contact, rather than wherever any stretch of it closes,
    /// and never two plates that broke apart less than `dome_push_duration`
    /// ago. A continent rifting apart by rotation closes at one end of the
    /// rift as it opens along the rest, and the mesh's zigzag makes some
    /// stretch of any fresh rift read as closing; either otherwise welds it
    /// shut at once.
    pub weld_on_net_closing: bool,
    /// Count slab pull as tension only where it pulls a plate from both sides.
    /// Off, the tension is the pull that cancels in the vector sum, which a
    /// zigzagging trench registers even when it all lies along one side. On,
    /// that artefact goes away, and with it most of the rifts this model was
    /// balanced to, so the default is off: the plates otherwise weld into one.
    pub two_sided_tension: bool,
    /// A boundary counts as quiet by its length-weighted mean relative speed
    /// rather than by its single fastest spot.
    pub quiet_by_mean_speed: bool,
    /// Passive margins collapse only on plates whose ocean floor is, on
    /// average, older than `passive_margin_collapse_age`: plates that have
    /// stopped renewing it, such as one covering the globe. A plate with a
    /// working ridge keeps an old coast or two without breaking up.
    pub collapse_stagnant_only: bool,
    /// Steer the continental area toward `continental_area_target`. Off, the
    /// area is whatever arcs, plumes, sediment and the fixed
    /// `continental_recycling_rate` leave.
    pub hold_continental_area: bool,
    /// Crust type follows what the crust is made of, not how thick it is:
    /// ocean floor turns continental once its basement is evolved enough
    /// (`continental_felsic_share`), arcs and plateaus are differentiated by the
    /// heat of arc magmatism, and thick buoyant ocean-type crust resists
    /// subduction and accretes. Experimental: in the first benches it either
    /// stalls ocean renewal (ocean floor ~700 Myr old, mantle never cooling) or
    /// makes almost no continent, depending on whether buoyant crust is allowed
    /// to jam trenches. See `todo/18a-crustTypeFromComposition.md`. Off, type is
    /// decided by thickness, as before.
    pub composition_crust_type: bool,
    /// A rift starts, and keeps stretching, only where the two pieces it would
    /// leave move apart under their own forces: each piece's drag, mantle
    /// carry, ridge push and slab pull, plus the push of any hot dome across
    /// the line. Off, slab tension against strength is enough, and many rifts
    /// break plates into pieces that ridge push and mantle drag hold together.
    pub rift_needs_separation: bool,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            supercontinent_insulation: true,
            weld_on_net_closing: true,
            two_sided_tension: false,
            quiet_by_mean_speed: false,
            collapse_stagnant_only: true,
            hold_continental_area: false,
            composition_crust_type: false,
            rift_needs_separation: true,
        }
    }
}
