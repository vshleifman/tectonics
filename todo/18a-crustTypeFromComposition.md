# 18a — Crust type from composition (step D of ticket 18)

Detailed plan for step D in `18-continentalGrowthBalance.md`. Read that first
for why: late continental growth is mostly island-arc crust at ~10% felsic
that counts as continental purely because it is over 15 km thick, and the
continents end up ~50% mafic.

**Decision (user, 27 Oct 2026):** composition is the root. Oceanic crust is
mafic from ridge melting; continental crust is felsic to andesitic, built by
subduction-zone magmatism and reworking. Density and thickness both follow.

## Audit of `crust_type` (27 Oct 2026, working tree)

About 200 mentions in 24 files. Most are reads that are correct once type
means "evolved crust"; the work is in the few that stand in for thickness or
buoyancy. Line numbers are as of the audit.

### 1. Writers of the type (6)

| Site | Today | Under D |
|---|---|---|
| `crust_lifecycle.rs:238-266` `apply_crust_lifecycle` | Ocean → continental when `h ≥ continental_thickness_threshold` and (plume, `h ≥ nucleation thickness`, or ≥ 2 continental neighbours). Continental → ocean when `h < continental_revert_thickness` on a coast or at ocean-floor thickness. | Ocean → continental when basement evolved share ≥ `continental_felsic_share`; back when below `oceanic_felsic_share`, or once thinned to ocean-floor thickness. Neighbour rule kept for the margin-growth path only if it still makes sense (see open question 2). |
| `crust_lifecycle.rs:133` `infill_enclosed_basins` | Basin cell filled to the threshold turns continental. | Filled with shed rim crust, whose composition it takes (lanes mix); turns continental by the same composition rule. |
| `advect.rs:893` pass 3, eaten continental cell | Sets `dst.crust_type = CONTINENTAL` where continental crust is overrun and welded on. | Unchanged in effect: it is carrying evolved crust. Keep. |
| `advect.rs:1529` pass 4e hole closing | Takes the type of the majority of the rim. | Keep; lanes are copied from the donors, so composition and type agree. |
| `advect.rs:1596-1602` pass 4c rift stretching | New cell is continental if the pulled crust is ≥ threshold thick, else ocean. | Type from the pulled crust's composition (lanes already carried), not its thickness. |
| `lay_new_floor` (`:152`), `plates.rs:178`, `cohesion.rs:31` (swap), `advect.rs:851` (transport) | Fresh floor oceanic; init all oceanic; type moves with crust. | Unchanged. |

### 2. Thickness thresholds standing in for type

`continental_thickness_threshold` (15 km) is used in 6 sites in
`crust_lifecycle.rs`, 5 in `advect.rs`, 4 in `primordial.rs`, 3 in `heat.rs`,
2 in `craton.rs`, and once each in `metrics.rs`, `erosion.rs`, `arcs.rs`.
Two meanings are tangled:

- **Type rule** (goes away): nucleation and revert flips, `advect.rs:1166`
  `stays_continental` (weld keeps age and keel if the cell is continental or
  becomes thick enough), `:1596`.
- **Geometric floor for evolved crust** (stays, but renamed or reread):
  erosion floor (`erosion.rs:187`), basin infill rim floor, hole-closing floor
  (`advect.rs:1493`), rift flank floor (`:1545`). These stop thinning crust
  that is meant to stay continental. They keep using a thickness, but only for
  cells that are continental by composition.
- **Ridge ceiling** `RIDGE_CRUST_CEILING × threshold` (`heat.rs`), plume tail
  cap (`primordial.rs`): these keep ridge and tail crust short of the old type
  line. Under D there is no thickness line, so these become plain limits on
  how much melt a ridge lays (ridge crust stays mafic and oceanic however
  thick), to be reread with the plume work in step 3.

### 3. Reads that should use buoyancy, not the label

These decide what can go down a trench. Under D a thick mafic arc or plateau
is oceanic by label and must still resist subduction.

- `advect.rs:883-944` **pass 3** (the central one): continental crust that is
  overrun or has nowhere to go welds on; all other crust that is eaten or
  blocked subducts, with only `accretion_fraction` (0.2) of it accreted at
  the arc. A thick mafic plateau at a trench would therefore be 80%
  subducted. Needs: crust whose column floats (`column_buoyancy ≥ 0`, so
  `slab_pull == 0`) welds on instead, as terranes do (Kohistan, Wrangellia,
  the Ontong Java plateau against the Solomon arc).
