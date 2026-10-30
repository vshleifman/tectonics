# 18 — Continental growth balance and the stagnant-lid split

## Goal

Make continental area the outcome of a physical balance between crust that
arcs and plumes add and crust that subduction takes back, with growth timed
by the mantle cooling past the point where modern subduction works. Remove
the chance split between a "warm, few plates, ~12% continent" world and a
"cool, many plates, ~30% continent" world.

Three changes, verified together because they interact:

- **A.** Stagnant plates shed their old passive margins at a realistic age.
- **B.** The hot-mantle brakes on arcs and drip fade out where modern-style
  subduction starts, not at today's mantle temperature.
- **C.** Recycling comes from processes at trenches (sediment subduction,
  subduction erosion of arcs), not from a fixed share of continental area.

Before C, crust type is redefined by composition (see "D. Crust type from
composition"), since that changes the growth flux C is meant to balance.

## Progress

Detailed plan for D: `18a-crustTypeFromComposition.md` (includes the
`crust_type` audit).

- **Step 1, A alone: done (27 Oct 2026).** `STAGNANT_AGE` 1.0,
  `passive_margin_collapse_age` 200. 20 seeds (a–t), level 6, 3000 Myr:
  0 of 20 end warm (mantle excess at 2800 Myr −0.04 to −0.08); fewest plates
  after 1000 Myr 32; oldest mean ocean age after 1000 Myr 273 Myr; none
  breaches the lid criteria. Continental area 29.7–39.3%, mean 36.1%, spread
  9.6 points (all in the "big" regime). The earlier 150 Myr x 2 test gave
  25–31%, so land share is higher than that test suggested; reported, not
  compensated. Continental crust ends ~25% felsic / 48% mafic / 27% sediment.
- **Step 2, B (value fixed at 0.68 / 0.30, see above): done in code, benched
  on seeds a–j with A on.** 157 Rust tests pass, typecheck and biome clean,
  identical fingerprint at 1 and 4 threads (`dc26ddffddcf4204`, dynamics
  `74f5faf94ff60ce1`, composition `6c3026e5ef779e9c`). Results, A only →
  A + B (same seeds a–j):
  - continental area at 3000 Myr 36% → **50%** (47.7–52.4%, spread 4.7);
  - growth is earlier: at 900 Myr ~7% → 12–21%, at 1500 Myr ~10% → 33–38%,
    at 2100 Myr ~13% → 42–47%; it flattens in the last 900 Myr, which is
    the missing-losses plateau, not a balance;
  - mantle excess at 2800 Myr −0.06 → **+0.16 to +0.20**: with half the globe
    continental the mantle sheds less heat and stays warmer;
  - plates 79 → 27–70 (mean ~42), mean ocean age 76 → 76–150 Myr; fewest
    plates after 1000 Myr 21–39 (seed e is closest to the 20-plate line);
  - continental crust ~19% felsic / 42% mafic / 39% sediment.
  This is what the ticket predicted: B moves growth earlier, and without C
  nothing takes it back. Land share has moved a lot (36% → 50%); per the
  standing rule it is reported here and not compensated for. C and D are the
  designed answers (process-based losses; crust type from composition), not
  adjustments to B.
- **Step 3, D (steps 1–4 of 18a built, 28 Oct 2026): built, benched,
  result unsatisfactory; paused for a decision.** Plumbing, differentiation
  gated on Moho temperature in any thickened crust, type by composition,
  buoyancy-based subduction (`resists_subduction`, basement thickness only),
  plume crust exempt from drip, and the user-approved answers to 18a's open
  questions (thresholds 0.35/0.20; neighbour rule dropped; oceanic-type thick
  crust keeps ocean heat flow and lid; stop after step 3 if step 4 grew).
  166 Rust tests pass; typecheck and biome clean. Bench (a–j, A + B + D):
  - continental area **52.6–57.2%** (mean 54.8%); early continent nearly
    gone (0.2% at 300 Myr, 2.7% at 600, ~10% at 900; A + B had ~4% at 300);
  - continental crust ~20% felsic / 27% mafic / **53% sediment**;
  - 66–90 plates, strict landmass connectivity poor mid-run (0.11 at 900 Myr),
    mean ocean floor **220–260 Myr** (peaks 440–470), mantle excess +0.24;
  - with buried sediment *not* counted as evolved (5 seeds): continental area
    **~3%**, mean ocean age ~1000 Myr, mantle +0.30, plates 72–88.
  So under a pure composition rule the model makes almost no continent from
  differentiation, and what continent it makes is built by buried sediment.
  Why (arithmetic, not yet measured): `island_arc_felsic` is 0.35, exactly
  `continental_felsic_share`, and an arc cell mixes its crust into the ~7 km
  of mafic ocean crust beneath it, so its felsic share stays below 0.35 at any
  thickness (e.g. 0.30 at 50 km); differentiation can't close the gap
  because `differentiate` uses ocean heat flow by age, so a 30 km arc over
  old floor has a ~410 C Moho, far below the 850 C solidus. The basement
  fix (sediment cover no longer makes crust resist subduction) changed
  nothing measurable (52.7% vs ~54%); kept as the correct physics.
  Options and recommendation in 18a, "Result of the first build".
- **Option A tried (28 Oct 2026, user choice): melted-sediment-only
  `evolved` plus arc heat in `differentiate`. Result: no usable regime.**
  `evolved` now counts buried sediment only where `LANE_PEAK_TEMP` reached
  `granite_min_temp` (650 C); `differentiate` raises the Moho temperature of
  young arc crust toward `arc_heat_flow` (80 mW/m2: 80-90 across the
  Sumatra-Andaman arc, ~80 in the Japanese volcanic zone, forearcs 40-70;
  regional sources only) fading over `arc_heat_duration` (50 Myr). The sources
  gave no basis to change `island_arc_felsic` (arc melts ~50 wt% SiO2 against
  ~60 for continental crust, continent forming only after differentiation and
  delamination of dense cumulates), so it stays 0.35. 168 tests pass.
  Bench, level 6, 3000 Myr:

  | configuration | continent | ocean floor age | mantle excess |
  |---|---|---|---|
  | A + B, type by thickness (a-j) | 48-52% | 76-150 Myr | +0.18 |
  | A + B + D, buried sediment counts (a-j) | 53-57% (53% sediment) | 220-260 | +0.24 |
  | D + melted-sediment + arc heat (a-j) | **25.6-27.8%** | **~700**, still rising | +0.27 |
  | same, buoyant-crust rule off (a-e) | 1.3-4.8% | 33 | -0.30 |
  | same, buoyant crust accretes but no front locking (a-e) | 46-49% (77% sediment) | 260-520 | +0.24 |

  The melted-sediment rule and arc heat fix what they were meant to: land
  share is Earth-scale and tight across seeds (spread 2.2 points), and
  composition is 38% felsic. But buoyancy-based accretion is what lets
  continent grow at all (off: ~3%), and the same mechanism leaves the ocean
  unable to renew itself (mantle flow 0.1 cm/yr against 0.7, ocean floor age
  rising linearly with time). Removing the front lock did not restore it.
  Not yet understood: why ocean-ocean subduction does not take over once arcs
  accrete. Candidates, untested: subduction initiation needs a plate
  boundary with a working margin and none forms in an ocean of small plates;
  trenches are lined with accreted arcs that `subduction_with` treats as
  floating on both sides; the very low bar for resisting (basement 10 km and
  a floating column) makes every arc resist.
- **Where the tree is now.** All of D is behind
  `SimOptions.composition_crust_type` (UI toggle "Crust type from composition
  (experimental)", bench `composition_crust_type=1`), default **off**. Off,
  the dynamics are bit-for-bit those of A + B (fingerprint
  `dc26ddffddcf4204`, dynamics `74f5faf94ff60ce1`, composition
  `6c3026e5ef779e9c`). On, they are the last row group of the table.
- **Diagnosis of the ocean stall (28 Oct 2026, user choice 1):** see 18a.
  Short version: trenches are not blocked; passive-margin collapse is a
  symptom, not the cause; the stall follows from composition mode making
  little early continent (`differentiation_rate` 0.002 was never calibrated)
  combined with a model whose ocean renewal leans on continental rifting.
  At 0.015 the stall is smaller (ocean age 700 to ~400 Myr, land share
  26.5% to 42.5%) but remains. D stays behind its option, off.
  **Corrected on review:** the stall is mostly a fifth to a third of the
  globe covered in thick undifferentiated mafic crust labelled ocean (mean
  age 850-1150 Myr); with the faster rate the thin ocean floor renews like
  the default's (109 vs 103 Myr). Under the thickness rule the same crust is
  the mafic half of the continents. Shared root: thick mafic crust is made
  faster than anything removes it, which is what C (and a modern-era sink
  for eclogitised roots) has to address. Details in 18a.
- **Housekeeping on review:** three tests relaxed for composition mode
  (`early_continent_comes_from_plumes`, `a_hot_mantle_raises_round_keeled_continent_and_keeps_the_ledger`,
  `a_running_world_keeps_its_ledger_with_cohesion_on`) were restored to
  their original form, since they run with the option off; 168 pass. WASM
  packages (`pkg`, `pkg-threads`) rebuilt so the app runs the current Rust.
- **Step 4, C0: done (27 Oct 2026). C1 is worth building as designed.**
  New bench lines: `trench fluxes` (Earth-scaled km³/yr per 500 Myr; model
  globe-share x km per Myr times 5.1e8 km² over 1e6 yr; crust km taken as
  real km, no correction for trenches resolved as ~100 km cells, so read
  these as order-of-magnitude) and `thick oceanic/continental crust`. Tallies
  added to `AdvectionField` (`sediment_deposited_trench_edge`,
  `sediment_deposited_near_trench`, `arc_accreted_this_step`,
  `welded_oceanic_this_step`, `welded_continental_this_step`); dynamics
  unchanged (default fingerprint still `dc26ddffddcf4204`). A + B, seeds
  default + a–d, 2500–3000 Myr:
  - erosion 6.7–10.4 km³/yr (Earth's river load is ~7–8 km³/yr of rock);
  - deposited within one cell of a trench edge 1.7–2.4 km³/yr (23–31% of
    erosion), on the edge itself 0.7–1.0. Earth's sediment subduction is
    ~1.65 km³/yr (Clift et al.; von Huene & Scholl 1991 give 1.5, ~1.0 long
    term), so the 10% bar is passed by an order of magnitude;
  - arc accretion 4.0–5.0 km³/yr (Earth ~2.5–3); margin erosion of
    continental margins 0.9–1.2 (Earth subduction erosion ~1.3–1.4);
    ocean crust subducted 17–22;
  - welded continental crust 5–11 km³/yr (transfer between plates, not a
    gain); welded oceanic 0, as expected with type by thickness.
  - Thick crust: in the default mode only ~3% of the globe is oceanic-type
    crust over 10.5 km (mean 12.6 km, none over 20 km). Continental crust
    averages 26–27 km basement; 73–82% of it is over 20 km, 27–37% over 30,
    and **0–4% over 45 km**. So an eclogitised-root sink would act on very
    little crust in the default mode: it is not the main missing loss there.
- **Step 5, C1 (built):** `trench_sediment_subduction` 0.7 (von Huene &
  Scholl 1991: ~70% of the incoming section subducts beneath large
  accretionary prisms, nearly all at non-accretionary margins; most trench
  sediment is at the former). At a trench edge cell (slab beneath, other
  plate beside) that share of the arriving load goes into
  `sediment_subducted` and `subducted_total`; the rest is routed as before.
  Rust tuning, bench override, TS tuning, slider and tooltip, metric
  `sedimentSubducted` and diagnostics series. Tests: ledger now eroded ==
  deposited + subducted (erosion and world tests); a range beside a trench
  sends between the share and all of what reaches the edge down, and nothing
  stays on the edge at share 1; the same range with no trench subducts none.
- **Step 6, C2 (built):** `scraped_by_trench` / `margin_take` in
  `advect.rs`: ocean going down under oceanic crust thicker than ridge crust
  scrapes it at `margin_erosion` per unit ocean consumed, as for a continent,
  down to ridge crust and no further; tallied as `arc_eroded_this_step`
  (continental stays in `eroded_this_step`). Tests: the rule (same take for
  both, floor at ridge crust, plain floor untouched) and a running world that
  scrapes arcs with the mass ledger closed. A staged two-plate trench test was
  not written: the model has no way to hold plate motion fixed in a test.
  172 tests pass.
- **C1 + C2 bench (seeds a–d, 3000 Myr):** continental area 47.7–50.5% →
  42.3–46.6% (C2 alone 45.3–49.4%); mantle excess +0.18 → +0.12–0.14.
  2500–3000 Myr fluxes: sediment subducted at the trench edge 0.3–0.5
  km³/yr, plus 0.5–0.8 carried down on ocean floor (new tally
  `sediment_carried_down_this_step`), so ~0.9–1.4 in all against Earth's
  1.0–1.65; arc scraping 0.7–1.3 plus continental margins 1.0–1.2, so
  ~2 km³/yr against Earth's ~1.3–1.4 (1.5× high, inside the 3× window).
  The fixed recycling rate took only 0.26–0.39 km³/yr (it recycles the
  thinnest margin cells first), well below C1 + C2's ~1.4–1.8, so C3's
  condition holds.
- **Step 7, C3: done.** `continental_recycling_rate` default 0.0004 → 0 in
  Rust and TS, docs updated (`recycling.rs`, tuning, tooltip); code path and
  `hold_continental_area` unchanged. 172 tests pass.
- **Step 8, diagnostics: partly done.** Bench `trench fluxes` line
  (Earth-scaled, per 500 Myr) and `thick … crust` lines; TS
  `sedimentSubducted` metric and chart series. Not done: per-process budget
  per 300 Myr in TS, growth-timing milestones, piling by origin.
- **Step 9, combined verification (A + B + C, 20 seeds a–t, level 6,
  3000 Myr):**
  - continental area **41.1–50.7%, mean 45.2%**, spread 9.6 (A + B: ~50%);
  - mantle excess at 2800 Myr +0.14 to +0.18; 0 warm-regime seeds;
  - fewest plates after 1000 Myr 19 (seed b, one sample under the 20-plate
    line; all others 28–44); final mean ocean age 62–133 Myr;
  - continental crust at 2700 Myr ~21% felsic / 47% mafic / 32% sediment;
  - default seed: fingerprint `64b0764efd4549c1` (dynamics
    `fa3b0461e837de4d`, composition `98c0bfc7297a9a55`), identical at 1 and
    4 threads; continent 45.2%, ocean age 118 Myr.
  Land share moved ~5 points down from A + B. Not compensated. The arc
  addition is still 4–6 km³/yr against Earth's 2.5–3, which is the
  largest remaining gap in the budget.
- **Arc addition, follow-up (28 Oct 2026): measured, nothing changed.** New
  bench columns: foundered at the cap, delaminated, dripped, trench length,
  arc accretion per km; and a `boundaries, km` line at the end. Seeds a–d,
  2500–3000 Myr:
  - foundered/delaminated/dripped are ~0 (0.02–0.13 km³/yr), so the model's
    arc addition is effectively net crust kept;
  - convergent boundary length 105–124k km (seeds a, b), ~45% of all
    boundaries (238–275k km); Earth's convergent margins are ~43,500 km (von
    Huene & Scholl 1991), so ~2.4–2.8x, or ~2x if cell staircasing inflates
    the count by ~1.3. About a third of cells carrying a slab no longer sit
    on a convergent boundary (stale slab);
  - arc addition per km of convergent trench ~40–50 km³/km/Myr, inside
    Dimalanta et al. 2002's 30–95 for western Pacific arcs (net crust;
    Jicha & Jagoutz 2015's 157–290 is gross productivity, cumulates included);
  - ocean consumed globally 17–25 km³/yr, near Earth's ~20–24.
  So arc addition is ~1.5–2x Earth for one of two reasons, depending on what
  arc output scales with: per unit ocean consumed, `accretion_fraction` 0.2 is
  ~2x Earth's ~0.1 (2.5–3 / ~24; the TS doc already says "Earth's arcs keep
  roughly a tenth", and 0.2 was chosen for how fast the area target was
  reached, an outcome); per km of arc, productivity is Earth-like and the
  model has ~2x Earth's convergent boundary length at half the convergence
  rate per km. Decision (user, 28 Oct 2026): set the fraction from sources
  now, look into boundary length separately.
- **`accretion_fraction` 0.2 → 0.11 (28 Oct 2026).** Doc comment in Rust and
  TS now gives the derivation (2.5–3 km³/yr arc addition over ~24 km³/yr of
  ocean crust consumed: 3.4 km²/yr of sea floor, Rowley 2002, at ~7 km).
  172 tests pass. 20 seeds (a–t), level 6, 3000 Myr:
  - continental area **24.3–33.4%, mean 28.2%** (was 41.1–50.7%, mean
    45.2%): a large move, reported, not compensated;
  - mantle excess at 2800 Myr +0.03 to +0.11 (was +0.14 to +0.18); final
    plates 38–88 (fewest after 1000 Myr 31); mean ocean age 52–126 Myr;
  - continental crust ~17% felsic / 55% mafic / 28% sediment;
  - **arc addition hardly fell: 3.3–4.9 km³/yr (was ~4–6)**, because ocean
    consumed rose from 20–25 to 34–40 km³/yr. Less continent means more
    ocean, more plates and more trench.
  Boundaries along mesh arcs, seed a at 3000 Myr (76 plates): convergent
  184k km (120k closing over 2 cm/yr), divergent 100k, transform 274k; sea
  floor consumed 5.9 km²/yr and made 5.3 km²/yr, against Earth's ~3.4. So
  the model renews its ocean ~1.7x as fast as Earth, along ~3–4x Earth's
  trench length at under half Earth's mean closing rate (3.2 against ~7.7
  cm/yr). Arc lengths along a cell staircase overstate a smooth boundary by
  up to ~1.3x. That ocean-turnover excess is the next thing to look at:
  it is what keeps arc addition high, and it tracks plate count.
