---
name: "Mantle phase 3: upper-mantle temperature field"
overview: "Give each of the 642 mantle sites a temperature anomaly around the global mantle temperature: cooled by slabs, warmed under continents, diffusing sideways. It drives upwelling and links per-cell insulation to the global budget."
todos:
    - id: field
      content: Add upper_temp per site and a site neighbour list in MantleFlow; update it from slab flux, continental cover, diffusion and relaxation
      status: completed
    - id: sampling
      content: Add upper_temp_at(i), blending a cell's site with its neighbour sites so the coarse grid does not show as blocks
      status: completed
    - id: coupling
      content: Drive MantleFlow source from positive upper_temp; scale mantle_heat accumulation in rifting.rs by the site anomaly; feed the mean anomaly back into PlanetHeat
      status: completed
    - id: expose
      content: Export a per-cell mantle temperature buffer and view mode; mirror tuning; metrics for the anomaly range
      status: completed
    - id: verify
      content: Unit tests for cooling, insulation and diffusion; bench supercontinent cycle timing against the phase 2 baseline (21 seeds at level 6 match phase 2 on plates, ocean age, rifts and collapses; supercontinent share 6% vs 12%, within noise)
      status: completed
isProject: false
---

# Mantle phase 3: upper-mantle temperature field

Part 3 of 5. Index: [planetary_heat_and_mantle_layers_f89b1aa1.plan.md](planetary_heat_and_mantle_layers_f89b1aa1.plan.md). Depends on phase 1 (`PlanetHeat.mantle_temp`).

## Why

[sim-rs/src/mantle.rs](sim-rs/src/mantle.rs) remembers where slabs sink but has no notion of temperature, and per-cell `mantle_heat` in [sim-rs/src/rifting.rs](sim-rs/src/rifting.rs) is a 0–1 counter that isn't tied to anything global. A temperature field on the existing sites gives both a common source and is what phases 4 and 5 read.

## Changes

- `MantleFlow` gains `upper_temp: Vec<f64>` (K anomaly) and `site_nbrs: Vec<Vec<u32>>`. Build the neighbour list in `MantleFlow::new` from the mesh: two sites are neighbours if any of their cells touch.
- In `MantleFlow::update`, after the sink update, change `upper_temp` per site by:
  - `- slab_cooling * flux[s] / site_area[s]`. As built this is this step's subduction, not the remembered `sink`: the field has its own memory, and cooling from an 800 Myr `sink` kept supercontinents over cold mantle long after their trenches died, so they stopped rifting and worlds merged into one plate;
  - `+ site_insulation * continental_share[s]`, where the share is accumulated from cells through `site_of`. Pass `CellData` in, as it already is;
  - `+ site_diffusion * (mean of neighbours - own)`;
  - `- upper_temp[s] / upper_relax_time`.
  Remove the area-weighted mean, then clamp to plus or minus `upper_temp_max`. `PlanetHeat.mantle_temp` is the mean, so a field that drifts cold during the busy early era would count slab cooling twice (it is already in the surface heat loss).
- `upper_temp_at(i)`: a weighted blend of the cell's site and its site neighbours, by inverse distance from the cell centre. Precompute the weights per cell in `new` so this is a lookup.
- Coupling:
  - `source[s]` comes from `upwelling_gain * max(upper_temp[s] / upper_temp_max, 0) * site_area`, replacing the sum over per-cell `mantle_heat`. Dividing by `upper_temp_max` keeps the gain's 0–1 scale.
  - In `insulate` in [sim-rs/src/rifting.rs](sim-rs/src/rifting.rs), multiply the accumulation rate by `1 + insulation_site_gain * upper_temp_at(i) / upper_temp_max`, so the per-cell rift trigger follows the site field. That needs `insulate` and `update_rifting` to take `&MantleFlow`.
  - `PlanetHeat` (phase 1) reads the area-weighted mean anomaly as a correction to surface heat loss, so heat held under continents counts once and only once. As built, each cell's heat flow uses `mantle_temp + anomaly - mean`, so the anomalies move heat loss around without changing the total.
- The heat pass order stays as in phase 1. `mantle.update` already runs in `drive_plates`, before forces.

## Exposure

A per-cell `mantle_temp` `f32` buffer filled from `upper_temp_at` (only when the view needs it, or every tick if that's cheap), plus `mantle_temp_ptr`, a "mantle temperature" view mode, new tuning keys, and min and max anomaly metrics.

## Verification

- Unit tests:
  - a steady sink cools its site and its neighbours;
  - a large still continent warms the sites beneath it;
  - an isolated anomaly diffuses and decays;
  - `upper_temp_at` is continuous across site borders, with no step bigger than a set share between neighbouring cells.
- Bench against phase 2: supercontinent assembly and breakup timing and rift counts, and that `upwelling_gain` still behaves as before at default settings.
