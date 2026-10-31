# 19 — Rigid-motion advection: crust moves with its plate, not the lattice

## Outcome (31 Oct 2026): abandoned, code reverted

The user judged per-plate frames a dead end. Every code change was reverted
to commit `78f0da5`. This note is kept as the record of what was tried.

The code is in a git stash, `stash@{0}` "frames attempt (todo/19), abandoned
31 Oct 2026", made on `crust-composition-experiment`; the stash index shifts
if more stashes are added. It holds 15 files, including the untracked
`sim-rs/src/frames.rs` and `sim-rs/src/bin/motion.rs`. Restore it with
`git stash apply` against that commit. The earlier marker attempt (below)
was saved as a patch outside the repo and is not in the stash.

What was tried, in order (details in the sections below):

1. **Sub-cell markers carried by each column** within the old cell-to-cell
   motion. Interior seams went away. Plates spinning about a pole inside
   themselves jammed, though: columns in closed loops found no free cell
   ahead. Five matching schemes failed, so it was reverted.
2. **Per-plate frames** (`frames.rs`, `SimOptions::plate_frames`). Each plate
   keeps its crust in its own frame, turned exactly, and the world's cells are
   rebuilt from the plates each step. It removed the seams and hex rings:
   interior welds went from 40–50 per tick to 0, and roughness from about 6 to
   3.5. But it broke much of what the model had been balanced on. Fixes were
   attempted for each:
   - collisions that let the deepest-keeled plate collect every continent
     (collision lock, mountain-belt push);
   - boundary jitter that stopped plates merging;
   - blending that thinned continental crust;
   - too few plates and Pangaea-style clumping (ocean-plateau collapse,
     in-ocean trench initiation);
   - rifts that merged straight back into their parent.

   Each fix uncovered the next mismatch: the merge, rift and collision rules
   had been calibrated against artefacts of the cell-to-cell motion. Transport
   also cost about 7× the old motion, about 2× for the whole run.

**Why it was dropped:** the seams were fixed early, but it took a long series
of rebalancing to stand still on plate count, continental shape and rifting,
with more of it still open.

**Not frames-specific, and could be worth reapplying to the old motion** (all
in the stash; each needs re-measuring with frames off):

- **Rift test and breakup venting** (`rifting.rs`, `separation()`). The test
  weighed the dome push before breakup vents half the heat beneath
  (`rift_heat_release`), so heat-driven rifts were accepted on a push the
  pieces lose at the split. Fix: the dome push at the heat left after venting.
  Test: `a_rift_that_would_vent_all_its_heat_does_not_open`. Under frames,
  merge-backs fell from about 30% to 8%. Under the old motion the effect was
  at noise level (one seed).
  **Reapplied to the old motion (31 Oct 2026, uncommitted).** Measured at
  level 6 over 4000 Myr, seeds alpha, beta and gamma, app options:
  - rifts: 35/57/44 before, 35/59/40 after;
  - rifts merging back: 5 before, 4 after (out of about 120 births from
    rifts);
  - continental share: 0.147/0.134/0.163 before, 0.146/0.135/0.163 after;
  - plates at the end: 45/72/62 before, 78/51/58 after. This is run-to-run
    divergence, with no consistent direction.

  This is a consistency fix with no measurable effect on outcomes here. The
  old motion's first-step gap already kept weakly driven halves apart.
- **Old ocean floor founders against buoyant plateaus, not only against
  continents** (`passive_margins.rs`, with `composition_crust_type`).
- **Collision lock and mountain-belt (orogen) push** (`plate_forces.rs`). These
  were built for frames' collisions and are untested on the old motion.

For the original seams, the cheaper options from the 30 Oct status section
are still open: old synchronous stepping with targets chosen by marker, or a
relaxed (smoother) mesh.

## Goal

Make each column of crust go where its plate's rotation actually carries it,
so plate interiors stop picking up straight seams and hexagonal rings. Keep
every invariant that stopped speckling, plate-colour bleed and corduroy
striping.

## Why it matters (diagnosis, 30 Oct 2026)

From a saved level-7 world at about 2600 Myr, run on for 400 ticks with the
interior events logged:

- **Straight seams are thickness steps on lines fixed to the mesh.** When a
  plate steps, every cell moves exactly one cell, towards the neighbour whose
  direction best matches its velocity (`advect.rs:371-428`). Where the mesh
  is irregular, two columns pick the same target and one is left with nowhere
  to go. Pass 3 welds that crust onto its own cell, so the crust thickens.
  This happens where the cell directions kink: icosahedron face edges and the
  edges of the coarser subdivision levels (midpoint-and-normalise
  subdivision). 98% of welding inside plates fell where a cell's velocity
  points almost exactly between two neighbours ("tie zones"), which are only
  19% of plate interiors. Crust that has crossed one of these lines is
  thicker than crust that hasn't, and that is the step on the map. The
  meridian seam near the south pole lies on two subdivision edges that meet
  at an icosahedron edge midpoint.
- **Hexagonal rings around Euler poles.** A plate steps when its *fastest*
  crust has gone a cell, and then every cell moves one cell. Crust near the
  Euler pole, which should barely move, moves as far as the edge, so the
  rings of cells around the pole spin as blocks.

Smoothing the elevation map, or smoothing the mesh on its own, would leave
both causes in place (face edges and pole over-rotation remain).