- `advect.rs:372-373, 402, 445-503` pass 1 (who overrides whom, who is
  eaten): continental over oceanic by label. Reread by buoyancy: the
  buoyant column overrides.
- `slabs.rs:64-66` `initiating_over`, `:86-87` `subduction_with`, `:187`
  founder, `:228`, `:242`: "continent never goes down" and "two continents
  can't subduct" become "a buoyant column does not go down" and "two buoyant
  columns don't subduct". Oceanic pairs already pick the heavier column by
  `column_buoyancy`, so only the mixed and floating cases change.
- `plate_forces.rs:112, 130, 143, 149, 211-212`: keel traction, ridge push and
  slab pull use the type to pick behaviours. Keel traction (continental keels
  are carried by mantle flow) is correct by label. Slab pull already goes
  through `slab_pull(column_buoyancy)`. Check `211-212` (boundary forces by
  type pair) one by one.
- `passive_margins.rs:123, 131`, `recycling.rs:109-114`: margin definitions by
  label. Correct as they stand (a margin is where evolved crust meets
  ocean).

### 4. Reads that are right as they stand

Properties of evolved lithosphere, correctly keyed on the label:
`heat.rs:83` (continental heat flow), `lithosphere.rs:72, 89` (continental
lid vs lid by ocean age), `rifting.rs` (weakness decay, insulation, strength,
craton tests), `craton.rs`, `cohesion.rs`, `sutures.rs`, `plate_lifecycle.rs`,
`arcs.rs:193-195` (arc zone behind a continental margin), `metrics.rs` (all),
`resources.rs:93`, `lib.rs:198` (exposed pointer). Under D these treat a
thick mafic arc as ocean (heat flow by age, lid by age, no keel). That is
defensible: it is not yet evolved crust.

## Design

### Type rule

Define in `cell_data.rs`:

```
fn evolved_share(lanes) = lane(FELSIC) + metasediment(lane(SEDIMENT) - cover lanes)
```

`continental_felsic_share` and `oceanic_felsic_share` (new tunings). Bulk
continental crust is andesitic (~57-61% SiO₂, Rudnick & Gao 2003, Treatise on
Geochemistry 3.01); on a basalt (~50% SiO₂) to granite (~72%) two-member mix
that is roughly 35-50% felsic. Record the derivation in the doc comment, and
fix both values from it before any bench: turn continental at **0.35**, back
at **0.20** (the gap is hysteresis, not a physical claim; the bulk figure only
sets the upper edge). Check the figures against the source when writing the
doc comments (cited from memory here).

Where it applies: ocean → continental and continental → ocean in
`apply_crust_lifecycle`, rift-stretch cells, infill cells. Thickness stops
deciding either flip. Cells that thin to ocean-floor thickness
(`oceanic_thickness`) still revert, since there is no crust left to call
continental.

### Differentiation outside continents

`differentiate` (`crust_lifecycle.rs:172`) runs only on continental cells:
`if crust_type[i] != CONTINENTAL { continue; }`. Replace the type test with
the physics it already models: Moho temperature above `mafic_solidus`
(`temperature_at(heat_flow, thickness)`), for any crust, so thick or hot
mafic crust of either type melts into felsic crust. That is how plateaus and
arcs become continent (TTG from thickened basalt; reworked arcs).

Open points: heat flow for oceanic cells comes from age (`cell_heat_flow`),
which is high for young crust, so a thick young arc has a hot Moho by this
formula (reasonable); a 7 km ridge crust column has a cold one (too thin to
melt). Check that no ocean-floor basalt differentiates at ridges: the unit
test below pins it.

### Subduction and accretion by buoyancy

Add `pub fn floats(t, data, i) -> bool { column_buoyancy(t, data, i) >= 0.0 }`
to `slabs.rs` (so the polarity code and the advection pass share it).

- `slabs.rs`: `subduction_with` returns `None` when both columns float;
  where one floats it overrides, as a continent does now; otherwise the
  existing slab-memory and heavier-column logic stands. `founder` stays
  suppressed for floating columns.
- `advect.rs` pass 1: overriding and eaten decided by `floats` instead of the
  label.
