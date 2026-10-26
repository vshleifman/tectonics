# 14 — Isostatic elevation

## Goal

Elevation becomes a function of what is under each cell: crust thickness and
density, the lithospheric lid, thermal age, the mantle's dynamic topography and
the water load. Boundaries stop adding relief directly. Mountains are high
because the crust under them is thick, and they stay high as long as it stays
thick.

## Why it matters

Today `apply_elevation` adds relief at convergent and divergent arcs, then
relaxes every cell toward `baseline_elevation` at `relax_rate` (0.02/Myr). So:

- Collision ranges fade in ~50 Myr even where the thickened crust is still
  there, and the bump counts the thickening twice, because the baseline
  already rises with thickness.
- `baseline_elevation` ignores the lid and the keel, while `buoyancy`, which
  decides what subducts, uses them. The map and the dynamics disagree about
  what floats.
- `continent_thermal_share` stands in for the missing lid.
- There is no water load. Below sea level, a column that rises by x in air
  rises by x / (rho_m - rho_w) * rho_m, about 1.45x, in water. This is why the
  cooling curve had to be imposed by hand (`ridge_elevation` to
  `abyss_elevation`) instead of following from the lid.

Elevation is read only by the renderer, metrics, protocol and cohesion swap,
so this ticket changes the map but not the plate dynamics.

## Design

### The column

For each cell, compute height in air above a datum:

```
e_air = datum
      + buoyancy(t, thickness, density, lithosphere, keel)   // existing fn, km
      - buoyancy(t, oceanic_thickness, oceanic_density, 0, 0) // ridge column
      + dyn_topo
      + boundary_term                                          // see below
```

- `datum` is chosen so that a ridge column (standard crust, no lid) under
  water sits at `ridge_elevation`: `datum = ridge_elevation / k`, where
  `k = rho_m / (rho_m - rho_w)`. `ridge_heat_uplift` keeps adding to it in a
  hot mantle, as `effective_tuning` does now.
- Use `column_buoyancy`/`buoyancy` as they are, so the map and the dynamics
  share one model. New fields: `water_density` (1.03).
- Water load: given sea level `s` (fixed at 0 until ticket 15),
  `elevation = e_air` where `e_air >= s`, else `s + (e_air - s) * k`.
- Clamp to `elevation_min`/`elevation_max` as now, as a safety net only.

### What replaces the boundary bumps

- **Collisions and arcs:** nothing. Advection already piles crust up
  (`max_thickening_per_myr`, cap `max_crust_thickness`) and accretes arc crust
  (`accretion_fraction`). Relief follows from that thickening.
- **Ridges:** nothing. Zero-age floor with no lid is the datum.
- **Trenches** are flexure and slab pull, not isostasy, so they stay a
  boundary term. Compute it fresh each tick rather than accumulating it: on
  the down-going cell of each convergent arc (the side without `slab`), add
  `-trench_depth * min(1, slab / slab_full_length)` taken from the overriding
  neighbour. `trench_deepen` becomes `trench_depth` (km, default ~3).
- **Optional, level 7+ only:** flexural smoothing. Spread each cell's load over
  one or two rings, weighted by lid thickness, so ranges get flanking basins
  and a 28 km cell does not stand alone. Skip at level 6, where cells are
  already ~110 km across.

### State

`elevation` stays a stored, advected field. Ticket 16 reads last tick's value
after transport, and the protocol and renderer stay unchanged. It is
recomputed from scratch at the end of every tick, so it no longer carries
history. The writes in `lay_new_floor`, `plates.rs` seeding and the tests
become harmless. Leave them for this ticket, and tidy them up later if they
cause confusion.

## Calibration (the real work)

Calibration fixes physical inputs, not outcomes (overview, decision 2). The
tests are column-by-column: each one fixes a column's crust, lid and keel and
checks where it stands relative to a ridge crest beside it. Nothing here
refers to an era, a land share or a sea level. The Earth ranges below are
only sanity checks on the inputs; if a physically defensible input set misses
one, record the miss rather than force it.

| Column, relative to a ridge crest under the same sea | Sanity range        |
| ---------------------------------------------------- | ------------------- |
| 100 Myr floor (lid at its cooled depth)              | 2.5-3.5 km lower    |
| Unkeeled continent, 40 km                            | 2.5-3.5 km higher   |
| Craton, 40 km, full keel                             | within ~0.5 km of the unkeeled continent |
| Orogen, 60 km                                        | 2-4 km above the 40 km continent |

My rough arithmetic with today's defaults (k = 1.45, datum -1.72):

- 100 Myr floor: lid 90 km -> about -5.0 km. Close, and `lid_delta_t` or
  `thermal_expansion` closes the gap.
- Unkeeled continent, 40 km: about +1.8 km. Too high. Raising
  `continental_density` to ~2.8 (closer to Earth's bulk crust) brings it to
  about +0.6.
- Craton, 40 km, keel 1: about +4.7 km. **Far too high.** `keel_depletion`
  is set so a keel exactly cancels its own cold (neutral buoyancy), so the keel
  contributes none of the lid's weight. This is the main calibration problem.
  Options:
  1. Lower `keel_depletion` so a full keel is slightly heavy (net about
     -2.5 km). This changes dynamics: keel buoyancy feeds slabs, overriding
     and craton survival. The bench gates it, and craton metrics will move.
  2. Keep the dynamics and add an elevation-only keel term. This
     reintroduces the two-model split this ticket exists to remove, so do it
     only as a named, documented fudge.

  Try option 1 first: it is the physically grounded choice, because a keel
  that is depleted but cold need not be exactly neutral. If craton survival
  breaks, fall back to option 2.

These numbers are back-of-envelope. Encode the column tests first, then
adjust physical inputs until they pass. Do not adjust anything to move the
global land share.

## Steps

1. Add `water_density` and `trench_depth`. Keep the old fields for now.
2. Write `column_elevation(t, data, i, dyn_topo, sea)` in `elevation.rs` next
   to `buoyancy`.
3. Rewrite the existing `elevation.rs` tests as the calibration table above.
   Keep `the_sea_floor_sinks_as_the_square_root_of_its_age_then_flattens` in
   spirit: the sea floor now flattens because the lid stops at
   `plate_thickness`.
4. Replace `apply_elevation` with a single parallel pass: column, then the
   trench term, then the clamp.
5. Tune until the tests pass, then run the bench.
6. Remove `collision_uplift`, `arc_uplift`, `ridge_pull`, `relax_rate` and
   `continent_thermal_share` from Rust, TypeScript, the UI and `bench.rs`.
   `abyss_elevation` and `seafloor_flattening_age` stay: ridge push
   (`plate_forces.rs:144` via `thermal_elevation`) still uses them. Add a test
   that the column model's 100 Myr depth agrees with `abyss_elevation` to
   within 0.5 km, so the two cannot drift apart.

## Acceptance criteria

- The calibration table passes as unit tests.
- At level 6, 12 seeds, 1000 ticks: every dynamics metric stays within its
  band, unless option 1 above was taken, in which case the craton metrics
  are re-baselined with the reasons recorded.
- Collision ranges persist as long as their thickened crust does, and no
  longer.
- `land_area` is reported before and after. It is an outcome, not a target.

## Cost

- Code: small (one pass replaced, ~150 lines including tests).
- Calibration: most of the ticket, particularly the keel.
- Runtime: about the same as today's elevation pass (0.21 ms/tick at level 6),
  or a little less, since the boundary loop shrinks to trenches only.
  Flexural smoothing, if done, adds one or two neighbour passes.

## Dependencies

None.
