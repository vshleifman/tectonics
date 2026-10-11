---
name: "Mantle phase 1: global heat budget"
overview: "Replace the fixed mantle_excess exponential with a planetary heat budget driven by the map: radiogenic decay and core heat in, surface heat flow out. Mantle temperature then controls plumes, arcs, drips, cratons, mantle viscosity, ridge height and the thickness of new ocean crust."
todos:
    - id: heat-module
      content: Add sim-rs/src/heat.rs with PlanetHeat (mantle and core temperature, radiogenic decay of four isotopes, parallel surface heat flow sum, core flux, smoothed response), owned by Sim and reset with mantle and primordial
      status: pending
    - id: replace-excess
      content: Run the heat pass at the end of evolve_crust; switch every mantle_excess(t, elapsed) caller to the stored excess; effective_tuning takes excess; remove primordial_cooling_time in Rust and TypeScript
      status: pending
    - id: heat-effects
      content: In effective_tuning, scale mantle_drag and motion_response_time by viscosity, raise ridge_elevation and set ridge_crust_thickness with excess; use ridge_crust_thickness for new floor, rift fill and crust_lifecycle new crust
      status: pending
    - id: expose
      content: Export heat_flow_ptr and the heat metrics; mirror the tuning in tuning.ts and controls; diagnostics rows; a heat flow view mode; bench overrides and columns
      status: pending
    - id: calibrate
      content: Calibrate so default runs stay close to the current 3000 Myr bench; check about 46 TW today and 50-100 K/Gyr cooling; update pinned tests
      status: pending
isProject: false
---

# Mantle phase 1: global heat budget

Part 1 of 5. Index: [planetary_heat_and_mantle_layers_f89b1aa1.plan.md](planetary_heat_and_mantle_layers_f89b1aa1.plan.md). Nothing comes before this phase. Every later phase reads `PlanetHeat`.

## Why first

Today the mantle's excess temperature is a fixed curve that the plates never affect ([sim-rs/src/primordial.rs](sim-rs/src/primordial.rs) `mantle_excess`). Every later phase needs a mantle temperature that responds to the surface, so this replaces the curve outright. That changes every default run, so calibration is part of this phase.

## Changes

- New [sim-rs/src/heat.rs](sim-rs/src/heat.rs): `PlanetHeat { mantle_temp, core_temp, last_surface_tw, last_radiogenic_tw, last_core_tw }`, owned by `Sim` in [sim-rs/src/world.rs](sim-rs/src/world.rs) and reset in `reassign_plates` together with `mantle.reset()` and `primordial.reset()`.
- Each step it applies `C_m dT/dt = H_radio + Q_core - Q_surface`:
  - `H_radio`: U-238 (4.47 Gyr half-life), U-235 (0.70), Th-232 (14.0) and K-40 (1.25), each weighted by its share of today's heat. Evaluated at `start_age_ga - elapsed / 1000`, and scaled so that output today is `radiogenic_heat_now` (about 20 TW).
  - `Q_surface`: a `par::sum` over cells. Ocean floor gives `490 / sqrt(age)` mW/m², floored at about 48. Continents give about 65 mW/m², reduced by `keel` and by `mantle_heat`, so insulated heat is not lost. Convert from unit-sphere area to Earth's area to get TW.
  - `Q_core`: `core_coupling * (core_temp - mantle_temp)`, with the core cooling by the same flux through its own heat capacity.
  - Relax the actual temperature change toward the computed one over `heat_response_time`, to damp feedback between plate speed and heat loss.
- `excess() = (mantle_temp - modern_mantle_temp) / excess_scale`, in today's `mantle_excess` units (1 is roughly 250 K hotter).
- Add the heat pass at the end of `evolve_crust`, once age and crust type are final. It is cheap and doesn't need a timing pass of its own, so fold its time into pass 6.
- Switch every caller to the stored excess:
  - `effective_tuning` becomes `effective_tuning(t, elapsed, excess)`. It still needs `elapsed` for the ridge-weight ramp.
  - `Primordial::update` takes `excess` instead of computing it.
  - `update_cratons` in [sim-rs/src/craton.rs](sim-rs/src/craton.rs) takes `excess`.
  - `Sim::mantle_excess` returns `heat.excess()`.
  - Remove `mantle_excess` and `primordial_cooling_time`. `primordial_heat` becomes the starting excess.
- New temperature effects in `effective_tuning`:
  - `mantle_drag` and `motion_response_time` multiplied by `exp(-viscosity_sensitivity * excess)`;
  - `ridge_elevation += ridge_heat_uplift * excess`;
  - `ridge_crust_thickness = oceanic_thickness * (1 + melt_gain * excess)`, as a new derived field on `Tuning`, set only here.
- Use `ridge_crust_thickness` for new floor in `lay_new_floor`, the rift fill in [sim-rs/src/advect.rs](sim-rs/src/advect.rs), and new ocean crust in [sim-rs/src/crust_lifecycle.rs](sim-rs/src/crust_lifecycle.rs). `baseline_elevation` keeps `oceanic_thickness` as its reference, so thicker crust sits higher. Make sure the mass ledger counts the thickness actually laid.

## Exposure

- [sim-rs/src/lib.rs](sim-rs/src/lib.rs): `heat_flow_ptr`, a per-cell `f32` buffer that `PlanetHeat` fills each step.
- Metrics: `surface_heat_tw`, `radiogenic_tw`, `core_heat_tw`, `mantle_temp`, `core_temp`. Keep `mantle_excess`.
- [src/sim/tuning.ts](src/sim/tuning.ts), [src/ui/controls.ts](src/ui/controls.ts), [src/sim/metrics.ts](src/sim/metrics.ts), [src/ui/diagnostics.ts](src/ui/diagnostics.ts): new knobs, with `primordialCoolingTime` removed, and a "Heat budget" diagnostics group.
- A "heat flow" view mode in `settingsStore.ts`, `colour.ts`, `CellRenderer.ts` and the worker protocol.
- `bench.rs`: overrides for the new keys, plus heat columns.

## Verification

- Unit tests:
  - radiogenic output at 4.5 Ga is about 4× today's;
  - young floor loses more heat than old;
  - a world entirely of old floor cools more slowly than one of young floor;
  - insulated continent reduces heat loss;
  - excess falls over a default run.
- Update `the effective_tuning` ridge-weight test in `world.rs` and any test that relies on `mantle_excess`.
- Calibrate against the current 3000 Myr bench baseline: continental share, craton share, plume-made versus arc-made crust and rift count should stay close. Modern heat flow should come out at about 46 TW, and cooling at about 50–100 K per Gyr.
