# Surface realism — iteration overview

Tickets to take the surface from "relief painted on by boundaries" to "relief
that follows from the crust, under a sea that has a fixed amount of water,
worn down by erosion". Hotspot tracks come along too, because they are cheap
and use the same plumbing.

## Where we are

The plate dynamics hold up: speeds (~6 cm/yr), mean sea-floor age (~74 Myr),
mean ocean depth (-3.7 km) and heat flow (~55 TW) all land near Earth's. The
surface does not:

- **Almost no dry land.** Level 6, 1000 ticks (3 Ga -> ~1 Ga): continental
  crust covers 13.9% of the globe, but only 2.6% stands above sea level
  (Earth: ~29%). Median continental thickness is 28.7 km, and
  `elevation.rs` puts 30 km of continent under the sea.
- **Sea level is a fixed 0 km.** On Earth it is set by a fixed volume of
  water, so it rises when ridges are young and fast and falls when the floor
  is old.
- **Elevation is decorative.** Boundaries add relief directly
  (`elevation.rs:105-125`), and it relaxes back over ~50 Myr. Nothing in the
  sim reads elevation back. Elevation (`baseline_elevation`) and buoyancy
  (`buoyancy`) are worked out by two different isostasy models.
- **Nothing erodes.** Thick crust thins only by rifting, margin erosion or
  delamination. Old orogens never wear down, and nothing builds shelves.
- **Plumes ride with the plate**, so there are no age-progressive hotspot
  chains.

## Tickets

| #  | Ticket                         | Size | Changes dynamics? |
| -- | ------------------------------ | ---- | ----------------- |
| 14 | Isostatic elevation            | M    | No (output only)  |
| 15 | Eustatic sea level             | S    | No (output only)  |
| 16 | Erosion and sediment           | M-L  | Yes (moves crust) |
| 17 | Hotspot tracks                 | S    | Slightly          |

Sizes are rough: S is a few days, M about a week, L more. Most of the effort
in 14-16 is calibration, not code.

## Sequencing

```mermaid
flowchart TD
  t14["14 Isostatic elevation"] --> t15["15 Eustatic sea level"]
  t15 --> t16["16 Erosion and sediment"]
  t17["17 Hotspot tracks"]
```

- 14 before 15: the sea-level solve needs elevation to be a function of the
  column, and the water load ties them together (see 15).
- 15 before 16: erosion needs a base level to erode down to and to deposit
  against.
- 17 is independent. It is the smallest, so it makes a good warm-up, or it can
  run alongside 14.

## Decisions (locked)

1. **Player edits are out of scope.** They belong to a later game stage that
   runs after the simulation, so derived elevation needs no override path
   now. Whatever that stage does, it can work on the finished world's crust.
2. **Values come from the system, not from an era.** Nothing is tuned to hit
   a land share, a sea level or a hypsometry at some chosen point in time.
   Calibration fixes only the physical inputs: material densities, the water
   load, the volume of water, how keels and lids weigh. Its tests are about
   individual columns: given this crust and lid, the column stands here.
   Land area, sea level and the rest are outcomes to report and watch, not
   targets. Earth figures appear only as sanity ranges for those inputs.
3. **The sea-level slider goes.** Its replacement is an ocean volume control
   in the same place. Changing it re-solves sea level and re-colours the map
   at once, even while paused (see 15), so it handles like the slider did,
   but the result is physically consistent.
4. **Sediment tracer: yes.** Add a `sediment_share` field (see 16) so hot-era
   drip does not founder sediment as if it were mafic crust.

## Cross-cutting rules

- Every new tuning field goes in `sim-rs/src/tuning.rs` and
  `src/sim/tuning.ts`, documented in the TypeScript as the others are, and in
  the tuning UI where it is worth dialling.
- Mass that moves between cells is a permutation or a transfer, never a
  creation. `mass_ledger_closes` and the drift readout stay at zero.
- The four-thread vs one-thread fingerprint test must still pass: reductions go
  through `par::sum`, and any sort breaks ties by cell index.