- **Plate count and boundaries (28 Oct 2026, user: go ahead), measured:**
  - Plates are too fragmented: plates over 5% of the globe 2–5 (Earth 7),
    over 0.5% 15–45 (Earth ~13), largest seven 0.33–0.9 of the globe
    (Earth ~0.9), the low end common.
  - Plate ledger, seed a over 3000 Myr: births ~300 rifts + ~210 pinched-off
    fragments (+ passive-margin collapses); deaths ~420 consumed, ~190
    craton merges, ~30 welds, only 9–15 quiet merges.
  - Boundary length by plate-pair mean relative speed (seeds a, c, e, g at
    3000 Myr, new bench line): under 1 cm/yr 9–17%, 1–2 cm/yr 10–20%, the
    rest faster. So most boundary separates plates that really move apart
    or together; co-moving plates that never merge are a minority.
  - **Continental breakups do not open.** Already in the bench output, and
    the same before this ticket (C0 run, A + B): 10 Myr after a breakup the
    new boundary is diverging nowhere in 83–100% of breakups; mean motion
    across it is −0.3 to +0.04 cm/yr opening against 0.2–0.9 sliding, net
    closing along 70–80% of it. Earth's young rifts open at ~1–5 cm/yr.
    A rift that breaks a plate but never spreads leaves a boundary that
    adds plates, transform and, later, trench without adding ocean. Best
    lead so far for the excess boundary; not yet diagnosed (force balance
    after the split).
