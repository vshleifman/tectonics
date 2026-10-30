---
name: "Rock cycle phase 2: igneous composition"
overview: "Add mafic and felsic lanes, written where crust is made: ridges and plumes lay mafic crust, arcs lay a mix set by what they build on, and thick hot crust differentiates mafic into felsic over time. An arc clock records how long ago a cell last saw arc magmatism. Output only."
todos:
    - id: lanes
      content: Add LANE_MAFIC and LANE_FELSIC; mafic + felsic + sediment is a partition summing to 1, plume stays a provenance tracer
      status: completed
    - id: sources
      content: Write composition at every source - lay_new_floor and initialise_crust (mafic), primordial raise (mafic), arc accretion in advect pass 4 and surplus placement (island_arc_felsic / continental_arc_felsic)
      status: completed
    - id: differentiation
      content: Add geotherm.rs (temperature at depth from the cell's heat flow), and a pass converting mafic to felsic where the base of continental crust is hotter than the mafic solidus
      status: completed
    - id: arc-clock
      content: Add CellData.arc_age (Myr since arc magmatism, NEVER sentinel), reset where accreted > 0, aged each tick, carried by gather and mixed as the minimum on welds
      status: completed
    - id: expose
      content: Lane exports, a "composition" ternary view, felsic/mafic in cell labels, tuning in tuning.rs/tuning.ts/controls.ts, felsic share of continental crust in bench and diagnostics
      status: completed
    - id: verify
      content: Partition and ledger tests; dynamics fingerprint unchanged over 3000 Myr; report felsic share of continental crust over time
      status: completed
isProject: false
---

# Rock cycle phase 2: igneous composition

Part 2 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md). Depends on phase 1.

## Why

Igneous rock is where the rock cycle starts, and where most metal deposits come from. The sim already knows where crust is made: ridges, plumes and arcs. It doesn't record what kind of crust each makes.

No continent is placed at the start. `initialise_crust` in [sim-rs/src/plates.rs](sim-rs/src/plates.rs) lays ocean everywhere, and every continent is made in the run by plumes, arc accretion and piling. So unless something turns mafic crust felsic, continents will stay mafic. Differentiation is that something.

## Changes

- **Lanes.** `LANE_MAFIC` and `LANE_FELSIC` join `LANE_SEDIMENT` as a partition summing to 1. `LANE_PLUME` stays separate as a record of where crust came from, not what it is.
  - Add `debug_assert`s on the partition after `advect` and `apply_crust_lifecycle`.
  - The existing rule "a tracer is untouched by removal" still holds: removal takes every lane in proportion.
- **Ocean floor is mafic.** `lay_new_floor` in [sim-rs/src/crust_lifecycle.rs](sim-rs/src/crust_lifecycle.rs) and `initialise_crust` set the mafic lane to 1. So does the `RIFT` fill value of the gather in [sim-rs/src/advect.rs](sim-rs/src/advect.rs).
- **Plume crust is mafic.** `raise` in [sim-rs/src/primordial.rs](sim-rs/src/primordial.rs) mixes in mafic crust. Plume crust that becomes continent turns felsic through differentiation, as thick plateau crust melting into TTG would. The plume lane still marks where it came from.
- **Arc crust.** In advect pass 4 (near 939) and surplus placement (near 1045), accreted crust mixes in with `Mix` felsic `f` and mafic `1 - f`.
  - `f` is `continental_arc_felsic` when the arc cell is continental and `island_arc_felsic` when it is oceanic.
  - These are physical inputs: the bulk composition of arc crust. Continental arcs assimilate crust and come out more evolved. Start at about 0.6 and 0.35, with a reference in the doc comments.
- **Geotherm.** A new [sim-rs/src/geotherm.rs](sim-rs/src/geotherm.rs): `temperature_at(t, heat_flow_mw, depth_km) -> °C`. It uses the conductive profile with crustal heat production: `T0 + q z / k - A z² / 2k`, with `surface_temp`, `crust_conductivity` (about 2.5 W/m/K) and `crust_heat_production` (about 1 µW/m³) as tuning. The input is `PlanetHeat::heat_flow`, per cell in mW/m², as of the last step. `mantle_heat` is not a temperature (it is supercontinent insulation), so it isn't used. Phase 4 reuses this module.
- **Differentiation.** A new `differentiate` pass in `crust_lifecycle.rs`, run after `apply_crust_lifecycle`, with `heat` passed in from `evolve_crust`:
  - It acts on continental cells whose Moho temperature, `temperature_at(heat_flow[i], thickness[i])`, is above `mafic_solidus` (about 850 °C, dehydration melting of amphibolite).
  - It converts mafic to felsic at `differentiation_rate * clamp01((T_moho - mafic_solidus) / melting_span)` per Myr. Felsic melt comes from the hot base of the crust: thicker or hotter crust differentiates, and thin or cold crust doesn't. There is no thickness threshold of its own.
  - It moves mass between lanes only; thickness is unchanged.
  - The residue that would founder is not modelled here. Delamination in [sim-rs/src/recycling.rs](sim-rs/src/recycling.rs) and drip already remove crust; phase 6 decides whether they should prefer the mafic lane.
- **Arc clock.** `CellData.arc_age: Vec<f32>` holds the Myr since the cell last received arc crust, with `NEVER = f32::MAX`.
  - Reset to 0 where `out.accreted > 0` and the crust was placed.
  - Each tick, `apply_crust_lifecycle` adds `dt` to every value that isn't `NEVER`.
  - The gather carries it, with `NEVER` as the rift value. Welds keep the younger (minimum) of the two, since a magmatic event isn't a quantity to average.
  - Add it to the cohesion swap and the dynamics fingerprint's exclusions.
