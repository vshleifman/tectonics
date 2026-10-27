//! Plate tectonics simulation core, compiled to WASM and driven by the
//! simulation worker through [`World`]. It began as a port of the TypeScript
//! simulation and was checked against it metric for metric before that was
//! removed.
//!
//! Per-cell passes with no ordering dependency run through [`par`], which
//! spreads them over every core with the `parallel` feature on, giving the
//! same world whatever the thread count.
#![allow(clippy::needless_range_loop)]

pub mod advect;
pub mod arcs;
pub mod boundaries;
pub mod cell_data;
pub mod cohesion;
pub mod craton;
pub mod crust_lifecycle;
pub mod elevation;
pub mod erosion;
pub mod heat;
pub mod lithosphere;
pub mod mantle;
pub mod math;
pub mod mesh;
pub mod metrics;
pub mod par;
pub mod passive_margins;
pub mod plate_forces;
pub mod plate_lifecycle;
pub mod plates;
pub mod primordial;
pub mod recycling;
pub mod registry;
pub mod rifting;
pub mod rng;
pub mod scratch;
pub mod shape;
pub mod slabs;
pub mod sutures;
pub mod timer;
pub mod tuning;
pub mod world;

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// `initThreadPool(threads)` on the JavaScript side: starts the rayon pool's
/// web workers. Must be awaited before the first world is built.
#[cfg(all(feature = "parallel", target_arch = "wasm32"))]
pub use wasm_bindgen_rayon::init_thread_pool;

/// Threads the simulation's passes run on: 1 without the `parallel` feature.
#[wasm_bindgen]
pub fn thread_count() -> u32 {
    #[cfg(feature = "parallel")]
    {
        rayon::current_num_threads() as u32
    }
    #[cfg(not(feature = "parallel"))]
    {
        1
    }
}

/// A world, as seen from JavaScript. Mesh getters copy (they are read once per
/// world); per-tick fields are exposed as pointers into WASM memory, which the
/// worker views and copies out of straight after each call.
#[wasm_bindgen]
pub struct World {
    sim: world::Sim,
}

fn to_js<T: Serialize>(value: &T) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true);
    value.serialize(&serializer).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
impl World {
    #[wasm_bindgen(constructor)]
    pub fn new(level: u32) -> World {
        World {
            sim: world::Sim::new(level),
        }
    }

    /// A world rebuilt from [`World::save_state`], or an error saying why the
    /// bytes were refused (another build wrote them, or they are damaged).
    #[wasm_bindgen(js_name = fromState)]
    pub fn from_state(bytes: &[u8]) -> Result<World, JsError> {
        Ok(World {
            sim: world::Sim::from_state(bytes).map_err(|e| JsError::new(&e))?,
        })
    }

    /// The whole world as bytes, for [`World::from_state`].
    pub fn save_state(&self) -> Vec<u8> {
        self.sim.save_state()
    }

    pub fn level(&self) -> u32 {
        self.sim.level()
    }

    pub fn cell_count(&self) -> u32 {
        self.sim.mesh.cell_count as u32
    }

    pub fn arc_count(&self) -> u32 {
        self.sim.mesh.arc_count() as u32
    }

    pub fn mesh_centres(&self) -> Vec<f32> {
        self.sim.mesh.centres.clone()
    }

    pub fn mesh_areas(&self) -> Vec<f32> {
        self.sim.mesh.areas.clone()
    }

    pub fn mesh_neighbour_offsets(&self) -> Vec<i32> {
        self.sim.mesh.nbr_off.clone()
    }

    pub fn mesh_neighbour_list(&self) -> Vec<i32> {
        self.sim.mesh.nbr.clone()
    }

    pub fn mesh_neighbour_arcs(&self) -> Vec<i32> {
        self.sim.mesh.nbr_arc.clone()
    }

    pub fn mesh_poly_offsets(&self) -> Vec<i32> {
        self.sim.mesh.poly_off.clone()
    }

    pub fn mesh_poly_corners(&self) -> Vec<f32> {
        self.sim.mesh.poly_corners.clone()
    }

    pub fn mesh_junction_pos(&self) -> Vec<f32> {
        self.sim.mesh.junction_pos.clone()
    }

    pub fn mesh_arc_ends(&self) -> Vec<i32> {
        self.sim.mesh.arc_ends.clone()
    }