- Before each ticket, record a bench baseline:
  `npm run bench -- level:6,seeds:12,ticks:1000 before.json`. Afterwards,
  compare with `npm run bench:compare`. Tickets 14, 15 and 17 should leave
  every dynamics metric inside its band. Ticket 16 will move some, and the
  bands are updated deliberately in that ticket, with the reasons recorded.
- Rebuild both WASM packages (`build:wasm`, `build:wasm:threads`).

## Out of scope this iteration (future work)

- **Supercontinent cycle check.** The level 6 run recorded 0 continental
  breakups (two continent-bearing plates splitting) in 2 Gyr, along with 0
  weld merges and 0 passive-margin collapses. Run a multi-seed ensemble
  (`sim-rs/target/ensemble.py`) before deciding whether anything is wrong.
- **Absorbed fragments (microplate specks).** ~2,700 pieces pinched off a
  plate and absorbed in 2 Gyr, over half of them a single cell, are still
  there. They come from the transport, which moves crust a whole cell at a
  time: a plate that claims a margin cell (`advect.rs`, pass 1) can strand a
  protruding cell of the plate it takes from, and the ridge hand-over (pass
  2d) leaves cells of fresh floor alone. Each is absorbed by the next
  identity pass, so they do no harm beyond the count. Refusing the claims that
  strand a cell removed only ~10% of the specks at the cost of a changed
  trajectory, so it was dropped. A fix means moving crust by less than a cell.
- **Ridge-transform segmentation.**

## Done after this iteration

Taken from the list above once tickets 14-17 were in. Each was checked against
`sim-rs/src/bin/bench.rs` over twelve plate layouts at level 6, 1000 ticks (2
Gyr), against the commit before.

- **Arc placement and back-arcs** (`arcs.rs`, `arc_setback`, `back_arc_width`,
  `back_arc_extension`). Accreted crust now lands `arc_setback` (0.025 rad,
  ~160 km) inland of the trench, counted in whole cells (one ring at level 6,
  none below level 5). Where a slab dips beneath a plate that is moving away
  from its trench, the band behind the arc stretches and what it thins goes
  onto the arc. Thinned to ocean thickness it reverts to ocean, a marginal
  sea. Mass is moved, never made. Plates, area, rifts and ocean age stayed
  inside the spread of the baseline.
- **Microplate jitter** (`microplate_area`, `microplate_damping`). Plates under
  1% of the globe settle onto their force target up to 6x more slowly,
  tapering to none at 1%: their targets are a few cells of slab pull, which
  change with every cell the margins move. Heading reversals fell from 218 to
  46 (212 to 37 on small plates), and in a regime with many small plates from
  784 to 375. Plate count, area, rifts and fragments did not move outside
  noise. The absorbed fragments are not fixed (see above).
- **Continental area** (`SimOptions.hold_continental_area`,
  `continental_area_target`, `continental_target_time`,
  `continental_target_gain`). The target is off by default, and nothing reads
  it: the area comes from arcs, plumes, sediment and the fixed
  `continental_recycling_rate`, and landed at 13.3% against 13.4% under the old
  controller. With the option on, margins recycle what everything else grows
  plus the gap to the target over `continental_target_time`, and arcs accrete
  more of the ocean they consume while below it (the old controller could only
  recycle less, and sat at 14% against a 40% target). Targets of 0.2, 0.3 and
  0.4 settled at 19.9%, 29.9% and 39.3%; 0.5 and 0.65 both reached ~42%, the
  natural ceiling at the default accretion.
- **Plume drift** (`plume_drift`). Hotspot tails are carried by the mantle
  flow beneath them, `plume_drift` times its speed (0.7 cm/yr on average, 1.5
  at most), instead of staying fixed on the mesh. Heads still ride with their
  plate. At 1, a chain can bend where plate motion and mantle flow part, as the
  Hawaii-Emperor chain does (not yet checked by eye at level 7). No
  systematic effect on the dynamics metrics over 12 and 24 layouts.
