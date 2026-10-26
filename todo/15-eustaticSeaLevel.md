# 15 — Eustatic sea level

## Goal

Sea level is set each tick by a fixed volume of ocean water filling the
basins, not fixed at 0 km. Elevations are then reported relative to that sea
level, so `elevation > 0` keeps meaning "dry land" everywhere downstream.

## Why it matters

- Land area depends on where the sea sits, and today that is a constant.
- The sea level of real-world history follows the ocean basins. Young, fast,
  shallow ridges displace water and flood the continents (the Cretaceous,
  ~250 m higher). An old, slow, deep ocean floor drains them. A
  supercontinent breakup makes young floor and raises the sea. The sim
  already produces the ocean-age history that drives this; it just never
  reaches the coastline.
- Ticket 16 needs a base level to erode down to.

## Design

### The solve

Let `e_air[i]` be ticket 14's height in air, before water loading. With water
load factor `k`, the water stored over cell `i` at sea level `s` is
`k * max(0, s - e_air[i]) * area[i]`. Then:

```
V(s) = k * sum_i area[i] * max(0, s - e_air[i])
```

`V(s)` is continuous, piecewise linear, convex and increasing, with slope
`k * (area below s)`. Solve `V(s) = V_water` by Newton's method, starting
from last tick's `s` (2-3 iterations once warm), with bisection as a fallback
if a step leaves the bracket. Each iteration is one `par::sum` over cells, so
the result is the same on any thread count.

Then each cell's elevation relative to sea level is
`e_air - s` above water and `(e_air - s) * k` below it. That is ticket 14's
formula with `s` plugged in, shifted so the sea sits at 0.

### Water volume

- New tuning field `ocean_volume`: water as a global equivalent layer in km
  (Earth: ~1.335e9 km³ / 5.1e8 km² ≈ 2.6 km). Volume is
  `ocean_volume * total_area` in the mesh's units. Default 2.6. It is a
  property of the planet, like its radiogenic heat, and is not tuned to make
  any particular amount of land.
- It stays constant over a run. Ingassing and outgassing over deep time are
  out of scope.
- An all-ocean starting world then stands with the sea some way above the old
  0 km datum. That is fine: the datum has no meaning once everything is
  measured from sea level.

### Where it runs

At the end of `evolve_crust`, inside the elevation pass: compute `e_air` for
every cell, solve `s`, then write elevations relative to `s`. Store
`sea_level` (relative to the datum) on `Sim` as the eustatic history readout.

### UI and metrics

- Add `sea_level` to `WorldMetrics`, with a chart in `diagnostics.ts`
  ("Eustatic sea level, km") next to land area and mean ocean age, so the
  ridge-volume link is visible.
- **The `seaLevel` slider is removed.** An "Ocean volume" slider takes its
  place in `controls.ts` (km of global equivalent layer, say 0-6), wired to
  the `ocean_volume` tuning value rather than to view settings. Remove
  `seaLevel` from `settingsStore.ts`, `main.ts` and the `CellRenderer` and
  `colour.ts` signatures. Colouring then always splits at 0, which is now the
  real sea level.
- **Live response.** The slider must react like the old one, including while
  paused. Today `setTuning` in `sim.worker.ts` stores the tuning and returns,
  so nothing changes until the next tick. Add `World::refresh_surface()`
  (wasm) to recompute `e_air`, re-solve sea level and rewrite elevation from
  the current state, with no transport and no time passing. Call it after
  `setTuning` and publish a snapshot. Cost: one elevation pass plus the solve,
  a few ms at level 8. Throttle slider input to one refresh in flight at a
  time.
- `metrics.rs`'s `land_area` (`elevation > 0`) is then correct as it stands.
- Bench `relief` line: add the sea level and the share of continental crust
  above water (Earth: ~0.7).

## Steps

1. Add `ocean_volume` in Rust and TypeScript.
2. Write `solve_sea_level(areas, e_air, k, volume, guess) -> f64` with unit
   tests: an all-flat world, a two-level world solved exactly, and a monotonic
   response to volume.
3. Wire it into the elevation pass, with a `sea_level` field on `Sim` and in
   metrics.
4. Add `refresh_surface()` and the worker's refresh-and-publish on
   `setTuning`.
5. Replace the sea-level slider with the ocean volume slider, remove
   `seaLevel` from settings and the renderer, and add the chart.

## Acceptance criteria

- The solve converges to 1e-6 km in at most 8 iterations from a cold start,
  and in at most 3 when warm.
- Same fingerprint on one thread and four.
- Test: a world whose mean ocean age is lowered, with everything else
  unchanged, has a higher sea level and less land.
- Test: more `ocean_volume` gives a higher sea level and less land.
- Moving the ocean volume slider while paused re-colours the map within a
  frame or two, with no tick taken (the fingerprint is unchanged apart from
  elevation).
- Land share, the share of continental crust above water, and sea level over
  time are reported in metrics and the bench, as outcomes rather than targets.

## Cost

- Code: small (~100 lines plus tests and a chart).
- Runtime: 3-8 parallel sums over the cells per tick. At level 8 (655k
  cells), that is in the low single-digit ms against ~100 ms/tick now.
- Risk: low. It is output only, apart from ticket 16 reading it.

## Dependencies

- 14 (it needs `e_air` and the water load).