    pub fn mesh_arc_cells(&self) -> Vec<i32> {
        self.sim.mesh.arc_cells.clone()
    }

    pub fn mesh_junction_arc_offsets(&self) -> Vec<i32> {
        self.sim.mesh.junction_arc_off.clone()
    }

    pub fn mesh_junction_arcs(&self) -> Vec<i32> {
        self.sim.mesh.junction_arcs.clone()
    }

    pub fn reassign_plates(&mut self, cracked: &[u8], init_crust: bool) -> u32 {
        self.sim.reassign_plates(cracked, init_crust) as u32
    }

    pub fn tick(&mut self) {
        self.sim.tick();
    }

    pub fn set_tuning(&mut self, value: JsValue) -> Result<(), JsValue> {
        self.sim.tuning = serde_wasm_bindgen::from_value(value)?;
        Ok(())
    }

    /// Recompute sea level and elevation from the current state, taking no
    /// time, so a change to the ocean volume shows while paused.
    pub fn refresh_surface(&mut self) {
        self.sim.refresh_surface();
    }

    pub fn set_options(&mut self, value: JsValue) -> Result<(), JsValue> {
        self.sim.options = serde_wasm_bindgen::from_value(value)?;
        Ok(())
    }

    pub fn elevation_ptr(&self) -> *const f32 {
        self.sim.front().elevation.as_ptr()
    }

    pub fn plate_id_ptr(&self) -> *const i32 {
        self.sim.front().plate_id.as_ptr()
    }

    pub fn crust_type_ptr(&self) -> *const u8 {
        self.sim.front().crust_type.as_ptr()
    }

    pub fn age_ptr(&self) -> *const f32 {
        self.sim.front().age.as_ptr()
    }

    pub fn density_ptr(&self) -> *const f32 {
        self.sim.front().density.as_ptr()
    }

    pub fn thickness_ptr(&self) -> *const f32 {
        self.sim.front().thickness.as_ptr()
    }

    pub fn keel_ptr(&self) -> *const f32 {
        self.sim.front().keel.as_ptr()
    }

    /// Thickness (km) of the mantle lid beneath each cell.
    pub fn lithosphere_ptr(&self) -> *const f32 {
        self.sim.front().lithosphere.as_ptr()
    }

    /// Share of each cell's crust that is reworked sediment.
    pub fn sediment_ptr(&self) -> *const f32 {
        self.sim.front().sediment_share.as_ptr()
    }

    /// Heat flow out through each cell (mW/m^2), as of the last tick.
    pub fn heat_flow_ptr(&self) -> *const f32 {
        self.sim.heat().heat_flow.as_ptr()
    }

    /// Upper-mantle temperature beneath each cell (K), above or below the
    /// planet's mantle temperature, as of the last tick.
    pub fn mantle_temp_ptr(&self) -> *const f32 {
        self.sim.mantle().cell_temps().as_ptr()
    }

    /// Slab piled in the lower mantle beneath each cell, as a share of the
    /// area over it, as of the last tick.
    pub fn lower_slab_ptr(&self) -> *const f32 {
        self.sim.mantle().cell_piles().as_ptr()
    }

    /// Dynamic topography (km) over each cell, as of the last tick.
    pub fn dyn_topo_ptr(&self) -> *const f32 {
        self.sim.dyn_topo().as_ptr()
    }

    pub fn cell_kind_ptr(&self) -> *const u8 {
        self.sim.boundaries().cell_kind.as_ptr()
    }

    pub fn arc_kind_ptr(&self) -> *const i8 {
        self.sim.boundaries().arc_kind.as_ptr()
    }

    pub fn plate_count(&self) -> u32 {
        self.sim.plate_count() as u32
    }

    pub fn plate_omegas(&self) -> Vec<f64> {
        self.sim.plate_omegas()
    }

    pub fn metrics(&self) -> JsValue {
        to_js(&self.sim.metrics())
    }

    pub fn summaries(&self) -> JsValue {
        to_js(&self.sim.summaries())
    }

    pub fn live_mass(&self) -> f64 {
        self.sim.live_mass()
    }

    pub fn mass_drift(&self) -> f64 {
        self.sim.mass_drift()
    }

    pub fn elapsed_myr(&self) -> f64 {
        self.sim.elapsed()
    }

    pub fn pass_timings(&self) -> Vec<f64> {
        self.sim.timings().to_vec()
    }
}
