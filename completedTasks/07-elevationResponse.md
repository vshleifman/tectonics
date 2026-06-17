What I implemented
Boundaries now build relief. A new elevation response turns the boundary
classification and density-driven motion of 04-06 into terrain: ridges at rifts,
trenches at subduction, arcs and collision ranges at convergence. Elevation
accumulates over deep time and relaxes back toward an isostatic baseline, so
relief grows while a boundary stays active and erodes once it goes quiet. All
the new work is a single pure function plus a one-line wiring change, so the sim
step stays a pure function over the mesh interface + flat typed arrays
(Worker/WASM-ready), matching advect/classify/plateForces.

Elevation response (new src/sim/elevation.ts)
applyElevation(mesh, data, dtMyr, boundaries) runs after advect on the freshly
advected back buffer, in two passes:

- Pass A - boundary forces. For each active boundary arc (boundaries.arcKind /
  arcClosing over the mesh's boundaryGraph), nudge the two cells it separates by
  an increment scaled by the closing speed |arcClosing| and dtMyr:
  - Convergent, both continental -> collision range: both sides rise
    (COLLISION_UPLIFT).
  - Convergent, otherwise -> the denser cell subducts (matching advect's and
    plateForces' density rule): it deepens into a trench (TRENCH_DEEPEN) while
    the buoyant overriding cell rises into a volcanic arc (ARC_UPLIFT). This one
    branch covers oceanic-oceanic (island arc) and oceanic-continental (Andean
    arc).
  - Divergent -> both cells are pulled toward the shallow mid-ocean-ridge crest
    (RIDGE_ELEVATION); a spreading centre shoals to a roughly fixed crest height
    regardless of rate, so this is a pull toward the crest, not an
    intensity-scaled push.
  - Transform -> no vertical change (sliding), so only relaxation applies.
- Pass B - isostatic relaxation. Every cell drifts toward its crust baseline by
  RELAX_RATE * dtMyr, then is clamped to the renderer's elevation range. This is
  what caps the accumulated uplift/subsidence (it plateaus instead of running
  away) and erodes inactive relief back toward isostasy over deep time.

Isostatic baseline (couples elevation to crust age/density)
baselineElevation(crustType, age, thickness) is the term Pass B pulls toward:

- Oceanic: lerp(RIDGE_ELEVATION, ABYSS_ELEVATION, age / MAX_OCEANIC_AGE) - young
  ridge crust sits shallow (~ -2.5 km) and deepens monotonically to abyssal
  (~ -6 km) as it cools, which is the "oceanic crust deepens as it ages"
  requirement.
- Continental: a buoyant base (CONTINENT_BASE) that rises with crust thickness
  above a reference (so accreted/thickened crust stands higher).

Seeding from the baseline (src/plates/PlateData.ts)
initialiseCrust now seeds each cell's elevation from the same oceanic baseline
(reusing the per-cell age it already assigns), instead of leaving the old random
-2..+2 jitter from seedUniformElevation. A fresh world therefore reads as
plausible ocean that deepens with age, and boundary relief grows from there.
MAX_OCEANIC_AGE is exported so the baseline stays in sync with the seeding.

Wiring (src/sim/step.ts)
step calls applyElevation(mesh, dst, dtMyr, boundaries) immediately after advect.
The boundary field is the same src-classified one advect consumes, so a
boundary's location is approximately stable across the step (the same
approximation advect already makes when gating subduction at the destination).

Decisions worth flagging
- Accumulate-plus-relax was chosen over recomputing elevation from scratch each
  tick: "boundaries build relief" implies relief that persists and grows, so
  mountains integrate uplift over many ticks and decay once convergence stops,
  rather than vanishing the instant a boundary goes inactive.
- The subducting (trench) side is picked by density, identical to advect and
  plateForces, so the three modules agree on which plate goes down.
- All magnitudes (RIDGE_ELEVATION, ABYSS_ELEVATION, CONTINENT_BASE,
  COLLISION_UPLIFT, ARC_UPLIFT, TRENCH_DEEPEN, RIDGE_PULL, RELAX_RATE, the
  elevation clamp) are module consts in one block - the 07 tuning sink - mirroring
  the const block in plateForces.ts. They are reasoned starting values, not
  empirically calibrated.
- No renderer change was needed: terrain shows immediately in the existing
  elevation colour mode (ridges light blue along divergent arcs, trenches deep
  navy and arcs rising toward land along convergent arcs). Elevation is not part
  of the mass budget, so the conservation drift readout is unaffected.

Known limitations / deferred
- Hard to fully judge correctness in isolation: convincing terrain depends on
  the rest of the loop (smooth motion, crust lifecycle, plate identity), so this
  is tuned to plausible behaviour rather than verified end to end. Expect to
  revisit the constants once 08-10 land.
- The continental rules (collision range, Andean arc, thickness-driven height)
  are coded but dormant until accretion mints continental crust in 08; today's
  all-oceanic runs exercise the subduction, divergent and transform paths only.
- Boundary kinds come from the pre-step classification and relief is written at
  the same cell indices on the post-step crust; with the ~2-cell boundary churn
  from 05 this can speckle slightly. The intensity scaling and relaxation keep it
  muted; genuinely smooth relief waits on the smooth-motion advection in 09.
- Trench and arc land on adjacent cells (cell granularity); the real inboard
  offset of a volcanic arc from its trench is beyond the current resolution.
