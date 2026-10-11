---
name: "Mantle phase 2: lithosphere and column buoyancy"
overview: "Track the cold lithospheric mantle under every cell, make density crust-only, and decide subduction and slab pull from the buoyancy of the whole column, so hot-era thick crust resists subduction by itself."
todos:
    - id: field
      content: Add CellData.lithosphere and carry it through every advect copy, new floor, cohesion swap, initialise_crust and test world builder
      status: completed
    - id: growth
      content: Grow oceanic lithosphere with sqrt(age) up to plate_thickness; set continental from keel; thin it at rifts, plumes and delamination
      status: completed
    - id: buoyancy
      content: Make density crust-only (drop oceanic_density_from_age), add column_buoyancy, switch initiating_over, the trench choice in apply_elevation and metrics to buoyancy, and slab_pull to negative buoyancy
      status: completed
    - id: expose
      content: Export lithosphere_ptr, add a lithosphere view mode, mirror tuning; retune hot_arc_suppression (kept at 1.0, as 0.5 and 0.75 lock worlds into a few plates)
      status: completed
    - id: verify
      content: Update pinned tests (slab_pull age ramp, oceanic density), add buoyancy tests, run the 3000 Myr bench against the phase 1 baseline
      status: completed
isProject: false
---

# Mantle phase 2: lithosphere and column buoyancy

Part 2 of 5. Index: [planetary_heat_and_mantle_layers_f89b1aa1.plan.md](planetary_heat_and_mantle_layers_f89b1aa1.plan.md). Depends on phase 1 (`ridge_crust_thickness` and excess).

## Why

The per-cell `density` currently stands in for the whole lithosphere: oceanic density rises from 2.9 to 3.0 with age, but never reaches `mantle_density` (3.3). Real crust keeps the same density as it ages. What makes old ocean sink is the cold mantle lid beneath it, which thickens with age. Modelling that lid directly gives subduction a physical cause and lets phase 1's thicker hot-era crust delay subduction without the `hot_arc_suppression` workaround.

## Changes

- [sim-rs/src/cell_data.rs](sim-rs/src/cell_data.rs): `lithosphere: Vec<f32>` (km), which travels with the crust. Carry it wherever the other travelling fields are copied or swapped:
  - [sim-rs/src/advect.rs](sim-rs/src/advect.rs): the copy loops near lines 738 and 1311, new floor near 761, and the type reset near 1365;
  - the swap in [sim-rs/src/cohesion.rs](sim-rs/src/cohesion.rs);
  - `initialise_crust` in [sim-rs/src/plates.rs](sim-rs/src/plates.rs);
  - the test builders in [sim-rs/src/world.rs](sim-rs/src/world.rs).
- How thick it is, set once per step in `apply_crust_lifecycle` or a new small pass:
  - ocean: `min(lith_growth * sqrt(age), plate_thickness)`, with `plate_thickness` about 100 km;
  - continent: relax toward `continental_lithosphere + keel * keel_depth` (about 150 km, rising to 250 km for cratons);
  - thinning: rift cells (`rift_side != 0`) and plume centres relax toward `rift_lithosphere`, and delamination in [sim-rs/src/recycling.rs](sim-rs/src/recycling.rs) removes it.
- [sim-rs/src/elevation.rs](sim-rs/src/elevation.rs): `column_buoyancy(t, data, i)`. That's the crust's `thickness * (mantle_density - density)` plus the lid's `lithosphere * (mantle_density - lid_density)`. `lid_density = mantle_density * (1 + thermal_expansion * lid_delta_t / 2) - keel * keel_depletion`. A negative result means the column sinks.
- `density` becomes crust-only. Oceanic is `oceanic_density` (2.9) at every age. Remove `oceanic_density_from_age`, `oceanic_density_young`, `oceanic_density_old` and `rift_density`, and change every caller found in phase research (crust_lifecycle, world, plates, advect).
- Polarity: compare `column_buoyancy` instead of `density` in `initiating_over` in [sim-rs/src/slabs.rs](sim-rs/src/slabs.rs), the trench and arc choice in `apply_elevation`, and the "which side goes down" check in [sim-rs/src/metrics.rs](sim-rs/src/metrics.rs). Continental crust still never subducts.
- `slab_pull` becomes `clamp01(-buoyancy / slab_pull_saturation_buoyancy)`, replacing `slab_pull_onset_age` and `slab_pull_saturation_age`. The crust-reversion path in `crust_lifecycle.rs` that currently clamps age to `slab_pull_onset_age` instead clamps to the age at which buoyancy is neutral.
- Keep thermal subsidence in `baseline_elevation` as it is. It already captures the lid's effect on depth.
- With buoyancy handling it, retune `hot_arc_suppression` down.

## Exposure

`lithosphere_ptr` in [sim-rs/src/lib.rs](sim-rs/src/lib.rs), a "lithosphere" view mode, and new tuning keys in [src/sim/tuning.ts](src/sim/tuning.ts) and [src/ui/controls.ts](src/ui/controls.ts), with the removed density keys dropped. The existing "density" view now shows crust density only. Either switch it to buoyancy or relabel it.

## Verification

- Unit tests:
  - ocean becomes negatively buoyant at about 20–30 Myr with default crust;
  - with thicker crust (phase 1's hot era) that age rises;
  - a cratonic keel is buoyant;
  - polarity picks the older of two ocean plates.
- Update the tests that pin the `slab_pull` age ramp and `oceanic_density_from_age`.
- 3000 Myr bench against the phase 1 baseline. Watch subduction polarity, the number of trenches and plate speeds.
