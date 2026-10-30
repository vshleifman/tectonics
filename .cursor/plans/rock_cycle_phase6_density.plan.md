---
name: "Rock cycle phase 6: density from composition"
overview: "Mix each cell's crust density from its lithology lanes, cover and metamorphic grade, replacing the two density constants, and switch hot_drip to the mafic lane. The one phase that changes the dynamics: calibrate physical inputs, bench, report."
todos:
    - id: densities
      content: Add per-lithology densities (mafic, felsic, metasediment, cover clastic, eclogite below eclogite_depth) as tuning; remove continental_density and oceanic_density
      status: pending
    - id: pass
      content: Add a single composition_density pass before apply_elevation; delete the scattered density writes in advect, crust_lifecycle, erosion, plate_forces, plates, slabs and world
      status: pending
    - id: drip
      content: Switch hot_drip and (optionally) delamination to the mafic lane
      status: pending
    - id: callers
      content: Check every reader of density - elevation buoyancy, erosion c, lay_new_floor height, slabs test builders, ridge reference in elevation.rs - still uses the right value
      status: pending
    - id: expose
      content: Density view ramp from the lithology bounds, density metrics, tuning and controls
      status: pending
    - id: verify
      content: Bench 3000 Myr against the phase 5 baseline - land share, hypsometry, trenches, plate speeds, continental growth - and report
      status: pending
isProject: false
---

# Rock cycle phase 6: density from composition

Part 6 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md). Depends on phases 2–4. Independent of phase 5.

## Why

This answers where this started: the density view shows two shades because density is two constants. With composition tracked, density should follow from it. Felsic continents then stand higher than mafic ones, and sediment cover is light. This also lets a hot mantle founder mafic crust specifically, which is what `hot_drip` approximates now.

## Changes

- **Densities as tuning**, in g/cm³, each referenced:
  - `mafic_density` about 2.95;
  - `felsic_density` about 2.7;
  - `metasediment_density` about 2.7;
  - `cover_density` about 2.4, for uncompacted clastic. Optionally ramp it toward 2.6 with cover thickness for compaction.
  - `eclogite_density` about 3.45, for mafic rock currently deeper than `eclogite_depth` (about 45 km, which is 12 kbar). Phase 4's peaks describe the exposed rock, not the root, so this comes from today's thickness.
  - Remove `continental_density` and `oceanic_density` from [sim-rs/src/tuning.rs](sim-rs/src/tuning.rs), [src/sim/tuning.ts](src/sim/tuning.ts) and [src/ui/controls.ts](src/ui/controls.ts).
- **`composition_density`** in a new small module or in [sim-rs/src/elevation.rs](sim-rs/src/elevation.rs). It gives the mass-weighted density of basement lanes and cover by thickness. For the mafic part, the share lying below `eclogite_depth` uses `eclogite_density`. Assume mafic rock sits at the base of the column, which is where differentiation leaves it.
  - Run it once per tick in `evolve_crust`, right before `apply_elevation`, so erosion on the next tick (`c` in [sim-rs/src/erosion.rs](sim-rs/src/erosion.rs), near line 74) sees this tick's value.
- **Delete the scattered density writes.** They set the two constants and become redundant:
  - [sim-rs/src/advect.rs](sim-rs/src/advect.rs): near 805, 855, 1511 and 1515;
  - [sim-rs/src/crust_lifecycle.rs](sim-rs/src/crust_lifecycle.rs): near 130 and 153;
  - [sim-rs/src/erosion.rs](sim-rs/src/erosion.rs): 169 and 174 (test);
  - [sim-rs/src/plate_forces.rs](sim-rs/src/plate_forces.rs): near 715, 722 and 782;
  - [sim-rs/src/plates.rs](sim-rs/src/plates.rs): near 180;
  - [sim-rs/src/slabs.rs](sim-rs/src/slabs.rs): 263 (test);
  - [sim-rs/src/world.rs](sim-rs/src/world.rs): test builders near 1044, 1257, 1360 and 1579.

  Test builders set lanes instead. Each place that flips crust type mid-tick must still leave a sensible density until the pass runs. The simplest way is to call `composition_density(i)` there instead of the constant.
- **Reference columns.** `lay_new_floor` and the ridge reference in `elevation.rs` (near 86) use `t.oceanic_density` for a column of fresh ridge crust. Replace it with `mafic_density`; fresh floor is all mafic.
- **`hot_drip`** in [sim-rs/src/primordial.rs](sim-rs/src/primordial.rs) switches from `1 - plume - sediment` to the mafic lane. Plume crust now founders until it has differentiated, which is physically what drip stands for, but it may slow early continent growth a lot. Bench it on its own before the density change, so the two effects can be told apart.
- **Delamination** in [sim-rs/src/recycling.rs](sim-rs/src/recycling.rs) can prefer mafic and eclogite lanes. This is optional; keep it out unless the bench shows thick orogens never shed their roots.

## Exposure

- The density view's range comes from the lowest and highest lithology density instead of the two constants (`densityMin` and `densityMax` in [src/plates/PlateData.ts](src/plates/PlateData.ts)). Update the view's note in [src/render/colour.ts](src/render/colour.ts) (near 526) to say what it now shows.
- Metrics: mean continental and oceanic density, and their spread.

## Verification

- **Unit tests:**
  - an all-felsic column stands higher than an all-mafic one of equal thickness;
  - 2 km of cover lowers column density;
  - a 60 km column with a mafic base is less buoyant than the same column at 40 km, per km of crust;
  - fresh ridge floor sits at `ridge_elevation`.
- **Bench in two steps,** level 6 over 3000 Myr against the phase 5 baseline:
  1. the drip switch alone;
  2. then density.

  For each, report land share, hypsometry, continental area and growth curve, trench count, plate speeds and mean ocean age, with `scripts/compare.ts` bands as the first check.
- Per the project rule, any adjustment is to a physical input with its reasoning stated, never a fit to an Earth figure. If land share moves a lot, report it and discuss before compensating.