- `advect.rs` pass 3: the continental branch is entered for any floating
  column, with its mass welded on. Mark welded oceanic-type arc crust as
  welded (not "continental") so it keeps its own age and keel logic (age is
  mixed by thickness only for continental crust today).

Risk: column buoyancy depends on the lid (`lithosphere`), and young ocean
lid is thin, so a thick arc floats; old ocean floor with a thick lid sinks.
That is the intended behaviour, but it makes polarity depend on lid state.
Pin it with tests.

### Revert and recycling

- `recycling.rs:131-145` thins margins to `oceanic_thickness` and the
  lifecycle reverts them; the remaining crust's lanes still read felsic. After
  the revert, set lanes to `FRESH_FLOOR` (and records to 0, `arc_age` NEVER)
  for the cells that reverted by thinning, or they would flip straight back.
  Same in `apply_crust_lifecycle` revert path (`crust_lifecycle.rs:266`).
- Erosion thinning (`erosion.rs`) never goes below its floor, so it cannot
  revert cells.

## Steps

1. **Plumbing.** `evolved_share` helper, two tunings in Rust + TS + controls
   + bench overrides. No behaviour change yet (rule still thickness).
2. **Differentiation by Moho temperature.** Switch the gate, bench seeds
   a–d: with type still by thickness, any change in continental composition
   is due to this alone (arc crust and thickened ocean now differentiate).
3. **Type rule by composition.** `apply_crust_lifecycle`, infill, rift-stretch
   cells, revert cleanup. Bench a–d.
4. **Buoyancy-based subduction and accretion.** `floats`, slabs, advection
   passes 1 and 3. Bench a–d.
5. **Reread the thickness floors** (section 2), plume tail and ridge ceiling,
   and update docs.
6. **Verify.** 20 seeds, A + B + D on; report continental area, composition,
   plume-made and arc-made continent, early continental area (300–1000 Myr),
   terrane accretion counts (welded oceanic-type crust per Myr), plate count.

## Tests

- A 25 km mafic arc cell over a trench stays oceanic, and does not subduct
  when it meets thin ocean floor; the thin floor goes down.
- Two floating mafic arcs meeting do not subduct either; they weld.
- A thick hot mafic plateau (plume lane high, Moho above the solidus)
  differentiates until it crosses `continental_felsic_share`, then turns
  continental, and stays until it drops below `oceanic_felsic_share`.
- Ridge-thickness ocean crust (7 km) does not differentiate, whatever its
  heat flow.
- Sediment cover on ocean floor does not make it continental
  (`LANE_COVER_*` excluded).
- A cell at the threshold does not flicker over 50 ticks.
- Recycled margin crust reverts with fresh-floor lanes and records zero.
- Mass: lane masses still add up to live crust (the existing
  `the_lanes_account_for_all_the_crust` test).
- Determinism: same fingerprint at 1 and 4 threads.

## Acceptance

Pass/fail: unit tests above; no regressions in the existing 157; mass drift
zero; identical fingerprint at 1 and 4 threads; no seed ends with no
continent at all or with 50% of continental crust below 0.2 felsic.

Reported, not targeted: continental area, composition (felsic/mafic/
sediment), early continental area, terrane accretion rate, plate count. If
early continent collapses, look at differentiation (Moho temperature,
`differentiation_rate`, melting span), not at the felsic threshold.

## Result of the first build (28 Oct 2026)

Steps 1-4 are built and the 166 tests pass (see ticket 18, Progress, for the
bench numbers). The first build does not give a usable model: either almost no
continent (~3%) when only felsic crust counts as evolved, or ~54% continent
that is 53% sediment when buried sediment counts. Causes found:

1. **Arcs sit exactly on the threshold and cannot cross it.**
   `island_arc_felsic` 0.35 equals `continental_felsic_share` 0.35; an arc
   mixes into the ~7 km of mafic floor under it, so its felsic share is
   `0.35 x added / (7 + added)`, always below 0.35.
2. **Differentiation can't help in arcs.** It reads `heat_flow`, which for
   oceanic cells is cooling-by-age, a few tens of mW/m2 on old floor. A 30 km
   arc over 100 Myr floor has a Moho near 410 C against a solidus of 850 C.
   Real arcs lose far more heat (volcanic and plutonic advection).
3. **Buried sediment is the only thing that crosses.** `lithify` turns deep
   cover into basement metasediment, and `evolved` counts it, so shelves and
   prisms turn continental, erode, make more sediment: a feedback that built
   ~54% continent in the benches.

