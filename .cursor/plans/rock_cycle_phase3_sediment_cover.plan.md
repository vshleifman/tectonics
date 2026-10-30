---
name: "Rock cycle phase 3: sediment cover and source-aware erosion"
overview: "Split each cell's crust into basement and a sediment cover on top. Erosion strips cover before basement and carries the eroded rock's composition downhill; deposits build cover; deep cover lithifies into basement metasediment. Adds the climate module and its two erosion seams, both neutral for now."
todos:
    - id: cover
      content: Add CellData.cover (km, part of thickness) and a cover lane block (COVER_FELSIC_CLASTIC, COVER_MAFIC_CLASTIC); basement LANE_SEDIMENT becomes LANE_METASEDIMENT
      status: completed
    - id: advect
      content: Gather scales cover with thickness; every weld, accretion, spill and surplus transfer moves the incoming cover into the receiving basement as metasediment; removal takes basement and cover in proportion
      status: completed
    - id: erosion
      content: Make erosion.rs strip cover then basement, carry load as a per-lane composition, and deposit into cover through deposit_lanes
      status: completed
    - id: lithify
      content: Convert cover below lithification_depth to basement metasediment each tick
      status: completed
    - id: climate
      content: Add climate.rs (Climate with optional temperature and runoff, ClimateSample, weathering_rate, deposit_lanes), held by Sim and serialised with serde default
      status: completed
    - id: expose
      content: Cover thickness view, sediment view kept meaning column sediment share, cover lanes in labels, lithification_depth tuning, cover metrics
      status: completed
    - id: verify
      content: Per-lane erosion ledger, cover never exceeds thickness, dynamics fingerprint unchanged with hot_drip = 0; report drift with drip on
      status: completed
isProject: false
---

# Rock cycle phase 3: sediment cover and source-aware erosion

Part 3 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md). Depends on phase 2.

## Why

A single mixed column can't tell thin sediment on thick basement from nothing at all. Erosion should strip the youngest rock first, and sediment should take on the makeup of what was eroded. A granite range sheds different sediment from a basalt plateau. Most sedimentary resources, and the metamorphism in phase 4, depend on that difference.

## Changes

- **Cover.** [sim-rs/src/cell_data.rs](sim-rs/src/cell_data.rs) gets `cover: Vec<f32>`: km of sediment cover. It is part of `thickness`, with `0 ≤ cover ≤ thickness`, so isostasy, elevation and every mass ledger are untouched.
  - A second `Lanes` block, `cover_lanes`, partitions the cover: `COVER_FELSIC_CLASTIC` and `COVER_MAFIC_CLASTIC`, by source rock.
  - Lanes for carbonate, evaporite and organic cover are reserved for climate. See the index.
- **Basement lanes.** The phase 2 lanes now describe the basement only (`thickness - cover`). `LANE_SEDIMENT` becomes `LANE_METASEDIMENT`: sediment that has been buried or deformed into basement.
  - `sediment_share` becomes a derived value: `(cover + metasediment * (thickness - cover)) / thickness`. The sediment view and the `sediment_share` metric keep their meaning.
  - That derived value is what `hot_drip` reads.
- **Advection.** The cover follows two rules in [sim-rs/src/advect.rs](sim-rs/src/advect.rs):
  - The rigid gather carries cover intact, scaled by the same area ratio as `thickness`.
  - Every transfer between cells moves the incoming cell's cover into the receiving basement as metasediment: welds, accretion, spill, surplus placement and the pulls. Crust moved that way has been deformed, as in an accretionary wedge or a suture.
  - This keeps the cover logic out of the deepest parts of advect. The weld and spill tallies only need the metasediment share of what they carry.
- **Removal.** Subduction, margin erosion, recycling, drip and rift thinning take basement and cover in proportion. Clamp `cover ≤ thickness` after each pass that thins.
- **Erosion.** Changes to [sim-rs/src/erosion.rs](sim-rs/src/erosion.rs):
  - **Stripping.** A cell's `take` comes off its cover first, then its basement. Basement erosion carries the basement lanes: felsic, or metasediment, goes to felsic clastic and mafic to mafic clastic.
  - **Load composition.** `load: Vec<f64>` gains a per-cell `[f64; COVER_LANES]` mass alongside it, passed downhill with the load.
  - **Deposition.** Deposits go into `cover` with `cover_lanes` mixed by mass. Their makeup comes from `deposit_lanes(setting, climate.sample(i), &load_lanes)`, which with no climate returns the load's own makeup.
  - **Climate multiplier.** `take` is multiplied by `climate::weathering_rate(climate.sample(i))`, which is 1.0 with no climate.
  - The order of the descending sweep and of every sum stays as it is.