- **Breakup diagnosis (28 Oct 2026, user: yes).**
  - `weld_on_net_closing` (existing option, off since the supercontinent
    experiments, no recorded reason), seeds a, c, e, g: welded back within
    10 Myr falls from 75–100% to 39–75%, but the halves still close on
    average. A secondary artefact, not the cause.
  - New diagnostic: `PlateData::force_parts` (not saved) keeps, per plate,
    the motion each driving force alone would give against the plate's full
    drag; the bench reports the opening across each breakup under each.
    Dynamics unchanged (seed a fingerprint `3caebd9a8afd438d` before and
    after). Medians over seeds a, c, e, g, opening (+) or closing (−) in
    cm/yr at the split:

    | | moving | all forces | mantle carry | ridge push | slab pull | dome push |
    |---|---|---|---|---|---|---|
    | hot breakups | ~0 | −0.6 to +1.7 (+ in 3 of 4 seeds) | −0.2 to +1.8 | −0.4 to −1.1, closes in ~100% | +0.0 to +0.3 | +0.3 to +0.5 |
    | cold breakups | ~0 | −0.5 to −0.7 | −0.4 to −1.2 | −0.4 to −0.8 | +0.1 to +0.3 | +0.2 to +0.3 |

  - Reading: (1) **cold breakups have nothing that would open them.** The
    rift is triggered by opposed slab pull (`plates.stress`), but slab pull
    alone opens the halves by only 0.1–0.3 cm/yr against continental drag,
    while ridge push (a plate's ridges lie on its outer margins, so it
    presses the plate together) and mantle carry close them by more. The
    rift criterion counts tension from slabs and ignores the compression in
    the same balance. (2) **Hot breakups that would open don't move:** at
    +10 Myr all forces still give +0.35 to +1.5 cm/yr while the plates
    move at ~0. `motion_response_time` is 40 Myr (only ~22% of the way to
    the target after 10 Myr, longer for small plates via
    `microplate_damping` 5), and the halves weld within 10 Myr, after which
    `craton_durability` 0.75 per tick holds the pair to a common motion and
    dome push stops.
  - **Built (28 Oct 2026, user: 1 and 2, plus over-eager merging):**
    1. `SimOptions.rift_needs_separation` (default on, TS toggle): a rift is
       picked, and keeps stretching, only where its two sides would part
       under their own forces. `plate_forces.rs` now builds every force from
       shared `cell_force` / `arc_force` (refactor checked: seed a fingerprint
       unchanged, `3caebd9a8afd438d`); `rifting::separation` sums them per
       side, adds dome push across the line, solves each side and takes the
       mean opening along the line. Test `heat_alone_rifts_…` now checks a
       hot continent with no dome push does not break at all.
    2. `weld_on_net_closing` default on (TS toggle), and no weld between
       plates that broke apart less than `dome_push_duration` ago.
    Seeds a–h: breakups with no diverging boundary after 10 Myr 0–50% (was
    83–100%); rifts ~205–225 (was ~280–310); plates over 0.5% 21–34 (was
    33–38), largest seven 0.52–0.78 (was 0.50–0.58); continental area
    29.5–36.5%. 172 tests pass.
  - **Merging.** New series column `top1` (largest plate's share). With 1 + 2
    the largest plate averages 17–30% but reaches 61–73% in 3 of 5 seeds:
    `craton_merge_time` (60 Myr) merges plates whose cratons touch whatever
    their motion, and `craton_durability` (0.75) doubled as the coupling
    that holds welded craton pairs to a common motion (now its own tuning,
    `craton_weld_coupling`, same default). Variants, seeds a–e:

    | | largest plate mean / max | plates over 0.5% | largest seven |
    |---|---|---|---|
    | 1 + 2 | 0.17–0.30 / 0.30–0.73 | 24–34 | 0.52–0.72 |
    | + no craton merge | 0.10–0.22 / 0.16–0.51 | 29–42 | 0.42–0.61 |
    | + quiet by mean speed | 0.10–0.19 / 0.15–0.42 | 32–40 | 0.42–0.53 |
    | + no craton coupling | 0.10–0.24 / 0.13–0.47 | 33–44 | 0.37–0.55 |

    Earth: Pacific ~0.20, ~13 plates over 0.5%, largest seven ~0.9. Taking
    the craton shortcut away ends the giant plates but exposes too many
    medium plates: the merges were hiding over-fragmentation. Defaults left
    as they are until fragmentation is dealt with.
  - **Fragments.** New bench lines. Seeds a–c: 195–225 fragments per run, of
    which 65–75% follow neither a rift nor a collapse; ~90% ocean; median
    0.37% of the globe (p90 ~0.95%); 4 neighbouring plates; their boundary
    ~28% convergent and ~50% transform; no neighbour lost much area that
    tick (0.1x the fragment). On top, ~4,600 pieces below the threshold are
    pinched off and absorbed per run (~1.5 per tick). So plates are cut
    into pieces as they move, mostly along transform boundaries, not by
    trenches eating through them: most likely a transport artefact of moving
    plates a cell at a time. Not yet located in `advect.rs`.
  - Candidate fixes (not built): rift only where the two candidate halves
    would separate under the full force balance (their solved targets open
    across the line), not on slab tension alone; a fresh breakup is not a
    suture (weld only on net closing, and not while the pair is still in
    `breakups`); then look at whether the 40 Myr response time has a
    physical basis or is smoothing for force jitter.
- **Step 10, write-up:** this section. `scripts/compare.ts` bands not
  changed; ticket 16's recycling note not yet updated.

## Findings that led here (27 Oct 2026)

All runs: level 6, 1500 ticks (3000 Myr), phase 6 code (dynamics fingerprint
`cfd0040456298971` with default seed).

### The regime split is chance

- Two algebraically identical drip formulas gave 14.2% and 40.2% continental
  area: float rounding alone flips the outcome.
- Five seeds each of the current and old drip rule: every run ends either at
  11–15% or 28–40% continental area. Which one tracks mantle excess at
  2800 Myr in all 13 runs (≤ 0 big, ≥ +0.12 small). The drip rule does not
  measurably shift the odds (current rule 2/5 big, old rule 3–4/5).

### Why runs go warm

1. Early on, subduction happens to bunch onto one hemisphere ("sink dipole"
   ≥ 0.2 in warm runs, ≤ 0.1 in cool ones by ~1000 Myr).
2. Slab pull then pulls plates the same way instead of apart. Rifting stops
   (warm seed c: 10, 11, 6, 0 rifts per 300 Myr from 600 to 1500 Myr; cool
   seed d: ~30 throughout).
3. Plates are consumed down to a near-stagnant lid (2 plates at 1600 Myr in
   seed c), with 95% of ocean floor old and 99% of margins passive.
4. Old floor barely loses heat (surface ~42–55 TW against 70–90 TW in cool
   runs), so the mantle stays hot, and the hot-mantle brakes keep continent
   small.
5. The only exit is passive-margin collapse, which for a stagnant plate waits
   until its mean ocean age reaches `STAGNANT_AGE` × `passive_margin_collapse_age`
   = 2 × 300 = 600 Myr. Earth has almost no ocean floor older than ~200 Myr.

Halving the collapse age to 150 Myr (multiplier left at 2) removed the warm
regime on all four seeds tried: 25.3–31.4% continental area, mantle excess ~0
at 2800 Myr, 73–94 plates, mean ocean age 69–82 Myr. Four seeds is a small
sample; see the seed count under Verification.

### Why continents grow late

With the collapse fix in place, continental area still grows mostly in the
last ~600 Myr (seed d: 13.6% at 2400 Myr, 31.4% at 3000 Myr).

- Growth is "piling": ocean cells crossing `continental_thickness_threshold`
  with no inflow that step. Cumulative piled area reaches 0.60–0.75 of the
  globe (gross), against 0.04 from arcs directly and 0.17–0.21 from plumes
  (which stop at ~2100 Myr).
- Piled cells are 56–83% recent island-arc crust (arc magmatism within
  100 Myr), felsic share only 0.09–0.14, 15–30 km thick. Another 10–40% is
  ocean crust thickened inside plates (welding in advection pass 3, ~5% of
  ridge production); sediment is 0–9%.
- While the mantle is hot, `hot_drip` founders that thickened crust and
  `hot_arc_suppression` scales arcs down. Both are linear in mantle excess and
  reach zero at today's temperature, ~2500–2700 Myr into a run.
- Causal test: holding the drip on (floor 0.15) left seeds c and d identical
  until ~2400 Myr, then stopped the surge: 14.5% and 15.3% at 3000 Myr instead
  of 27.8% and 31.4%.
- Nothing scales the losses with the growth. `continental_recycling_rate` is a
  fixed 0.04%/Myr of continental area (late in a run it removes ~0.012% of the
  globe per Myr, about a third of net growth). Subduction erosion
  (`margin_erosion`, `subduction_erosion`) only acts on continental overriding
  cells, so intra-oceanic arcs grow untouched until they flip. Sediment never
  reaches a trench, because erosion routes it on its own plate only.

Continental crust ends at roughly 30% felsic, 50% mafic, 20% sediment.

### What the Earth comparison can and cannot say

The sim starts at `start_age_ga` 3.0. "Earth built most of its continental
crust by ~2.5 Ga" mostly describes growth before the sim begins; how much
growth Earth had between 3.0 Ga and today is disputed (compare Dhuime et al.
2012 with Armstrong-style early-growth models). So the Earth record can tell
us the brakes are timed wrongly (modern-style subduction by ~3.0–2.5 Ga, when
the mantle was well above today's temperature), but it can't supply a target
for when the sim's continents should grow. Growth timing is reported, not
tested against (see Acceptance).

## Design

### A. Passive-margin collapse at a realistic age

`passive_margins.rs`:

- `STAGNANT_AGE` 2.0 → 1.0. A stagnant plate sheds its margin once its mean
  ocean age reaches the collapse age itself.
- `passive_margin_collapse_age` 300 → 200 Myr: past this age ocean
  lithosphere is dense enough that old passive margins and transforms beside
  it fail on Earth (oldest in-situ floor ~180–200 Myr in the Pacific; the
  Eastern Mediterranean at ~280 Myr is the exception). Justify in the doc
  comment. This is a physical input, not a fit to the land-share outcome.
- Keep `collapse_stagnant_only`: a moving plate with trenches still keeps an
  old passive coast, as the 180 Myr Atlantic margins do.
- Update the module docs and the tests in `passive_margins.rs` that pin the
  multiplier.

**Untested combination.** The test that removed the warm regime used a
150 Myr coast age and a 300 Myr stagnant gate (150 × 2). This design uses
200 Myr for both: stricter on the stagnant gate, looser on the coast gate.
A is therefore run on its own first (step 1) to confirm it still removes the
warm regime before B or C are built.

### B. Hot-mantle brakes fade at the modern-subduction transition

Today (`heat.rs::effective_tuning`, `primordial.rs`):

```
accretion_fraction *= clamp01(1 - hot_arc_suppression * excess)
drip = clamp01(hot_drip * excess)
```

Both reach zero only at excess 0 (today's mantle).

New: one shared "hot-slab" factor in `heat.rs`:

```
hot = smoothstep((excess - modern_subduction_excess) / modern_subduction_width + 0.5)
accretion_fraction *= 1 - hot_arc_suppression * hot
drip = hot_drip * hot
```

- `modern_subduction_excess` (new tuning) is the excess at which slabs become
  strong enough for sustained modern-style subduction.
- `modern_subduction_width` (new tuning) spreads the transition so it is
  gradual, not a switch. Default 0.3 (~90 K at `EXCESS_SCALE_K` 300).
- Keep `hot_arc_suppression` and `hot_drip` as the strengths (1.0).
- Plume behaviour, keel growth and the rest of the primordial era keep their
  own dependence on excess; only these two brakes move.
- Fix the doc mismatch while there: `heat.rs` module docs say an excess of 1
  is "about 250 K", but `EXCESS_SCALE_K` is 300.

**Choosing the value without circularity.** This dial sets when arc growth
starts, so it must not be adjusted against growth-timing results.

1. Before writing any B code, check the sources and fix the value from the
   slab physics alone: the mantle temperature excess above which slabs break
   off too often, or are too weak and buoyant, for sustained subduction. The
   candidates I quoted from memory (van Hunen & van den Berg 2008, Lithos
   103; Sizova et al. 2010, Lithos 116; Cawood et al. 2018, Phil. Trans. R.
   Soc. A 376) put it at roughly 100–200 K above today's, which would give
   0.33–0.67. Read them and record the figure, page and argument in the
   tuning's doc comment. If they don't support a figure, say so in the doc
   and use the midpoint of what they do support.
2. **Value fixed 27 Oct 2026, before any B code or bench:**
   `modern_subduction_excess` = **0.68** (~205 K at `EXCESS_SCALE_K` 300) and
   `modern_subduction_width` = **0.30** (~90 K), so the brakes are full above
   excess 0.83 (250 K) and gone below 0.53 (160 K). Basis (abstracts only, full
   texts not read): Sizova, Gerya, Brown & Perchuk 2010, Lithos 116, 209–229,
   report a gradual transition from a pre-subduction regime at 250–200 K above
   present upper-mantle temperature, and an abrupt one to modern-style
   subduction at 175–160 K; van Hunen & van den Berg 2008, Lithos 103,
   217–235, find slab weakness (frequent breakoff in a hot, low-viscosity
   mantle), not buoyancy, is the main limit. This replaces the 0.5 I first
   proposed from memory; it is the centre of the range the abstracts give.
   In current runs it lifts the brakes around 900–1000 Myr (excess 0.5–0.45),
   well before today's 2500–2700 Myr. Models disagree on these figures
   (conference abstracts give 160–200 K and 175–250 K for the band), so the
   dial stays exposed, but it is not to be tuned against results.
3. After that it stays put. If the combined runs look wrong, the next step
   is to find which process is wrong, not to move this value.

Expected effect: arc growth starts while the mantle is still well above
today's temperature, so continental growth moves earlier. Without C it would
then run away for longer, so B must not land without C.

### D. Crust type from composition

Today a cell turns continental once it is thicker than
`continental_thickness_threshold` (15 km), whatever it is made of. Most of the
late growth is island-arc crust at felsic share ~0.1 that qualifies on
thickness alone, and continents end up ~50% mafic.

**Decision (user, 27 Oct 2026): composition is the root, not thickness or
buoyancy.** Oceanic crust is mafic (basalt and gabbro) from mantle melting at
ridges; continental crust is felsic to andesitic, built by subduction-zone
magmatism, partial melting and reworking. Density follows from composition
(oceanic ~2.9–3.0, continental ~2.7 g/cm³, mantle ~3.3). Thickness partly
follows too: oceanic crust is a uniform 5–10 km because one melting process
makes it; continental crust is 30–50 km (70+ under the Himalaya) because it is
buoyant, resists subduction, and accumulates and thickens in collisions.

So in the sim:

- **Crust type is a function of basement composition.** A cell is
  continental when its evolved share, felsic plus metasediment in the
  basement (cover excluded: loose sediment lying on ocean floor doesn't change
  what the crust beneath it is), is above `continental_felsic_share`. Use
  hysteresis (`continental_felsic_share` to turn continental, a lower
  `oceanic_felsic_share` to turn back) so cells near the line don't flicker.
  Choose the values from bulk composition, not from outcomes: bulk continental
  crust is andesitic (~57–61% SiO₂, Rudnick & Gao 2003, Treatise on
  Geochemistry 3), which on a basalt–granite two-end-member mix is roughly a
  third to a half felsic. Record the derivation in the doc comment.
- **Thickness stops deciding type.** `continental_thickness_threshold` and
  `continental_nucleation_thickness` go from the type flip. A thick mafic arc
  or plateau stays oceanic; thinned continental crust stays continental until
  it is gone.
- **Density and buoyancy already follow composition** (phase 6
  `composition_density`). Behaviour that should depend on buoyancy uses
  column buoyancy, not the type label:
  - Subduction polarity and whether a column can go down at all
    (`slabs.rs` `initiating_over` today says "continent never goes down").
    A thick arc or plateau is buoyant enough to resist subduction and accretes
    as a terrane (Kohistan, Wrangellia, Ontong Java at the Solomon trench),
    while thin mafic floor goes down. Without this, removing thickness from
    the type flip would subduct island arcs wholesale and lose terrane
    accretion, a real growth path.
  - Welding versus subducting in advection pass 3, which today branches on
    `crust_type`.
- **Differentiation must not be gated on type.** `differentiate`
  (`crust_lifecycle.rs`) runs only on continental cells today. Under the new
  rule a mafic plateau or arc could then never become continental. Gate it on
  the physics it already models: base-of-crust temperature above the mafic
  solidus, i.e. thick or hot crust of any type. This is how plume plateaus
  and arcs turn into continent (TTG from thickened mafic crust; reworked
  arcs).
- **Crust that loses its continental part must lose its composition.**
  Recycling (`recycling.rs`) thins margins to `oceanic_thickness` and the
  lifecycle reverts them; the remaining crust is then given fresh-floor
  composition, or it would still read as felsic.
- **Drip** already reads the mafic lane (phase 6), so it acts on mafic arcs
  and plateaus whatever their type. Unchanged.

What this changes in the findings: the island-arc "piling" that drives late
growth becomes thick oceanic crust. It turns continental only by
differentiating or by accreting onto a continent and being reworked, so the
growth flux C has to balance is the real one.

Things to check while building D:

- Every place that reads `crust_type` (subduction, rifting strength,
  passive margins, recycling, erosion, heat flow, cratons, metrics,
  renderer). Classify each as "composition-defined type is right here" or
  "should read buoyancy or thickness instead". List them in the
  implementation plan before changing code.
- Plume-made early continent: plumes lay mafic crust, so early continent now
  depends on differentiation running fast enough on thick hot plateaus.
  `differentiation_rate` (0.002) was set when it only ran on continent;
  report early continental area, don't retune it to restore the old one.
- Cratons and keels: keel growth is tied to continental cells; check it
  still has somewhere to grow.
- Cost: the type flip reads lanes it already has; no runtime concern.

D is large enough to plan in detail on its own before coding (the
`crust_type` audit above is its first step), but it stays in this ticket
because C is calibrated against what it produces.

### C. Recycling from trench processes

Earth today: arcs add ~2.5–3 km³/yr of crust; sediment subduction (~1.65
km³/yr) and subduction erosion (~1.4 km³/yr) return about as much (Scholl &
von Huene 2007, GSA Memoir 200; Clift et al. 2009, Earth-Sci. Rev. 97). Both
losses happen at trenches and scale with how much margin there is and how
fast it converges, as arc growth does. Verify these figures against the
sources when writing the doc comments.

**C0. Measure first.** Before building C1, add a bench diagnostic for how
much eroded sediment is deposited within 1 ring of an active margin (a cell
with slab beneath it or bordering another plate across a convergent arc),
as a share of all deposition and in Earth-scaled km³/yr. Sediment fills
shelves first and only reaches `sediment_reach` (3) rings out, so this may
be a trickle. If it is below ~10% of the Earth sediment-subduction flux,
C1 alone can't carry the sediment part of recycling; find out why (routing
reach, shelf capture, active margins mostly submerged) before building it.

**C1. Sediment reaching a trench goes down.**

- In `erosion.rs` routing, a cell on the overriding side of an active margin
  is a trench outlet. Of the load that arrives there, a share
  `trench_sediment_subduction` (new tuning, default 0.7: most trench sediment
  is subducted; the rest builds the accretionary prism; confirm the figure
  from Clift et al. 2009) leaves into the subduction ledger
  (`subducted_this_step`) instead of being deposited. The rest is deposited
  there as now.
- The erosion ledger (eroded == deposited) becomes eroded == deposited +
  subducted; update its test.
- Add `sediment_subducted` to `AdvectionField` and the metrics.

**C2. Subduction erosion also wears intra-oceanic arcs.**

- In `advect.rs` pass 3, `margin_erosion` today only applies when the
  overriding cell is continental. Extend it to an oceanic overriding cell
  thicker than `ridge_crust_thickness` (arc crust), taking only the excess
  above ridge crust. Tonga and Izu-Bonin-Mariana forearcs are eroding today;
  intra-oceanic arcs are not exempt.
- No new tuning: same `margin_erosion` per unit of ocean consumed.

**C3. Retire the fixed rate, last.**

- Only once C1 and C2 are in and the 4-seed bench shows their losses are of
  the same order as the fixed rate's (~0.012% of the globe per Myr late in a
  run): set `continental_recycling_rate` default 0.0004 → 0. Before that,
  removing it takes away a working loss for an unproven one.
- With C1, C2 and the existing subduction erosion, the losses are explicit,
  and the fixed rate double-counts them (already flagged in ticket 16).
- Keep the code path and `hold_continental_area` controller unchanged as
  options.
- Tests in `world.rs` that set the rate to 0 stay valid. Re-check any test
  that relies on the default being non-zero.

**Calibration of C (physical inputs, then report).** Measure from one run at
~2500 Myr: arc addition, sediment subducted, margin erosion, all as Earth-
scaled km³/yr. Write down the scaling (unit sphere area to Earth's
5.1e14 m², level 6 cell ~100 km across against a real trench width) and its
uncertainty with the numbers, since a factor-3 window is wide enough to pass
for the wrong reason. If a flux is out by more than ~3×, look for the cause
in that process first. Don't scale it to make the continental area come out
at any figure.

## Steps

1. **A on its own.** Change `STAGNANT_AGE` and the collapse-age default,
   update docs and tests. Bench 20 seeds (see Verification). Proceed only if
   no seed falls into the warm regime; otherwise rethink A before going on.
2. **B, value first.** Read the sources, fix `modern_subduction_excess`,
   record it here. Then add it and `modern_subduction_width` in Rust
   (`tuning.rs`, `heat.rs`, `primordial.rs`, bench overrides) and TypeScript
   (`src/sim/tuning.ts`, slider and tooltip in the primordial group of
   `controls.ts`, persist). Unit tests:
   - the factor is 1 well above the transition, 0 well below, 0.5 at it;
   - arc accretion is full and drip zero well below the transition;
   - drip still removes thickened mafic floor at excess 0.9.
   Bench seeds a–d.
3. **D, crust type from composition.**
   - Audit every `crust_type` read and write it up (see D).
   - Fix `continental_felsic_share` and `oceanic_felsic_share` from bulk
     composition and record them here before any bench run.
   - Implement: type flip by basement composition with hysteresis;
     differentiation gated on Moho temperature, not type; buoyancy-based
     subduction polarity and pass-3 welding; fresh-floor composition on
     revert. Mirror tunings in TypeScript.
   - Unit tests: a 25 km mafic arc stays oceanic; it accretes rather than
     subducts at a trench; a thick hot mafic plateau differentiates and turns
     continental; a sediment cover on ocean floor does not make it
     continental; a cell near the threshold does not flicker; recycled
     margin crust reverts with fresh-floor composition.
   - Bench 20 seeds (A + B + D) and report continental area, composition and
     plume-made continent before C.
4. **C0.** Sediment-at-trench diagnostic; one run; decide whether C1 is
   worth building as designed.
5. **C1.** Trench outlets in erosion routing, `trench_sediment_subduction`
   tuning (Rust + TS), ledger change, metric. Unit tests:
   - a coastal range beside a trench sheds the set share of what reaches the
     trench into the ledger and deposits the rest;
   - a passive coast subducts none;
   - the ledger closes to 1e-9 with the new term.
6. **C2.** Margin erosion on oceanic overriding arc crust. Unit test: an
   intra-oceanic arc thickened to 20 km over a trench loses crust at the same
   rate per unit ocean consumed as a continental margin, and never below
   ridge-crust thickness.
7. **C3.** Recycling rate default to 0, only if the condition above holds.
   Run the full test suite.
8. **Diagnostics** (Rust bench and TS `diagnostics.ts`):
   - continental budget by process per 300 Myr: plume, arc, piling, spreading
     gains; margin erosion, sediment subducted, eaten-margin erosion, drip,
     delamination, fixed recycling (should read 0 after C3) losses;
   - growth timing: Myr at which continental mass reaches 50% and 80% of its
     3000 Myr value (reported only);
   - Earth-scaled fluxes (km³/yr) for arc addition, sediment subduction and
     subduction erosion at the end of the run, with the scaling stated;
   - piling by origin (recent arc, plume, sediment, in-plate welding), so the
     welding artefact stays visible.
9. **Combined verification** (see Verification and Acceptance).
10. **Write-up.** Report outcome numbers against the old ensemble, including
    the land-share shift, before changing any band in `scripts/compare.ts`.
    Update ticket 16's recycling note.

## Verification

- **Seed count.** 20 seeds at level 6, 1500 ticks. With 8 seeds a warm
  regime still occurring 1 run in 10 would be missed 43% of the time
  (0.9⁸); with 20, 12% (0.9²⁰). On 10 cores that's ~50 min per batch.
  If that's too slow for the per-step checks, use 20 seeds at level 5 for
  steps 1–7 and keep level 6 for the final run, after first checking on
  4 seeds that level 5 shows the same split.
- Default seed also at 1 and 4 threads (determinism); one seed at level 7
  for cost.

## Acceptance

Pass/fail tests are internal consistency and physics only. Earth-shaped
outcomes are reported, not targeted.

**Pass/fail**

- **No stagnant lid:** across 20 seeds, no run has fewer than 20 plates or
  a mean passive-margin age over 400 Myr at any point after 1000 Myr.
- **Balance:** over the last 500 Myr of every seed, total continental gains
  and losses within ±30% of each other.
- **Process sanity:** every loss process in the budget is non-zero where
  its setting exists (active margins, intra-oceanic arcs), and the fixed
  recycling term reads 0 after C3.
- **Determinism:** same fingerprint at 1 and 4 threads; mass drift stays
  zero; the erosion ledger closes.
- **Cost:** ms/tick at level 6 within 5% of now.

**Reported, discussed, not targeted**

- Continental area per seed, its spread across seeds, and how many seeds land
  in each regime (if any split remains).
- Growth timing (50% and 80% of final continental mass), mantle excess over
  time, plate count, ocean age, sink dipole.
- Earth-scaled fluxes against Scholl & von Huene / Clift, with the scaling
  and its uncertainty.
- Land share, hypsometry and continental composition (felsic/mafic/
  sediment).
- If any of these look wrong, the response is to look for the wrong process,
  not to move a dial toward an Earth figure.

## Risks

- **Circular dial.** B's value decides growth timing. Mitigated by fixing it
  from the sources before any B bench, recording it here, and not testing
  growth timing pass/fail.
- **A's untested combination fails.** Step 1 runs A alone on 20 seeds first.
- **C balances an artefact.** Mitigated by D landing before C.
- **D breaks early continent.** Plume plateaus now need differentiation to
  become continent. If early continent all but vanishes, check the
  differentiation physics (Moho temperature, melting span, rate) rather than
  lowering the felsic threshold.
- **D subducts arcs wholesale.** If buoyancy-based polarity is missed
  anywhere, thick mafic arcs and plateaus will go down trenches instead of
  accreting. The unit test and the `crust_type` audit cover this.
- **D's reach.** `crust_type` is read in many passes; the audit may find more
  than this ticket can carry. If so, split D into its own ticket and pause C.
- **C1 is inert.** C0 measures how much sediment reaches trenches before C1
  is built.
- **Losing a loss.** The fixed rate goes last, and only once C1 and C2 are
  shown to carry a comparable loss.
- **Soft flux check.** The Earth scaling is stated with its uncertainty;
  a pass inside the factor-3 window doesn't prove the process is right.
- **Too little continent.** B and C together might settle well below today's
  ~28% in cool runs. That is an outcome, not a fail; check fluxes before
  anything else.
- **Earlier growth feeds back on the regime.** More continent earlier means
  more insulation and keels while the mantle is hot, which could bring back
  a lid. Watch plate count and sink dipole at each step.
- **Plume-made continent.** Drip on plume crust fades earlier under B, so
  more plume plateau may survive as continent. Report the plume share.
- **In-plate welding stays.** 10–40% of piling is crust welded inside plates
  (likely a grid artefact) and none of A–C touches it. It stays visible in
  the piling-by-origin diagnostic, and is a follow-up.
- **Bands.** `scripts/compare.ts` continental-area band will need a new
  baseline. Re-baseline only after the write-up is agreed.

## Cost

- Code: large. A is a few lines; B ~60 lines plus tests and TS mirroring;
  D is the largest part, sized by the `crust_type` audit; C0 ~40 lines of
  bench diagnostic; C1 ~80 lines in erosion routing plus ledger and tests; C2
  ~20 lines; C3 one default.
- Runtime: negligible. C1 adds one check per routed cell; C2 one branch in
  pass 3.
- Bench time: 20-seed batches at ~50 min (level 6) or less at level 5, per
  step, plus a final level 6 batch.
- Reading: the B sources and the C flux sources, before coding each.

## Follow-ups (not in this ticket)

- **Resource rules read the whole column, not the exposed rock.** Option 1
  from the gold review: derive the exposed lithology from the layering, with
  plume and accreted ocean crust mafic at the surface. Revisit after D: with
  composition-defined continents, the column-average problem shrinks but
  doesn't vanish.
- **Ocean crust welded inside plates** (advection pass 3, ~5% of ridge
  production): likely a grid artefact of quantised rotation.
- **Mantle viscosity sensitivity.** `viscosity_sensitivity` 0.1 gives ~10%
  lower drag at excess 1; an Arrhenius law with ~300 kJ/mol gives ~30×. The
  physics is contested for the Archean (Korenaga's sluggish-lid argument), so
  look at it separately.
- **Seed `b` outlier** (current rule, default collapse age): 31 plates and
  232 Myr mean ocean age at 3000 Myr. Recheck after A.

## Dependencies

- Rock-cycle phase 6 (density from composition) in the working tree.
- 16 (erosion and sediment) is in; C1 changes its routing and ledger.