Options, for the user to choose:

- **A. Principled fix (recommended).** (i) Count buried sediment as evolved
  only where it has been to melting temperature: `LANE_PEAK_TEMP` already
  records the hottest the rock has been, and `granite_min_temp` (650 C) is
  already the S-type granite threshold. Sediment that has not been through
  anatexis is still sediment on ocean crust, not continent. (ii) Give arcs
  their heat: raise the Moho temperature of young arc crust (`arc_age` is
  recorded) so thick arcs differentiate on a realistic timescale, and
  re-derive `island_arc_felsic` and `differentiation_rate` as physical inputs
  (time for andesitic arc crust to reach bulk continental composition is
  ~100-200 Myr). Then re-bench; early continent depends on this.
- **B. Back off D.** Keep type by thickness (revert 18a steps 3-4, keep the
  buoyancy-based subduction, which is correct on its own), carry on with C,
  and accept the thickness-defined type.
- **C. Lower the threshold** to ~0.25 so arcs cross it. Not recommended:
  it moves a number to make an outcome appear, and arcs would then make
  continent in open ocean with no accretion.

## Result of option A (28 Oct 2026)

Built as chosen (see ticket 18, Progress, for the table). Land share became
Earth-scale and tight (25.6-27.8%), but ocean renewal fails: mean ocean floor
~700 Myr and rising, mantle +0.27. Isolated: with buoyant ocean-type crust not
resisting subduction the ocean is healthy but continent is ~3%; with it
resisting, continent grows and the ocean stalls; removing the front lock in
pass 1b did not help. All of D is therefore behind
`SimOptions.composition_crust_type`, off by default (fingerprint of A + B
reproduced exactly). To continue, the next question is why ocean-ocean
subduction does not take over after arcs accrete, not another tuning.
Concrete first checks: count active trenches, slab length and ocean consumed
per Myr with the option on and off; look at which margins have
`subduction_with` returning `None`; try a much higher bar for resisting
(a terrane-sized body, not any 10 km arc).

## Diagnosis of the ocean stall (28 Oct 2026)

Seed a at level 6, plus five-seed and three-seed variants, composition mode
against the default.

Ruled out:
- **Blocked trenches.** Convergent length, working-trench length and slab cells
  are similar with the option on and off; subduction is blocked on <3% of
  convergent length and the area of ocean-type crust that resists subduction
  is ~0% of the globe at any instant. The buoyancy rule is nearly inert as a
  state; it matters as an event (arcs accrete at a trench instead of
  subducting), which is why continent is ~3% with it off.
- **Slow plates.** Mean speed is the same (0.0077 vs 0.0076 rad/Myr).
- **Passive-margin collapse as the cause** (my earlier explanation, wrong).
  With collapse off, passive-margin floor age (the bench's series `age`
  column, which is passive-margin age, not mean ocean age) rises just as fast
  (115, 184, 309 Myr at 300, 900, 1200), the final mean ocean age is worse
  (1131 Myr) and the plates merge into a 4-plate lid. Collapse only decides
  whether the stagnant ocean ends as a lid or as ~100 shattered plates.

**Correction on review (28 Oct 2026): the stall is mostly thick mafic crust
labelled ocean.** Splitting the "ocean" by basement thickness (seed a, 3000 Myr):

| | thin floor (basement <= 10.5 km) | thick ocean-type crust (> 10.5 km) |
|---|---|---|
| default (thickness rule) | 49.1% of globe, mean age 103 Myr | 3.2%, 147 Myr |
| composition, rate 0.002 | 36.4%, 298 Myr | **36.9%, 1153 Myr**, evolved 0.09 |
| composition, rate 0.015 | 37.0%, **109 Myr** | **20.1%, 854 Myr**, evolved 0.12 |

With the faster rate the real ocean floor renews as in the default. The
"ocean age" of 370-700 Myr comes from a fifth to a third of the globe covered
in thick, undifferentiated mafic crust that neither subducts nor turns
continental. Being ocean-type it also gets ocean heat flow by age (old, so
little heat loss, which fits the warm mantle) and ocean rifting strength by
age (old, so it hardly rifts, which fits the low rift count).

