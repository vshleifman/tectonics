---
name: "Mantle phase 5: dynamic topography"
overview: "Let the mantle push the surface up over hot upwellings and pull it down over sinking slabs, by up to about 1.5 km, as an extra term in the elevation baseline."
todos:
    - id: compute
      content: Compute a per-cell dyn_topo buffer from upper_temp_at and the sampled sink, clamped to dyn_topo_max, in Sim after mantle.update
      status: completed
    - id: apply
      content: Pass the buffer into apply_elevation and add it to the baseline in the relaxation loop
      status: completed
    - id: expose
      content: Export dyn_topo_ptr and a dynamic topography view mode; mirror tuning
      status: completed
    - id: verify
      content: "Unit tests for sign and clamp; zero gain reproduces the phase 4 fingerprint exactly; bench over 14 layouts at level 6 over 3000 Myr: plates identical, land above sea level 9.0% to 12.9%, uplift 2.1% of the globe assembled vs 0.8% dispersed, relief within ±1.5 km"
      status: completed
isProject: false
---

# Mantle phase 5: dynamic topography

Part 5 of 5. Index: [planetary_heat_and_mantle_layers_f89b1aa1.plan.md](planetary_heat_and_mantle_layers_f89b1aa1.plan.md). Depends on phase 3 (`upper_temp_at`). Phase 4 isn't needed, but pile cooling will show up here once it lands.

## Why

Mantle flow lifts and drops the surface over regions thousands of km across, typically by 1–2 km. Supercontinents rise over the heat they trap, and continent above subduction zones sinks and floods. It's the most visible way of showing mantle activity on the map, and the sites already have what it needs.

## Changes

- New per-cell `dyn_topo: Vec<f32>` on `Sim` in [sim-rs/src/world.rs](sim-rs/src/world.rs). It's a function of position, not crust, so it doesn't belong in `CellData` and isn't advected.
- After `mantle.update` in `drive_plates`, fill it with `clamp(dyn_topo_gain * upper_temp_at(i) - sink_topo_gain * sink_at(i), plus or minus dyn_topo_max)`. Add a `sink_at(i)` to `MantleFlow` that uses the same smooth blend as `upper_temp_at`. As built:
  - `sink_at` is per unit area (share of the area over the cell per Myr), and the term uses `sink_at(i) - mean_sink()`. The remembered sink has an 800 Myr memory and is broad (cells peak near 0.035/Myr but average ~0.012 everywhere), so taken raw it lowered the whole globe by ~0.25 km, which is only a sea-level shift. Subtracting the mean matches the flow model, where each downwelling is balanced by gentle upwelling everywhere else;
  - the formula is a pure `dynamic_topography(t, temp, sink)` in [sim-rs/src/elevation.rs](sim-rs/src/elevation.rs), so its sign and clamp are tested on their own;
  - defaults are `dyn_topo_gain = 0.01` km/K (150 K of trapped heat lifts ~1.5 km), `sink_topo_gain = 20` km per share/Myr and `dyn_topo_max = 1.5` km. At 0.005 km/K the relief peaked under 1 km and continents rose only ~0.15 km while assembled.
- `apply_elevation` in [sim-rs/src/elevation.rs](sim-rs/src/elevation.rs) takes `dyn_topo: &[f32]` and uses `base + dyn_topo[i]` in the relaxation loop. `baseline_elevation` itself stays pure, so its tests are unaffected.
- `dyn_topo_gain = 0` gives today's behaviour, which keeps old tests and comparisons simple. As built, both gains at 0 give the phase 4 fingerprint bit for bit; `dyn_topo` is left out of the fingerprint for that reason (its effect is already hashed through elevation).

## Exposure

`dyn_topo_ptr` in [sim-rs/src/lib.rs](sim-rs/src/lib.rs), a "dynamic topography" view mode with a diverging ramp in `colour.ts`, and new tuning keys with control descriptions. As built: view mode `dyn-topo` (purple drag through neutral to brown lift, ±1.5 km), a "Dynamic topography" slider group, the value in the cell labels, metrics `dynTopoLow`, `dynTopoHigh` and `landArea` (share of the globe above 0 km), a diagnostics chart plus an "Above sea level" series on the continental area chart, and the bench `heat` line gains `dyn`, `dyn_cont`, `dyn_ocean` and `land`.

## Verification

- Unit tests: a hot site raises the cells above it, a sink lowers them, the clamp holds, and with zero gain elevation matches the phase 4 result exactly. As built, a world test also checks that switching it on moves elevation and no other field.
- Bench: land share above sea level over a supercontinent cycle should rise while it's assembled and fall after breakup, and the size of the relief stays within `dyn_topo_max`. As built (`target/p5/ensemble5.py`, 14 layouts at level 6 over 3000 Myr, each run on and off):
  - elevation feeds nothing back, so plates, rifts, cratons and ocean age match in all 14 pairs;
  - land above sea level at the end goes from 9.0% to 12.9% (mean);
  - over 1000–3000 Myr, dynamic topography lifts 2.1% of the globe above sea level in the most assembled third of samples and 0.8% in the most dispersed; correlation of that uplift with the largest continent's share has median +0.59 (3 of 14 layouts negative);
  - relief reaches −1.5 km over trenches in every run (the clamp) and +1.45 km at most.
