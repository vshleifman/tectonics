---
name: "Rock cycle phase 4: metamorphism and exhumation"
overview: "Record the peak pressure and temperature of the rock now at each cell's surface, from how deep erosion has cut into the basement and the cell's geotherm, plus the burial of cover. Facies are read off the peak at display time. Output only."
todos:
    - id: exhumation
      content: Add CellData.exhumed (km of basement eroded, cumulative) and arc_exhumed (exhumed at the last arc event), written by erosion.rs and reset with arc_age
      status: pending
    - id: peak
      content: Add peak_temp and peak_pressure for the exposed rock, raised each tick from the geotherm at the exhumed depth and from collisional thickening; carried by gather, mixed by mass on transfers
      status: pending
    - id: cover-burial
      content: Lithified cover entering the basement carries the P-T of lithification_depth; deep burial under thick cover raises it further
      status: pending
    - id: facies
      content: Facies classifier (none, zeolite, greenschist, amphibolite, granulite, blueschist, eclogite) from peak P-T, in Rust and mirrored for the renderer
      status: pending
    - id: expose
      content: Metamorphic facies view, exhumation view, peak P-T in labels, facies boundary tuning, share of continental area by facies
      status: pending
    - id: verify
      content: Tests on exhumation bookkeeping and facies; dynamics fingerprint unchanged; report facies areas
      status: pending
isProject: false
---

# Rock cycle phase 4: metamorphism and exhumation

Part 4 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md). Depends on phase 3 (stripping cover before basement) and phase 2's `geotherm.rs`.

## Why

Many deposits are defined by metamorphism (orogenic gold) or destroyed by it, and erosion decides which can still be found. A porphyry is formed 1–5 km down: too little erosion and it is buried, too much and it is gone. What matters is the history of the rock now at the surface, not a column average.

## Model

The rock exposed at a cell was `exhumed` km below the surface before erosion removed what lay above it. While it was down there, its pressure was set by the depth and its temperature by the geotherm at that depth. Peak conditions are a running maximum:

- `P = rho_crust * g * depth`, in kbar. That is about 0.27 kbar per km, with `rho_crust` the current crust density. Phase 6 makes this follow composition.
- `T = geotherm::temperature_at(heat_flow[i], depth)`.
- **Approximation.** The rock was deepest before erosion began, but the temperature uses the heat flow at the time it is exhumed. Heat flow changes over tens of Myr, so this is close for one orogen and drifts for rock exhumed long after burial. Storing the geotherm at burial time would need a depth history per cell, which isn't worth it yet.
- **Collisional thickening.** Where crust thickens past its previous value, the exposed rock isn't buried; the mid-crust is. That only shows once erosion cuts down to it. So no separate thickening rule is needed: exhumation reaches deeper rock in thick crust, and that rock sees the geotherm at depth. This is the main simplification, and it keeps the field count low.
- **Subduction (blueschist and eclogite).** Accreted crust in advect pass 4 is mixed in carrying `subduction_peak_pressure` and a low temperature, reflecting the accretionary wedge. That is one `Mix` value on each of the two peak fields. If this proves noisy, drop it and record it as later work.

## Changes

- **New fields** in [sim-rs/src/cell_data.rs](sim-rs/src/cell_data.rs), all travelling with the crust:
  - `exhumed: Vec<f32>`: km of basement removed by erosion since the crust formed. Starts at 0 for new floor and plume crust.
  - `arc_exhumed: Vec<f32>`: the value of `exhumed` when `arc_age` was last reset, so `exhumed - arc_exhumed` is the erosion since the last arc magmatism.
  - `peak_temp: Vec<f32>` (°C) and `peak_pressure: Vec<f32>` (kbar) of the exposed rock.
- **Erosion.** Basement erosion in [sim-rs/src/erosion.rs](sim-rs/src/erosion.rs) adds the basement km it removes to `exhumed`, then raises the peaks to the conditions at the new depth.
- **Lithification.** In phase 3's lithification pass, basement entering from cover sets the peaks to at least the conditions at `lithification_depth`. If the cover is very thick, the peaks rise further as it is buried deeper.
- **Advection.** In [sim-rs/src/advect.rs](sim-rs/src/advect.rs):
  - the gather carries all four fields;
  - transfers mix `peak_*` and `exhumed` by mass;
  - `arc_exhumed` takes its value from whichever side `arc_age` came from;
  - lay new floor and the `RIFT` fill reset everything to 0.
- **Arc snapshot.** Wherever `arc_age` is reset, `arc_exhumed = exhumed`.
- **Facies.** `metamorphism.rs::facies(peak_temp, peak_pressure) -> Facies` with simplified boundaries as tuning, each referenced (for example to Spear 1993 or Winter's petrology text):
  - zeolite below about 200 °C;
  - greenschist about 300–500 °C;
  - amphibolite about 500–700 °C;
  - granulite above about 700 °C at moderate pressure;
  - blueschist at high pressure and low temperature;
  - eclogite above about 12 kbar.
- Add all four fields to the composition fingerprint and leave them out of the dynamics fingerprint.

## Exposure

- Pointers in [sim-rs/src/lib.rs](sim-rs/src/lib.rs) for the four fields.
- A "metamorphism" view: categorical facies colours with a legend. Mirror the classifier in TypeScript or export a computed `facies_ptr`; exporting avoids keeping two copies in step. Prefer exporting, updated in `refresh_surface`.
- An "exhumation" view: km eroded.
- Cell labels: facies, peak °C and kbar, exhumed km.
- Tuning: the facies boundaries and `subduction_peak_pressure`.
- Metrics: share of continental area by facies, and mean exhumation of continental crust.

## Verification

- **Unit tests:**
  - eroding 20 km off a 60 km orogen at 70 mW/m² exposes amphibolite-facies rock;
  - eroding 2 km exposes rock at or below zeolite;
  - `exhumed` only rises;
  - the arc snapshot resets with `arc_age`;
  - a facies table test at the boundary values.
- **Dynamics fingerprint** matches phase 3 bit for bit over 3000 Myr, at 1 and 4 threads.
- **Report:** facies areas over the run. Old cratons should show deeper exhumation than young arcs. If they don't, look at erosion and craton wear, not at the facies thresholds.