## What must not come back

Each line below is a failure from tickets 05 and 12 or from `8a0f59b`, with
the invariant that prevents it. All of them stay.

| Past failure | Cause then | Invariant kept |
|---|---|---|
| Corduroy striping (rows of crust copied twice or dropped) | Resampling a rotation onto the mesh every tick with nearest-cell lookup, which isn't one-to-one | **Transport stays an exact permutation.** Each column goes to at most one cell, and each cell takes at most one column. No resampling, no gather from a back-rotated position. `transport_is_a_permutation` stays green. |
| Speckle: islands of a stale plate inside another | A hole filled with its own old crust after the surrounding cells changed hands | **Ownership changes only in the existing passes** (1, 2b, 2d), each with its foothold rule (`MIN_FOOTHOLD`, two-cell foothold in 2b). Moving within a plate never changes `plate_id`. |
| Subducting crust surviving on the overriding side | A lone foreign column accepted as winner | Pass 1 and pass 3 unchanged: foreign crust enters only across a convergent margin the plate overrides; otherwise it subducts or welds via the ledger. |
| Plate colour bleeding across a boundary in dashed rows | Holes filled from the dominant neighbouring plate | Holes inside a plate are filled from the same plate only (pass 2c), as now. |
| Fronts fraying into lobes and fingers | Polarity decided cell by cell | Polarity stays decided for the **whole margin at once**. See step 4: with markers, fewer cells cross per substep, so the tally must use the whole margin, not only this substep's contacts. |
| Trails behind moving plates | A vacated trailing cell refilled with the departing plate's crust | Trailing holes still go through 2b, then 2d, then rift. |

## Approach: carry a sub-cell marker with each column

Each column stores where its crust really is, as an offset from its cell's
centre. The marker rotates exactly with the plate. The column moves to a
neighbour only once its marker is closer to that neighbour's centre than to
its own. Moves stay one cell at a time, as a permutation, but *when* and
*which way* now come from geometry instead of the lattice:

- Near an Euler pole a marker barely moves, so the column stays (fixes the
  hex rings).
- Direction comes from which cell the marker is now nearest to, not from
  which neighbour's direction best matches the velocity, so a kink in the
  mesh no longer sends whole rows the same way (fixes the seams).
- The marker keeps the remainder after a move, so mismatches don't build up:
  a column that can't move this substep lags and moves on a later one, and
  its crust is not welded.

Considered and rejected:
- **Semi-Lagrangian resampling from the accumulated rotation.** This is the
  ticket 05 scheme that produced corduroy and speckle.
- **Fully Lagrangian per-plate sheets that are rasterised to the grid.** This
  is the standard approach in the literature, but rasterising overlapping
  plates each tick reintroduces boundary speckle, and it means rewriting
  every pass that reads the grid.

## Steps

### 0. Baseline harness (before any change)

Add a diagnostics mode to `bench.rs`, kept for later regression checks:

- Welds inside plates per tick (cells two rings from any boundary with
  `welded > 0` and not overrun), and the share of them in tie zones.
- Holes inside plates (`interior_holes`) and how many are still unfilled.
- **Speckle count:** cells with fewer than 2 neighbours on the same plate;
  plate components under `min_fragment_area`; `fragment_count` and
  `absorbed_fragment_count` per 100 Myr; the existing craton "speckled" share.
- **Boundary roughness:** plate perimeter² / area (`metrics.rs:289` already
  sums perimeters).
- Mass drift, and the pass timings for advection.

Record the numbers for 3 seeds × 1000 ticks at level 7 and save them in this
ticket, so after-change runs compare against the same baseline.

**Done (30 Oct 2026)** as its own binary, `src/bin/motion.rs`
(`cargo run --release --bin motion -- 7 1000 4 seed=<name>`). Welds and holes
are counted on the last substep of each tick only (about a third of all
substeps); the after-change runs are counted the same way.

