# 16 — Erosion and sediment

## Goal

Land above sea level wears down at a rate set by its height. The eroded crust
runs downhill to the coast and builds shelves and slopes on the ocean floor
beside it. Mass is conserved: crust moves, nothing is created or lost.

## Why it matters

- Old orogens never wear down today. Their thick crust stays forever unless a
  rift, a margin or delamination takes it, and delamination ran at 0.0000%/Myr
  in the level 6 run.
- Erosion is why Earth's continents sit near sea level, with flat interiors
  and high ranges only where crust is being thickened now. Without it, 14
  and 15 give a physically consistent surface for crust that never wears, so
  relief only ever builds up.
- Shelves, deltas and passive-margin wedges are how continents gain area
  without arcs.

This is the one ticket in the set that changes the dynamics, because
thickness drives buoyancy, the crust-type flips, craton growth and rifting
strength.

## Design

### Erosion rate

For each cell above sea level (elevation `e > 0`, from last tick and carried
with the crust by advection):

```
eroded_thickness = (e / c) * (dt / erosion_time) * erodibility
```

- `c = (rho_m - rho_c) / rho_m` (~0.15-0.18) converts height to the excess
  crust holding it up. Removing `e / c` of thickness brings the column to sea
  level, so `erosion_time` is the e-folding time of the relief itself,
  isostatic rebound included.
- `erosion_time` default ~150 Myr. A 4 km range is then ~1.5 km after
  150 Myr, in line with Appalachian/Caledonian-type ranges keeping relief for
  a few hundred Myr. Today's active ranges erode faster; for later, a slope
  term (`1 + slope_gain * local relief`) can capture that.
- `erodibility` = `craton_wear(keel)`, so cratons, which are low anyway, wear
  slowest, as they do everywhere else in the model.
- The cap per tick is never more than the crust standing above sea level.

### Routing and deposition (recommended)

Steepest-descent routing over the cell graph:

1. Collect land cells and ocean cells within `sediment_reach` rings of the
   coast (default 3), sorted by elevation, highest first, ties by index.
2. Each cell adds its own eroded load to whatever load has arrived from
   upslope, then passes the total to its lowest neighbour on the same plate.
   A cell with no lower neighbour on its plate is a closed basin: it keeps the
   load (lake or rift fill).
3. In the sea, a cell keeps what fills it up to `shelf_depth` (default
   -0.15 km relative to sea level) and passes on the rest. Past the shelf it
   keeps a share `slope_capture` (default 0.5) and passes on the rest, so the
   slope tails off. Whatever is left past `sediment_reach` stays in the last
   cell, so nothing is lost.
4. Deposited sediment raises thickness in the receiving cell. It does not
   change the cell's `density` field: crust type and density stay governed
   by `apply_crust_lifecycle`. Shelf cells beside continent that pass the
   continental threshold flip through the existing `continental_nbrs >= 2`
   rule, which is how margins grow outward.

Staying on the same plate keeps sediment from being passed across a trench in
one step. A passive margin is a single plate, so the shelf is built there.

**Cheaper fallback** if routing proves too slow at level 8: a few iterations of
explicit diffusion of `e` restricted to land and nearshore cells, with the
deposit pinned at `shelf_depth`. It gives no deltas or drainage, but has the
same mass behaviour.

### Sediment tracer

Add `sediment_share: Vec<f32>` to `CellData`, handled exactly like
`plume_share`: it travels with the crust, mixes by mass on deposit through
`mixed_share`, and is left untouched by removal. Uses:

- Hot-era drip (`primordial.rs`) treats `plume_share + sediment_share` as
  buoyant, so sediment does not founder as mafic crust.
- Diagnostics: how much of the continental crust is reworked sediment.

It also has to be added to the advect gather, the cohesion swap, the
fingerprint and the multi-thread test's field list.

### Placement in the tick

In `evolve_crust`, after `recycle_continental_margins` and
`infill_enclosed_basins` and before `primordial.update` and
`apply_crust_lifecycle`. That way shelf cells can flip in the same tick and
the plume and drip logic see the new thickness. It reads last tick's elevation
and sea level (both carried or stored), which is a one-tick lag that is fine
at 2 Myr ticks.

### Interactions to watch

- **Recycling.** `recycling.rs` models sediment carried into trenches as a
  fixed share of continental area per Myr (`continental_recycling_rate`), unless
  `hold_continental_area` is on, when it matches that to growth and the gap to
  `continental_area_target`. Explicit sediment that reaches a trench and
  subducts is counted again, as advection's subduction. With the target held,
  the controller rebalances itself. With the fixed rate it double-counts, so
  document this and consider lowering the default rate.
- **Craton growth** needs thick, quiet interiors. Erosion thins interiors
  slowly (they are low), but check `craton_area` and `ancient_keeled` in the
  bench.
- **Rift strength** and continental thickness thresholds: a continent eroded
  thin reverts to ocean at `continental_revert_thickness`. Erosion is to sea
  level, not below it, so this should not happen in interiors. Test it.
- **Ledger.** Erosion and deposition are transfers. Add `eroded_this_step`
  and `deposited_this_step` tallies to `AdvectionField` and assert they are
  equal (to rounding) every tick in a debug test.

## Steps

1. Add `sediment_share` plumbing: data, advect, cohesion, fingerprint, tests.
2. Add tunings `erosion_time`, `shelf_depth`, `slope_capture`,
   `sediment_reach` in Rust and TypeScript.
3. New module `sim-rs/src/erosion.rs` with `erode_and_deposit(...)`. Unit
   tests:
   - A single peak decays toward sea level with e-folding near `erosion_time`.
   - The ledger closes to 1e-9.
   - A coastal range builds a shelf on its own side.
   - A closed basin fills.
   - Nothing crosses a plate boundary in one step.
4. Wire it into `evolve_crust` and add timings to `PASS_NAMES` on both sides.
5. Update drip to respect the tracer.
6. Bench at levels 6 and 7 with 12 seeds. Expect band changes in continental
   area, thickness percentiles, craton metrics and possibly rift counts.
   Record them, and update the bands deliberately.
7. UI: an optional "sediment" colour mode, plus erosion and deposition flux on
   the diagnostics mass chart.

## Acceptance criteria

- Mass drift stays at zero over 2 Gyr.
- An orogen whose collision has stopped loses half its relief in roughly
  `erosion_time * ln 2` (±30%).
- Passive margins grow a shelf over 100 Myr. Visibly, at level 7+, continental
  area along passive coasts rises.
- Late-run continental thickness p90 falls and the spread narrows, as crust
  is redistributed from ranges to margins. The resulting thickness and land
  share are reported, not tuned toward.
- Same fingerprint on one thread and four.

## Cost

- Code: medium. Roughly 300-400 lines plus tests; the tracer plumbing is the
  fiddly part.
- Runtime: the sort is the hot spot. It covers only land and nearshore cells
  (perhaps 30-50% of the globe), so at level 8 (~200-300k cells) expect tens
  of ms per tick in single-threaded WASM, against ~100 ms/tick now. Measure
  it. If it is too slow, sort every few ticks and reuse the order (relief
  changes slowly), or use the diffusion fallback.
- Calibration: medium to large, because it feeds back into the dynamics.

## Dependencies

- 14 (relief from the column) and 15 (base level).
- Decision 4 in the overview (sediment tracer: yes).