- **`hot_drip` is unchanged.** It still reads `1 - plume - sediment`, not the mafic lane. Phase 6 makes that switch.

## Exposure

- `lane_ptr` covers the new lanes. Add `arc_age_ptr` in [sim-rs/src/lib.rs](sim-rs/src/lib.rs).
- A "composition" view in [src/render/colour.ts](src/render/colour.ts): a ternary blend of mafic (dark green-grey), felsic (pink) and sediment (sand), with a legend in the same shape as the others. Register it in [src/ui/settingsStore.ts](src/ui/settingsStore.ts) and [src/ui/persist.ts](src/ui/persist.ts).
- [src/render/CellLabels.ts](src/render/CellLabels.ts): a `maf/fel/sed` percentage line, and arc age when it isn't `NEVER`.
- Tuning: `island_arc_felsic`, `continental_arc_felsic`, `differentiation_rate`, `mafic_solidus`, `melting_span`, `surface_temp`, `crust_conductivity` and `crust_heat_production`, in [sim-rs/src/tuning.rs](sim-rs/src/tuning.rs), [src/sim/tuning.ts](src/sim/tuning.ts) and [src/ui/controls.ts](src/ui/controls.ts), in a "Composition" group.
- Metrics: the felsic and mafic share of continental crust by mass, in [sim-rs/src/metrics.rs](sim-rs/src/metrics.rs), [src/sim/metrics.ts](src/sim/metrics.ts), the bench and [src/ui/diagnostics.ts](src/ui/diagnostics.ts).

## Verification

- **Unit tests:**
  - new ocean floor is all mafic;
  - an arc on continent gets a higher felsic share than an island arc;
  - the geotherm gives a plausible Moho temperature for 35 km of crust at 65 mW/m². The defaults give about 675 °C; assert 550–750 °C;
  - thick hot continent grows more felsic and thin or cold continent doesn't;
  - lanes sum to 1 after a full tick on a seeded world;
  - lane mass is conserved through advect, counting created and subducted mass;
  - the arc clock resets at an arc and ages away from it.
- **Dynamics fingerprint:** matches phase 1 bit for bit over the 3000 Myr bench, at 1 and 4 threads.
- **Report, don't tune:** the felsic share of continental crust over the run. It's worth comparing with Earth (bulk continental crust is intermediate), but the inputs that set it are the arc compositions and the differentiation rate, adjusted only on physical grounds.

## As built

Where the code departs from the plan above:

- **Partition checks are tests, not `debug_assert`s.** `mafic_felsic_and_sediment_always_make_up_the_crust` runs 150 ticks from the hot start (plumes, drip, arcs, erosion and rifting all active) and checks the lanes sum to 1 every tick; `the_lanes_account_for_all_the_crust` checks the lane masses add up to the live crust mass.
- **Plume crust doesn't dilute sediment.** Phase 1 never diluted the sediment lane when a plume raised a cell, and `hot_drip` reads it, so changing that would change the dynamics. `raise` dilutes the felsic lane and lets mafic take up what is left. Phase 3 rebuilds sediment as a cover layer and ends the approximation.
- **Erosion deposits dilute the igneous lanes.** Silt added to a cell scales its mafic and felsic shares down, so the partition holds. The plume lane is still not diluted, as before.
- **Drip takes from the mafic lane first**, then from the felsic only if that is not enough. What founders is still decided from `1 - plume - sediment`, unchanged.
- **Surplus arc crust uses the receiving cell's type** to pick `island_arc_felsic` or `continental_arc_felsic`, not the cell it spilled from.
- **Arc age is carried by the youngest rule** through welds, spill rounds, lateral spreading and the two hole-filling pulls. Spilled crust takes the age of the cell it spills from, which is an approximation.
- **`CellData::new` starts every cell as fresh ocean floor** (all mafic, no arc), so hand-built test worlds satisfy the partition.
- **The composition fingerprint includes `arc_age`.** The dynamics fingerprint is unchanged from phase 1.

## Verified

- Dynamics fingerprint at level 6: `d6fd61faffafbb09` over 1000 ticks, on 1 thread and on the parallel build. At level 5 over 300 ticks: `509b2cd6ef1752d5`. Both are the same as phase 1.
- Over 1500 ticks (3000 Myr), a fingerprint of everything but the new lanes and `arc_age` is `31aaebe50ee138b4`, identical to the original `main` build.
- Tick time is unchanged (about 17.8 ms/tick at level 6).

## First results (level 6, 3000 Myr, default inputs)

Share of continental crust by mass, felsic / mafic / sediment, every 300 Myr: 7/91/3, 16/78/6, 20/73/6, 27/64/9, 27/60/13, 29/54/17, 28/47/25, 24/46/30, 22/44/34, 21/40/40.

Felsic rock builds up for the first 1.8 Gyr and then falls as sediment takes over. Mafic crust is still 40% at the end, well above the roughly intermediate bulk composition of real continental crust. The cause is clear in the inputs: at the default heat flow (65 mW/m²) the base of the crust only passes the 850 °C solidus once it is about 60 km thick (the geotherm peaks near 65 km), so differentiation is limited to the hottest, thickest crust and early hot-mantle eras. Per the project rule these are reported, not tuned. If felsic crust is wanted, the physical inputs to look at are crustal heat production and the arc compositions, not the rate.