| Seed | Interior welds/tick | In tie zones (zones' share) | Interior holes/tick (unfilled) | Lonely cells | Detached cells (pieces) | Fragments / absorbed per 100 Myr | Roughness | Plates | Advection ms/tick |
|---|---|---|---|---|---|---|---|---|---|
| alpha | 45.3 | 99.2% (18.0%) | 280 (2.2) | 52.7 | 65 (9.6) | 7.5 / 276 | 5.99 | 57 | 46.1 |
| beta | 39.8 | 99.2% (17.5%) | 239 (1.8) | 49.1 | 52 (6.4) | 8.0 / 296 | 6.43 | 44 | 44.6 |
| gamma | 49.9 | 99.5% (19.5%) | 276 (1.1) | 60.2 | 139 (14.8) | 6.9 / 287 | 6.30 | 40 | 45.7 |

Mass drift about -2e-7 on all three. Mean substeps per tick about 3.

### 1. Marker state

- Add `marker: Vec<[f32; 3]>` (a tangent offset from the cell centre, in unit
  sphere units) to `CellData`, saved with the world. Pass 2 gathers it like
  the other per-cell fields, and its value is re-expressed relative to the
  new cell when a column moves.
- Reset it to zero for fresh floor (`lay_new_floor`), rift placeholders and
  primordial initialisation. Welded crust takes the host column's marker.
- Plate merges, reassignments and rift splits keep markers as they are; the
  next substep rotates them with the new plate's ω.

### 2. Motion from markers

- Each substep, rotate every live column's marker by its plate's
  `ω · dt_step`.
- `forward[cell]` = the neighbour whose centre is nearest the marker, if it
  is nearer than the cell's own centre; otherwise -1 (staying). This replaces
  the per-plate `pending`/`steps` gate (`advect.rs:371-389`) and the
  best-direction neighbour (`:396-428`).
- Keep the substep rule (`world.rs:318-343`): the fastest 95% of the globe
  moves at most about one cell per substep, so moves remain neighbour to
  neighbour.

### 3. Matching within a plate

- Pass 2 score: distance from the source column's marker to the destination
  centre (closest wins), replacing velocity alignment plus the `forward`
  bonus.
- A destination is free only if its own column is leaving (or it is a margin
  cell taken in pass 1). Columns whose target isn't free **stay and lag**.
  Repeat until no claimant is left holding a cell whose occupant couldn't
  leave (a cascade; cycles are fine). Cap the iterations and count any that
  hit the cap.
- Pass 2c (holes inside a plate) stays as the safety net. Its donor becomes
  the same-plate neighbour whose marker is nearest the hole, instead of the
  neighbour directly "behind" by velocity.
- Pass 3 must then see almost no unplaced columns inside plates. Count the
  ones it does see.

### 4. Margins and locked fronts

- Work out the pass 1 polarity tally (`margins`, `advect.rs:≈500`) per plate
  pair from **all** convergent arcs between them, not just this substep's
  contacts, so the whole front keeps one polarity.
- Stalled columns (pass 1b) don't move their marker past the front: clamp the
  marker's component along the motion so it can't build up a burst of
  pent-up movement.
- Check that fronts don't fray now that leading cells cross one at a time
  rather than all together. The foothold rule should hold this; watch the
  roughness metric.

### 5. Ridge hand-over (pass 2d)

`hands_over_ridge` alternates trailing holes between the two plates using
plate step parity, which no longer exists. Replace it with a rule based on
geometry: give the hole to the plate whose nearest bordering marker has
pulled further from the hole's centre, so the ridge stays midway. Test: a
symmetric two-plate split keeps its ridge within one cell of the midline
over 500 Myr.

### 6. Validate against the baseline

Acceptance criteria below. Also do a visual check of the same seed in
Plates, Elevation and Crust views at 500 and 2000 Myr.

### 7. Clean up

Remove `pending`/`steps` from `PlateData` if nothing else uses them, rewrite
the header comment in `advect.rs` and the comment on pass 2c (which
describes the old lattice behaviour), and move this ticket to
`completedTasks/`.

## Acceptance criteria

- **Seams:** welds inside plates per tick drop by at least 10×, and their
  tie-zone share falls to about the tie zones' share of area (~20%) rather
  than 98%. No straight-line thickness steps on face or subdivision edges in
  a 2000 Myr run.
- **Rotation:** a single plate spun 360° about an interior pole returns every
  interior column to within one cell of its start, with interior thickness
  unchanged (new unit test).
- **No speckle regression:** cells with fewer than 2 same-plate neighbours,
  small components, fragment counts and the craton speckled share are no
  worse than the baseline (within seed-to-seed spread). Boundary roughness is
  no worse.
- **Permutation:** `transport_is_a_permutation` passes, and a new test checks
  that no `plate_id` changes outside passes 1, 2b and 2d.
- **Markers stay bounded:** 99.9% of markers are within 1 cell width of
  their cell's centre. A lagging column never stays put for more than a few
  substeps.
- **Mass:** drift stays within the existing test tolerance.
- **Cost:** advection pass time is no more than 1.5× the baseline.

Continental growth, plate counts and other outcomes may shift because crust
now moves correctly. Report them against the baseline; don't tune them back
to the old values.

## Dependencies

- 05 and 12 (the invariants above), 10 (substepping).
- Saves from before this change won't load (the source hash changes anyway).

## Out of scope

- Smoothing the mesh (e.g. relaxing the cells to even spacing). Worth
  revisiting only if seams remain after this change.
- Changing how plates are driven (`plate_forces.rs`) or how boundaries are
  classified.

## Open questions

1. Lag versus early fill. When a hole inside a plate opens and its nearest
   marker is still closer to its own cell, should that column move early
   (2c) or should the hole wait a substep? Early fill keeps the permutation
   without empty cells; waiting would need an "empty but owned" state, which
   nothing downstream understands. The plan assumes early fill.
2. Whether pass 1's per-margin tally should be cached per tick from
   `boundaries` (cheaper) or recomputed per substep.

## Status (30 Oct 2026): approach blocked on rotation, needs a decision

Built on the working tree (uncommitted; patch also saved outside the repo):
markers on `CellData`, marker rotation and lag clamp, margin-wide polarity
tally, forward-only moves, ridge hand-over by across-ridge crust age, and
three successive designs for pass 2. Tests: everything passes except the new
spin test and, intermittently, `arcs_leave_their_mark...` (arcs are suppressed
longer because the mantle runs hotter; see below).

What worked, measured with the first pass-2 design at level 7 (3 seeds × 2000 Myr):

- Interior welds per tick: 40–50 → 0.07; their tie-zone share fell from 99%
  to chance. The seam mechanism is gone.
- Lonely cells 49–60 → 38–40; detached cells 52–139 → 5–18; roughness
  6.0–6.4 → 3.7–3.8. No speckle regression; better on every measure.
- Ridge test passes once only across-ridge crust is compared.

What did not:

- **Closed flow.** A plate spinning about a pole inside itself moves its crust
  in closed loops. Matching columns to cells one step at a time on the
  irregular mesh keeps leaving a column with no free cell ahead, which holds
  up the whole loop or chain behind it. Tried in turn: hold-and-cascade,
  cost-checked push chains, whole-chain and whole-loop shifts, and augmenting
  paths (up to 1024 columns deep). The best reached 1.2 cells of lag after an
  eighth of a turn, growing to ~15 cells by three quarters, with up to 4% of
  the cap refilled with fresh crust. Plates with interior poles are common, so
  this is not shippable.
- Interior holes rose ~20× and advection cost ~4× (first design, level 7).
- Crust turnover fell ~30% (old motion moved every cell at its plate's
  fastest speed), so the mantle runs hotter and arcs are suppressed longer.
  That is a correct consequence, not a bug, but it shifts calibrated outcomes.

Options for the next decision:

1. **Per-plate frame.** Keep each plate's crust in its own fixed frame and
   rotate the frame exactly; only cells at plate edges are ever resampled.
   Interiors cannot jam, lag or seam. Largest change; edge resampling must
   keep the ticket 05/12 invariants.
2. **Old synchronous stepping, with targets chosen by marker.** Plates keep
   stepping as wholes (no jams); each cell's target comes from its marker
   instead of the best-aligned direction, and a cell whose marker has not moved
   half a cell stays. Smaller change; would fix the hex rings, and probably
   most seams, but conflicts in tie zones would still need resolving.
3. **Smoother mesh only** (relax the cells). Removes the subdivision-edge
   seams, not the face-edge ones or the hex rings. Cheapest; partial.

## Decision (user, 30 Oct 2026): per-plate frames

The marker work-in-progress was reverted (patch kept outside the repo).
`frames.rs` implements option 1 behind `SimOptions::plate_frames` (default
off; UI toggle "Plates carry crust in their own frames (experimental)").

How it works:

- Each plate keeps its crust as columns on its own copy of the mesh (body
  frame) with an exact body-to-world rotation (quaternion). Moving a plate
  turns the rotation and nothing else.
- Each step the world's cells are rebuilt as a view. A plate covers a cell if
  the body cell under it holds a column. Each plate's columns are matched one
  to one with the cells it covers, nearest first, in a few rounds of claims.
  Solved fresh each step from the body frame, so it cannot jam or drift.
  Cells vary in size over the mesh, so a plate can land on more cells than it
  has columns there, or fewer: a cell left over is lent a small share by every
  matched column within two cells (a ~5% thinning spread over them, not a
  hole or a half-thickness cell), and a column left over spreads over the
  cells within two of it. The view shows all the crust, nothing twice.
- Two plates covering a cell across a closing (convergent) boundary: the one
  that rides over keeps it, by the same rules as before (continent over
  ocean, keel, thickness, then the margin-wide buoyancy tally), and the
  other's column goes down or welds on through the shared `overrun` code
  (pass 3's). Elsewhere (sliding boundaries, edge jitter) the cell's last
  plate keeps it and the other's column waits unshown; under another plate
  for more than three steps, it is overrun.
- Cells no plate covers open as new floor; along a ridge, the side with the
  older crust across the ridge takes it, so the sides alternate.
- Everything after transport (`advect::settle`, pass 4 onwards) and every
  other module runs on the view unchanged. At the start of the next step the
  view is written back to the columns: each cell's gain or loss of crust,
  and of each component, is shared among the columns it showed by what each
  lent, so mass and composition tracers stay exact and no column gives up
  more than it lent. A cell that changed plate takes its column with it; a
  plate new since the last step takes its parent's frame, so a split is
  exact.
- Saved with the world (`State.frames`).

Verified so far:

- With frames off, worlds are bit-identical to before (fingerprints), after
  splitting `advect` into transport and `settle` and sharing `overrun` and
  `begin_step`.
- All 173 tests pass with frames on, including mass ledger, restore-exactly,
  plume tracer, erosion conservation and ridge sharing.
- New test `frames::tests::a_plate_spinning_about_itself_...`: a cap spun a
  full turn stays within 0.37 cells of the exact rotation on average (worst
  1.8), with no drift and no new crust inside it.
- Level 5: interior welds 8 → 0 per tick; mass drift about 1e-9; gaps inside
  plates about 5 cells a tick, all closed from their rims.

### Level 7 against the old motion (31 Oct 2026, threaded build, 3 seeds × 2000 Myr)

| | Old (alpha / beta / gamma) | Frames (alpha / beta / gamma) |
|---|---|---|
| Interior welds per tick | 45 / 40 / 50 | 0 / 0 / 0 |
| Gaps inside plates per tick (unfilled) | 280 / 239 / 276 (1–2) | 68 / 64 / 69 (~0) |
| Lonely cells | 53 / 49 / 60 | 27 / 32 / 30 |
| Detached cells (pieces) | 65 (9.6) / 52 (6.4) / 139 (14.8) | 15 (3.0) / 74 (6.9) / 39 (5.9) |
| Roughness | 6.0 / 6.4 / 6.3 | 3.4 / 3.6 / 3.5 |
| Plates at the end | 57 / 44 / 40 | 25 / 26 / 23 |
| Subducted per Myr (share of crust) | 0.0067–0.0068 | 0.0097–0.0104 |
| Mass drift | ~2e-7 | ~6e-7 |
| Transport ms per tick | 24 | 178 |
| Whole run | 103–127 s | 231–237 s |

Open: transport is ~7× the old one (whole simulation ~2×); fewer plates and
~50% more crust turnover; seed beta has more detached cells than before.

### After the user's test run (31 Oct 2026)

The user's run (level 6, 11,724 Myr, frames on): one plate covered 50% of the
globe and carried every continent (90% continental, moving at about a tenth of
the others' speed). Continental shapes were rounded, the oceans had no islands
or arcs, and the simulation ran slower.

Level 6, seed bench, 6000 Myr, `motion ... series=250`:

| | Old | Frames |
|---|---|---|
| Merges | 349 | 16 |
| Rifts | 454 | 483 |
| Continental area at the end | 37% | 16% |
| Felsic mass at 1000 Myr | 9.6 | 5.6 |
| Felsic on oceanic cells at 600 Myr | 34% | 50% |
| Subducted per Myr | 5.1e-3 | 1.1e-2 |
| Roughness | 4.6 | 3.0 |

Causes found:

1. **Collisions.** A buoyant column that lands under another plate at a
   closing margin is overrun, and `overrun` welds it onto the rider. The old
   motion stalls the front instead (pass 1b), so its own plate shortens and
   the contact lasts. Under frames, the plate with the deepest keel takes
   every continent it meets, which is how one plate ends up with them all.
2. **Boundary jitter.** Between one tick and the next, the boundary between
   two plates swings by up to 25% (331 → 270 → 381 arcs). Continental contact
   keeps dropping to zero and the suture timers in `sutures.rs` reset, so
   none reaches `suture_merge_time`. With frames off, the same pair builds
   206 Myr of unbroken suture.
3. **Blending.** About 5% of cells get no column each step, and spare columns
   spread over 2 rings, so about half the view is a mix of columns every step.
   Crust type and erosion work on that mix. Turning off the spare-column
   spread alone did not change continental growth, so the cause is the
   borrowing by cells left without a column, or something not yet found.
   Unproven.

Quiet subduction (without an arc) is under 1% of subducted mass, so it is not
why less felsic crust is made.

Rifting: from the user's saved world, neither motion rifts the big plate in
600 Myr, so the failure to break up is a property of the state. Frames
produced that state, but frames do not stop rifting directly; the bench run
rifts at the normal rate.

### Collision fix tried (31 Oct 2026)

`frames.rs` now applies the old pass 1 rules at a closing margin (see
`barred`):
- ocean never overruns buoyant crust;
- nothing overruns a craton except a deeper-rooted one;
- two continents never stack beyond `max_crust_thickness`.

A column held up this way is folded into the column behind it in its own plate
(`fold_behind`), instead of welding onto the rider. Tests pass in both modes.

Level 6, seed bench, 6000 Myr, frames on, before and after the fix: merges
went from 16 to 20, and continental area at the end from 16% to 11.5%. The
largest plate reached 52% at the end. It made no real difference.

Why contact is lost (`flicker` probe, 400 ticks): frames lose all continental
contact on 38% of pair-ticks, against 12% for the old motion. The main reason
is a contact cell that now holds an ordinary oceanic column of its own (median
7 km thick), so it is not thinning by blending. The contact edge itself moves
from tick to tick.

### Diagnosis by stage (31 Oct 2026)

**Correction.** Every run above used `SimOptions::default()`. The app turns on
`composition_crust_type`, `two_sided_tension` and `quiet_by_mean_speed`. Under
composition crust type the rules for what counts as continental differ, so
the continental-growth numbers above do not apply to what the user sees.
`motion ... app=1` now runs with the app's options.

**Tools added:**
- `Sim.probe` (`world.rs`): when set, it records crust type, felsic mass,
  thickness and evolved share after each stage of a tick.
- `frames::tests::shape_kept_in_transport` (ignored): an irregular continent
  carried by transport alone.

**Transport alone keeps shapes.** Frames give 0.93 overlap with the exact
rotation and keep the islands and the peninsula. The old motion gives 0.82 and
loses most of the islands.

**Same state (1000 Myr, app options), 100 ticks each way, per tick:**

| | Frames | Old |
|---|---|---|
| Transport: cells changing type, lost / gained | 90 / 106 (49 in small features) | 19 / 19 (13.5) |
| Lifecycle reverts | 17 | 0.2 |
| Continental cells after 100 ticks (from 182) | 459 | 352 |

Of the frames reverts, 1194 of 1530 are cells with no column of their own,
reverted for not being felsic. They borrow a thin mix from their neighbours
but take the crust-type flag of their biggest lender.

**6000 Myr, app options:**

| | Frames | Old |
|---|---|---|
| Continental area | 4.1% | 16.5% |
| Merges | 22 | 238 |
| Felsic mass | 8.9 | 10.0 |
| Felsic on continental cells | 57% | 55% |
| Continental mean thickness | 40–60 km | 23–30 km |
| Mass drift | -4.8e-6 | 4e-8 |

Frames make about as much felsic crust, but pile it into small, very thick
continents. The fold has no thickness cap, which is a suspect. The mass drift
under these options is a separate problem.

**Leading cause:** about 5% of cells each step get no column, and as many
columns get no cell. Lending blends those cells, and their crust type follows
whichever lender gives most, so coasts, small features and contacts flicker.
Proposed fix: pair each gap cell with a nearby spare column of the same plate,
with a bounded search, instead of lending.

### Blending removed, collisions shorten, mountain-belt push (31 Oct 2026)

- **No more lending.** A cell its plate covers but no column matched shows,
  whole, the column whose body cell lies under it, even if that column also
  shows elsewhere. Spare columns wait unshown. Gap cells and spare columns
  are not near each other (1320 of 1768 gaps had no spare of their plate
  within 6 rings), because cell size varies smoothly over each icosahedron
  face, so pairing them cannot work.
- **Ledger.** A column shown twice can be worn away beyond what it holds.
  `write_back` returns the shortfall, and it comes off that step's subducted
  tally. Without this correction the ledger drifted 6.6e-4.
- **True mass.** `Sim::lane_mass` / `Frames::lane_mass` give the true lane
  mass (columns plus changes to the view since it was built). The plume
  tracer test uses it.
- **Shortening.** A held-up column (`barred`) is shared among its plate's
  buoyant columns within `SHORTEN_RINGS` (4), up to the thickness cap. What
  does not fit is overrun (welded onto the rider). Folding into a single
  column had built piles up to 1434 km. A partial merge no longer turns ocean
  floor continental.
- **Mountain-belt push.** `orogen_push` (frames only, 1/35 of `slab_weight`
  per km above `crust_spread_start`) pushes apart plates that a thickened
  continental margin separates. Its effect is small: merges 17 without it,
  30 with it, on one seed.
- Tests: 173 pass with frames on and with frames off.

Level 6, app options, 6000 Myr, frames now (old motion in brackets):
- continental thickness 31–44 km (23–30 km);
- rifts 44 (47);
- merges 30 (238);
- continental area 2.6–5.6% (16.5%);
- largest plate swings between 27% and 90%;
- mass drift -2e-6, and -1e-4 with the push off. The drift is open.

At 100 ticks from the same state, continental growth is now equal under both
motions. The long-run shortfall in area and in merges remains.

### Plate count and continental area (31 Oct 2026, the user's 3rd export)

**Export.** Level 6, 4918 Myr, `riftStrength` 0.45. One compact, round
continent stands about 4 km high; 4.6% of the globe is continental; 15
plates.

**Passive-margin rename loop (a bug in both modes, fixed).** An ocean plate
carrying specks of continent counted their old coast, so its ocean body
broke away as a new plate. The specks left behind were too small to stand
alone and were absorbed into the new plate, which then qualified again on the
next tick. The plate was renamed every few ticks and lost its opening rifts,
sutures and quiet timers. Over 200 ticks from the export there were 162
collapses with frames and 62 with the old motion. `collapse_passive_margins`
now requires the largest connected piece left behind to reach
`min_fragment_area`.

**Three seeds (alpha, beta, gamma), level 6, app options, 4000 Myr:**

| | Frames | Old |
|---|---|---|
| Plates | 4–11 | 20–37 |
| Largest plate | 66–96% | 18–32% |
| Continental area | 1.8–3.9% | 15–17% |
| Rifts | 18–32 | 25–30 |
| Merges | 14–27 | 92–109 |

Plates consumed by 1500 Myr: 104 with frames, 46 with the old motion. Frames
plates move about twice as fast.

**Continental budget**, same start, 1000 ticks, net continental cells per
tick:

| | Frames | Old |
|---|---|---|
| Lifecycle | +8.7 | +4.3 |
| Transport | -7.0 | -1.8 |

Within transport under frames, over 500 ticks: 828 continental columns were
shortened at held-up fronts, and 668 were overrun or welded. Only 15 and 24
went by the two unseen-column routes.

**Contact (app options).** Contact is lost on 59% of pair-ticks with frames,
16% with the old motion. Most losses are a one-cell continental sliver
(usually arc crust) whose column simply moved one cell with its plate.

**Reading.** The old motion stalls converging continents in place, so they
keep their area and their contact, sutures form, the suture coupling stops
the plates, and they merge. Rigid frames keep converging: collisions eat
continental area, no lasting contact forms, the coupling never starts,
plates stay fast, and plates are consumed.

**Candidate next step.** Collision resistance as a coupling of the two
plates' relative motion across continental contact, from first contact,
instead of the present drag of each plate against the mantle. Not yet tried.

### Collision lock (31 Oct 2026)

`collision_lock` (frames only, default 1) is applied in `update_plate_motion`
after the suture coupling. It works per plate pair:
- Over the converging arcs where continent meets continent, it finds by least
  squares the relative turn that undoes the closing across them (with a 1e-3
  ridge), and removes it from the targets.
- The share removed is `collision_lock` × continental contact length ÷
  converging length.
- The correction is split between the two plates by the other's drag, so the
  larger plate gives way less.

Tests pass in both modes.

Three seeds, level 6, app options, 4000 Myr, frames with the lock (before it
in brackets):
- plates 2–7 (4–11);
- largest plate 57–91% (66–96%);
- continental area 3.4–5.4% (1.8–3.9%);
- merges 23–51 (14–27);
- rifts 20–58 (18–32);
- mean speed 3.4–6.2e-3 (the old motion runs at 2.3–2.9e-3).

Plates consumed by 4000 Myr: 129–232. A modest gain; the plates are still
consumed too fast.

The plate budget for alpha at 4000 Myr:
- made: 58 rifts, 165 fragments and 53 collapses;
- lost: 232 consumed and 51 merged.

Open: why frames plates move faster. Less continental keel drag is one
suspect (a feedback with continental area), not verified.

### Why frames plates are faster (31 Oct 2026)

From shared states (seeds alpha and beta, frames for 750 ticks, then 100 ticks
each way), plate speed under each force alone is the same in both motions for
mantle carry and ridge push. Slab pull alone gives about 1.5–1.7× more speed
under frames, yet the total slab pull is lower under frames (alpha 8.1 against
9.3, beta 4.3 against 5.4). The pulling margins are shorter too (13.1 against
16.5). Slab strength and the buoyancy of what goes down are similar. So the
difference is in the drag.

| Boundaries | Frames | Old |
|---|---|---|
| Total length | 39 (alpha), 18 (beta) | 53, 26 |
| Converging length | 12.4, 4.8 | 15.7, 7.1 |
| Collision drag weight | 749, 393 | 1062, 559 |

Mantle drag weight is about 400–420 in both.

**Conclusion.** Collision drag, charged per unit of converging arc length, is
the largest resistance. The old motion's ragged, zigzag boundaries
(roughness about 4.7 against about 2) are 27–28% longer, which slows its
plates to 1.5–1.8 cm/yr. Frames plates run at about 2–4.5 cm/yr, in Earth's
range.

The halved tension that drives rifting under frames (opposed pull 0.53
against 1.05 from the user's export) probably has the same cause: slab pulls
along a zigzag point many ways and partly cancel. Not measured directly.

The calibration of `collision_drag` and `rift_strength` has in effect
absorbed the mesh raggedness of the old motion.

Options put to the user:
1. Make boundary-length forces depend on smoothed boundary length and
   direction, not cell-edge sums, in both motions; then recalibrate against
   physical references.
2. Recalibrate the frames inputs only.

### Boundary smoothing tried and reverted; the real cause of plate loss (31 Oct 2026)

**Smoothing.** Boundary forces (slab pull, suction, collision drag, pushes,
two-sided tension, collision lock) were charged by smoothed length, from a
2-ring smoothed normal with each edge counted at `len × (n·n_smooth)`. Raw
over smoothed boundary length came out the same in both motions: 1.30 with
frames, 1.32 with the old motion. The old motion has more boundary because it
has more plates, not because its boundaries zigzag more, so the hypothesis was
wrong. Smoothing only rescaled every boundary force by about 0.77 in both
motions, so it was reverted. A copy is in the scratchpad
(`plate_forces.smoothed.rs`).

**Cause.** From a shared state (seed beta, 750 ticks), plate 96 starts at
3280 cells and closes at about 1e-2 rad/Myr along about 1.5–2.3 rad of
converging edge in both motions.

| Plate 96 | Frames | Old |
|---|---|---|
| Cells after 60 ticks | 36 (gone soon after) | about 2740 (flat from tick 24) |
| Its own buoyant cells on its converging edge | 0–9 | 11, rising to 30 |
| Mean closing at tick 60 | 1.1e-2 | 6.6e-3 |

In the old motion, buoyant crust gathering at a plate's front locks the front
(pass 1b), and the jam spreads back through every cell heading into it. The
plate keeps converging but stops being consumed. Plates 32, 40, 90 and 97
show the same: gone under frames, levelling off under the old motion.

Frames consume ocean at the closing rate and accrete or shorten buoyant
blocks, which is closer to the physics. But plate creation (rifting,
collapses, fragments) was calibrated while that jam was keeping plates
alive, so with frames creation no longer keeps up with consumption.

### Missing plate creation (31 Oct 2026)

**Tension (candidate 3) is not the jam.** On shared plates, tension under
frames stays near 0–0.18 while under the old motion it climbs to 0.4–0.66.
Total slab pull on the plate is similar (plate 85: frames 3.3–3.8, old
2.8–3.7), and the pull from slabs beside the plate's own buoyant crust is
about 0 in both. The difference is layout: the old motion keeps the plate
whole, so it keeps 11–12 rad of down-going margin pulling from opposite
sides. Under frames the plate is being consumed, its down-going margin
shrinks from 9.8 to 7.0 rad, and the pull ends up mostly one-sided. So the
low tension follows from the faster consumption.

**Collapse on any plate (candidate 1).** `collapse_stagnant_only = false`,
three seeds, 4000 Myr, frames:
- alpha: 23 plates (7 without the change);
- gamma: 13 plates (2);
- beta: 3 plates (3), unchanged.

Continental area 3.1–6.2%; largest plate 45–96%.

Beta ends as one oceanic plate over 96% of the globe with 3% continent: it
has no continental coast to collapse and no continent to trap heat for
rifting. Nothing in the model can start a trench inside an ocean plate (old
floor foundering, or at a fracture zone), so an oceanic stagnant lid never
breaks. That is the next candidate for a missing mechanism, alongside
microplates from shrinking ocean plates (candidate 2).

Motion diagnostic switch added: `stagnant=0`.

### Old floor founders against plateaus too (31 Oct 2026)

Seed beta under frames ended as one plate over 96% of the globe: floor median
2045 Myr old, 89% of it heavy enough to sink, moving ~11 mm/yr. It held 256
rafts of buoyant ocean-type crust (plateaus and stranded arcs, ~18 km, 10% of
the globe) with 65 rad of old sinking floor around them, but passive-margin
collapse only counted a continent as the buoyant side, so nothing could start a
trench. Collapse now counts any crust that `resists_subduction` (unchanged
without `composition_crust_type`). References: Whattam & Stern 2015
(Caribbean plateau), Gerya et al. 2015 (plume-induced initiation). Test:
`old_floor_breaks_from_an_ocean_plateau_as_from_a_continent`.

Level 6, app options, frames, 4000 Myr (plates / continental / largest plate):

| seed  | before (approx.)  | gate on    | gate off  |
|-------|------------------|------------|-----------|
| alpha | 7 / 5% / 57%     | 10 / 12.5% / 38% | 6 / 13.8% / 37% |
| beta  | 3 / 3% / 96%     | 10 / 9.8% / 28%  | 9 / 9.2% / 73%  |
| gamma | 2 / 4% / 91%     | 12 / 13.0% / 27% | 6 / 12.3% / 70% |

`collapse_stagnant_only` stays on: off gives more plates early but ends with
a bigger lid. Old motion for reference: 20-37 plates, 15-17% continental.
Mass drift with frames still -0.8e-6 to -3.7e-6 (open).

### Rifts that rejoin their parent (31 Oct 2026)

Reported from the app: a plate splits, the halves travel together, then stitch
back. Tracked every plate birth over 4000 Myr, seed alpha, level 6, app
options (`dump/src/bin/splits.rs` in the session scratchpad):

- Frames: 22 of 64 rifts merge back into their parent (10 quiet, 12 weld,
  ~100 Myr later). Old motion: 2 of 57.
- The rejoining rifts are a distinct group, not frames slowing all rifts.
  Relative target motion of their halves just after the split is 0.0044
  rad/Myr against 0.036 for the rest. Rifts that stay apart are driven apart
  by slab pull (difference 0.033). Rejoining ones are heat-driven continental
  breakups: mantle carry, ridge push and dome push each differ by ~0.004 and
  partly cancel; slab pull 0.0016.
- Their shared edge moves at 0.0001-0.0005 rad/Myr (0.6-3 mm/yr) for life,
  below `merge_quiet_speed` (0.002, ~13 mm/yr). Their relative omega reaches
  0.0028, so the relative pole lies near the rift: they scissor rather than
  part. Continental contact stays along much of the line (66% of the edge one
  tick after the split, 41% at 50 Myr).
- Root cause: the rift test (`separation` in `rifting.rs`) accepts any
  positive parting rate, while the merge rules count a boundary slower than
  `merge_quiet_speed` as dead, and weld any continental contact with net
  closing once the 60 Myr breakup ledger (`dome_push_duration`) expires. A
  rift opening slower than the quiet speed is bound to be merged back. The old
  motion hid this: its first whole-cell step opened a cell of fresh ocean
  along every rift (no continental contact at t+1), whatever the forces.
- Ruled out: collision lock and drag across fresh breakups (removing them
  halves merge-backs at most, 11 of 38); edge coupling (off); suture coupling
  and lock change relative targets by <3%.

Follow-up, same day: the "root cause" above was only part of it. Added each
half's solved target, split by force part, along the shared edge one tick
after the split. In the rejoining rifts the halves close from that first
tick: mantle carry and ridge push close them (+0.001-0.005 rad/Myr each),
the dome push opens them (-0.001-0.004), and closing wins. The rift test
predicted opening from the same forces. The difference is the venting: a
split multiplies the heat under the plate by `1 - rift_heat_release` (0.5)
and the dome push scales with that heat, so the push halves at the moment of
the split. `separation` measured it before venting.

Fix: `separation` takes the dome push at the heat left after venting. Test
`a_rift_that_would_vent_all_its_heat_does_not_open` (fails without it).
Rift merge-backs, frames, 4000 Myr, app options:

| seed  | before        | after        |
|-------|---------------|--------------|
| alpha | 22 of 64      | 4 of 48      |
| beta  | 10 of 38      | 5 of 30      |
| gamma | 14 of 51      | 0 of 29      |

There are about 30% fewer rifts: heat-driven break-ups wait until the
heat left after venting can part the pieces. With old motion (beta), 1 of
55 before and 3 of 60 after, which is noise-level.

Tried and reverted:
- **A boundary that is opening never counts as quiet.** It removed beta's
  5 quiet merge-backs, but those pairs then welded later instead (beta 0→5
  welds, gamma 0→3). They open slowly for a while, then turn and close once
  the dome push expires (`dome_push_duration`, 60 Myr).
- **Weld closing judged over the whole shared boundary.** On alpha it gave
  no clear reduction (12 → 8 welds) and roughly halved the rift count,
  unexplained.

What's left: slow heat-driven pairs that part while the dome pushes, then
close under carry and ridge push once it stops. Whether 60 Myr of dome push
and the ridge push from a narrow new ocean are right is a separate question.
