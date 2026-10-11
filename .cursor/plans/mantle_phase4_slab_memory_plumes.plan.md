---
name: "Mantle phase 4: two-layer slab memory and plume sites"
overview: "Pass sinking slab material into a lower-mantle layer after a delay, let it pile up and decay slowly, and raise plumes at the edges of those piles at a rate set by core heat flow, so plumes persist into the modern era as hotspots."
todos:
    - id: lower-layer
      content: Add lower_slab per site with transfer over slab_transit_time and decay over slab_pile_lifetime
      status: completed
    - id: plume-weights
      content: Compute per-site plume weight peaking at pile edges (low own pile, high neighbour pile) and expose a weighted pick
      status: completed
    - id: plumes
      content: In primordial.rs, set plume rate from core heat flow, place plumes by the weighted pick with the existing Rng, scale plateau thickness with excess between plume_thickness_cold and plume_thickness
      status: completed
    - id: expose
      content: Export lower_slab sampled per cell and a view mode; mirror tuning; metrics for plume source and pile mass
      status: completed
    - id: verify
      content: "Unit tests for transit, decay and edge weighting; check modern plumes make plateaus not continent; bench against phase 3 (14 layouts at level 6 over 3000 Myr: plates, ocean age, rifts and collapse rate match; cratons 21.5% vs 23.5%; plume continent by 1000 Myr 0.116 vs 0.102; ~2 plumes alive in the modern era vs 0)"
      status: completed
isProject: false
---

# Mantle phase 4: two-layer slab memory and plume sites

Part 4 of 5. Index: [planetary_heat_and_mantle_layers_f89b1aa1.plan.md](planetary_heat_and_mantle_layers_f89b1aa1.plan.md). Depends on phase 1 (core heat flow, excess) and phase 3 (site neighbours, `upper_temp`).

## Why

Plumes currently rise at random, and only while the primordial excess lasts ([sim-rs/src/primordial.rs](sim-rs/src/primordial.rs)). On Earth they cluster at the edges of deep, long-lived structures (the LLSVPs) that slab history has shaped. A lower layer gives the mantle a memory spanning hundreds of Myr, and gives plumes a place to rise and a reason to keep rising today.

## Changes

- In [sim-rs/src/mantle.rs](sim-rs/src/mantle.rs), `MantleFlow` gains `lower_slab: Vec<f64>`. Each step:
  - `moved = sink[s] * dt / slab_transit_time` (about 100 Myr) is added to it. As built, this step's subduction (`flux`, per unit site area) goes into a `transit` reservoir that empties into the pile over `slab_transit_time`, so arrival is a true first-order delay. The remembered `sink` already carries an 800 Myr memory, which would have stacked on top of the pile's own, as phase 3 found for cooling;
  - it decays over `slab_pile_lifetime` (500–1000 Myr; 700 by default);
  - it spreads to neighbours at `lower_diffusion`, much slower than the upper layer (0.002 against 0.01).
- Cold piles also cool the upper layer above them slightly (`pile_cooling * lower_slab`), so the two layers interact.
- `plume_weight[s] = max(0, mean of neighbour piles - own pile) * (1 + max(upper_temp[s], 0) / upper_temp_max)`, plus a small floor so an empty mantle can still raise plumes. Add `pick_plume_site(&self, u: f64) -> Vec3`, which picks a site from the cumulative weights. As built, the floor is `plume_floor` times the mean edge weight, spread by area (with no edges anywhere, plumes rise anywhere alike), and the pick takes `&Mesh` too: the remainder of `u` within the chosen site's share picks one of its cells, so plumes don't all rise on the site centre.
- [sim-rs/src/primordial.rs](sim-rs/src/primordial.rs):
  - plume rate becomes `plume_rate * heat.core_tw / modern_core_tw`, replacing `plume_rate * excess`. As built, `modern_core_tw` is derived as `core_coupling * (MODERN_CORE_K - MODERN_MANTLE_K)` rather than tuned, and `PlanetHeat::core_tw` gives the current flow. At the defaults the ratio stays within a few per cent of 1 for the whole run, because the core cools about as fast as the mantle;
  - placement uses `mantle.pick_plume_site(rng.next())`, keeping the existing `Rng` so runs stay deterministic;
  - plateau thickness is `lerp(plume_thickness_cold, plume_thickness, clamp01(excess))`, so a cool mantle's plumes build ocean plateaus (about 15–20 km) rather than reaching the continental threshold. As built, `plume_thickness_cold` is 13 km, because the continental threshold is 15. The lerp is on `excess^1.5`: linear, mid-era plumes made so much keeled continent that cratons went from 24% to 34% and plate counts fell; squared, cratons dropped to 14%;
  - rename the module doc to cover the modern era too, but keep the file name.
- `Primordial::update` takes `&MantleFlow` and `&PlanetHeat`. Update the call in `evolve_crust` in [sim-rs/src/world.rs](sim-rs/src/world.rs).

## Exposure

The lower pile sampled per cell for a "slab graveyard" view mode, new tuning keys in [src/sim/tuning.ts](src/sim/tuning.ts) and [src/ui/controls.ts](src/ui/controls.ts), and metrics for total pile mass and peak plume weight. The existing `active_plumes` metric stays. As built: `lower_slab_ptr`, a "Lower mantle & plumes" slider group, `pileMass` (globe areas of slab) and `plumeWeightPeak` (peak over mean weight), a diagnostics chart, and bench columns `pile` and `plume_peak`.

## Verification

- Unit tests:
  - a constant sink fills the lower layer with roughly the transit-time delay;
  - a pile decays once its sink stops;
  - weights are highest one ring out from a pile, and low at its centre and far from it;
  - picks are deterministic for a given seed.
- Check that a cool mantle (excess near 0) keeps raising plumes, and that they don't make continent.
- Bench against phase 3: plume-made crust share in the early era should stay close, and hotspot counts in the modern era should be non-zero.