That crust is the same thing that, under the thickness rule, is the mafic half
of the continents and the ~50% land share. So the root shared by both modes
is that **thick mafic crust is made faster than anything removes it**. Two
candidate gaps, unmeasured:
1. Nothing founders it in the modern era. B makes the drip hot-mantle only,
   and phase 6 computes an eclogite share below `eclogite_depth` (45 km) for
   density but no process removes eclogitised roots. Whether this crust is
   thick enough to eclogitise has not been measured.
2. The bar for resisting subduction (basement >= 10 km and a floating column)
   is low: thick arcs and plateaus that floats by that test weld on instead of
   going down. On Earth most arcs and plateaus are subducted and only the
   thickest accrete; the threshold should come from the buoyancy literature
   (Cloos 1993 puts it near 17 km for plateaus; contested), not from the
   continental revert thickness.

Next measurements, before any change: the thickness distribution of thick
ocean-type crust (how much is past 45 km), and how much of it was welded by
`resists_subduction` against how much was piled by other routes.

Found:
- Ocean consumed per tick falls to a third of the default (27 vs 84
  milli-globe-km) with the same trench length, because the plates are twice as
  many and reverse direction 2.3x as often: their motions do not add up to
  net convergence.
- The runs are identical to the default until ~600 Myr. Floor age then rises
  in every composition run, before any collapse has happened.
- **Early continent is what separates them.** Composition mode makes 0.1% at
  300 Myr and 2.4% at 600 (default: 3.4% and 5%), 5.6% at 900 (default 21%).
  `differentiation_rate` 0.002 (215 Myr to evolve a plateau to the continental
  threshold) was set in phase 2 when nothing depended on it. At 0.015 (the
  ~28 Myr conduction time of a 30 km crust, chosen before the test) early
  continent appears (2.9% at 300 Myr), land share goes 26.5% to 42.5%, ocean
  age ~700 to ~400 Myr, but the stall remains, and continental crust becomes
  6% mafic (over-differentiated).
- **Likely structure (not proven; weakened by the correction below, since
  the thin ocean renews normally once differentiation is faster):** ocean renewal in this model depends on
  continental rifting. Old ocean lithosphere is too strong to rift (see
  `rifting.rs`), so new ridges mostly appear where continents break up
  (default: 372 rifts by 3000 Myr; composition mode: 121-147). A world whose
  continent comes late and small has few ridges to renew its floor. Anything
  that delays early continent, as composition mode does, therefore ages the
  ocean.

Not established: why the age rise starts at ~600-900 Myr in composition mode
when the default holds ~100-150 Myr until 1500; the rift count is the best lead.

What it means for D: it cannot be judged on land share alone. It needs (1) a
differentiation calibration that is physical and does not over-differentiate
(the lower crust of real continents is ~50% mafic; delamination of dense
cumulates, not modelled, is what keeps it so), and (2) an answer to how the
ocean is renewed when continents are few. Both are bigger than this ticket.

## Open questions for the user (answered 28 Oct 2026)

All four were answered "agree": thresholds 0.35/0.20 stand (see the result
above for their consequence); neighbour rule dropped; oceanic-type thick crust
keeps ocean heat flow and lid for now (this is part of cause 2); stop after
step 3 if step 4 grew (step 4 did grow and was built, see above).

Original questions:

1. **`continental_felsic_share` 0.35 / `oceanic_felsic_share` 0.20.** Derived
   from bulk andesitic composition as above. Say if you would rather derive
   them from density instead (e.g. the share at which a 30 km column of the
   mixture floats).
2. **The neighbour rule.** Ocean turns continental today only beside
   continent, over a plume, or once piled past 20 km. With a composition
   rule the neighbour condition is redundant for crust that is felsic, and
   harmful for crust that isn't. Plan: drop it, so continental margins
   grow only by evolved crust accreting (arcs differentiating, welded
   terranes), not by adjacency. Tell me if you want it kept.
3. **Heat flow and lid for oceanic-type thick crust.** Left as ocean (by
   age). A 30 km thick mafic plateau then has a thin oceanic lid; check how
   that looks in elevation and buoyancy once step 4 is in.
4. **Scale.** The audit lists ~15 sites to change, most in `advect.rs` and
   `slabs.rs`. If step 4 turns out to touch the advection passes more than
   expected, stop after step 3 and discuss before going on.

## Dependencies

- Ticket 18 steps A and B (done / in progress); C comes after this.
- Rock-cycle phase 6 (`composition_density`, lanes) in the working tree.