- **Lithification.** In a new small pass, or at the end of `erode_and_deposit`, cover deeper than `lithification_depth` (about 2–3 km, where diagenesis turns sediment into rock) moves into the basement as metasediment each tick.
- **Climate.** A new [sim-rs/src/climate.rs](sim-rs/src/climate.rs):
  - `pub struct Climate { temperature: Option<Vec<f32>>, runoff: Option<Vec<f32>> }`, with `Default` being empty.
  - `sample(i) -> Option<ClimateSample>`.
  - `weathering_rate(Option<ClimateSample>) -> f64`.
  - `deposit_lanes(Setting, Option<ClimateSample>, &[f64; COVER_LANES]) -> [f64; COVER_LANES]`. `Setting` is shelf or slope or basin, which `erode_and_deposit` already knows from `ring` and `keep`.
  - Held by `Sim`, added to `State` with `#[serde(default)]`, and passed to `erode_and_deposit`.

## Exposure

- `cover_ptr` and the cover lanes through `lane_ptr` (give the cover block its own lane ids or a `cover_lane_ptr`).
- A "cover" view showing sediment cover thickness (km). The "sediment" view is unchanged in meaning.
- [src/render/CellLabels.ts](src/render/CellLabels.ts): a cover km line and a felsic/mafic clastic split.
- Tuning: `lithification_depth`.
- Metrics:
  - global cover volume;
  - mean cover on continents and on ocean floor;
  - the share of eroded mass that came from cover versus basement.

## Verification

- **Erosion ledger.** Extend `what_is_eroded_is_deposited_and_the_ledger_closes` so that each cover lane's deposited mass equals the eroded mass of its sources.
- **New tests:**
  - a granite range sheds felsic clastic and a basalt plateau sheds mafic clastic;
  - erosion on a cell with cover removes cover before basement;
  - cover deeper than `lithification_depth` becomes metasediment;
  - `cover ≤ thickness` holds everywhere after a tick;
  - a welded cell's cover arrives as metasediment.
- **Dynamics fingerprint.** It must match phase 2 bit for bit with `hot_drip = 0`. With drip on, `sediment_share` now falls when cover is stripped, so drip can move slightly. Report the difference in the bench metrics instead of hiding it.
- **Climate seams.** With `Climate::default()`, results are identical to calling neither seam.

## As built

**The cover is two more lanes, not a thickness field.** The plan had `cover: Vec<f32>` (km) plus a second lane block, with the basement lanes redefined to exclude cover. That needs new handling wherever thickness changes. As built, the cover is two more column lanes, `LANE_COVER_FELSIC` and `LANE_COVER_MAFIC` (shares of the crust by mass, so cover km = `(felsic + mafic cover) x thickness`). `LANE_SEDIMENT` stays what it was, all the sediment in the column, and the cover is the part of it still lying on top:

- Metasediment is implied: `sediment x thickness - cover km`. `sediment_share` needs no derived buffer, the sediment view and metric keep their meaning, and `hot_drip` reads the same lane in the same way.
- Every place that thins or thickens crust leaves shares alone, which is the proportional removal the plan wanted, so none of them needed a change. The advect lane loops, welds and the mixing helpers carry the new lanes without further code.
- Cover on crust that is welded, spread or pulled across is dropped: those transfers skip the cover lanes (`is_cover`), so it arrives as sediment in the basement. The rigid gather carries cover whole.
- Arcs, plume raising and erosion deposits dilute the cover like any other part of the crust; drip scales it with sediment.

**Other points.**
- The climate seams are in `climate.rs` and are identity for now: `weathering_rate` gives 1 and `deposit_lanes` returns what arrived. `Sim.climate` is saved with the world.
- The erosion ledger is checked as an identity: cover mass equals the basement that has been shed (`the_cover_ledger_closes`). Per-lane composition is checked by the granite and basalt tests.
- There is no dedicated test that welded cover turns to sediment. The cover-never-exceeds-sediment invariant is checked over 150 ticks from the hot start, but it would not catch a weld that carried cover by mistake.
- `strip` leaves the plume lane alone, as before.

## Verified

- With `hot_drip=0` the dynamics fingerprint matches phase 2 bit for bit: `3123ea13fd8f9417` (level 5, 300 ticks), `1449ee0f6b4115fd` (level 6, 1000) and `cd30f0862325b047` (level 6, 1500). At 1 and 4 threads.
- With drip on, a temporary switch restoring the old sediment lane after each strip reproduced phase 2's default fingerprint exactly (`810062435eaa1bf0`). So the only source of drift is the intended change: erosion now lowers a cell's sediment share, which drip reads.
- That drift is within seed variation. Over eight seeds (level 5, 3000 Myr) mean continental share was 35.2% before and 35.4% after, with one collapse outlier in each set (12.9% before, 28.6% after). The default seed at level 6 ends at 13.0% against 36.6%, which is such an outlier.
- Tick time is unchanged (about 16.2 ms/tick, level 6, drip off).

## First results (level 6, 3000 Myr, defaults)

Mean sediment cover is 0.24 km on land after 300 Myr and about 0.6 km from 1200 Myr; at sea it is 0.02-0.09 km averaged over all ocean floor. 8-37% of eroded rock was loose cover rather than basement. Sediment share of continental crust ends at 23%, down from 40% in phase 2, because stripping cover now lowers it.
