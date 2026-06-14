What I implemented
Replaced the lossy advection with a hybrid scatter/gather scheme that conserves crust and turns the two boundary artefacts (convergence pile-up, divergence gaps) into explicit, handleable events. All changes are contained to src/sim/advect.ts; the advect signature, the AdvectionField scratch and the step contract are unchanged, so the function stays pure over the mesh interface + flat typed arrays (Worker/WASM-ready).

Forward-scatter framework
Each cell scatters its crust forward along its plate's rotation:

- Pass 0 — for every source cell, rotate its centre about its plate's Euler pole and find the nearest destination cell (nearestCell, the greedy hill-climb from 03). dest[i] is where that crust lands.
- Pass 1 — resolve each destination's winner. On a collision (2+ sources claim one cell) the more buoyant (lower-density) crust survives on top, ties by index.
- Pass 2 — write the surviving crust into each destination, open a rift at a divergent gap, or fill an interior hole (see below).

Conservation — subduction gated to real convergence
A collision only destroys crust when the destination is a genuinely CONVERGENT boundary cell (from ticket 04's boundaries.cellKind). There the loser subducts: a fraction (ACCRETION_FRACTION = 0.5) of its mass accretes onto the survivor (thickening it), the rest is recorded in the subduction ledger. Because liveMass + subductedTotal is held constant, no crust is duplicated or silently lost, and the subducted / riftCell event flags now reflect only real geology — exactly what 06 (elevation), 07 (crust lifecycle) and 11 (slab pull) consume. An interior collision (discretisation noise, not a convergent boundary) is NOT ledgered: the survivor simply occupies the cell and the loser is dropped, so plate interiors are not eroded by grid aliasing. liveMass is recomputed from dst each step, so the UI drift readout stays honest.

Divergence — rift placeholders preserved
A destination that no source claims and that sits on a DIVERGENT boundary is left as an empty rift placeholder (oceanic, age 0, RIFT_DENSITY, zero thickness) and flagged in riftCell for 07 to fill. This branch is untouched by the hole-fill work below.

Hole handling — the hard part (and where most of the iteration went)
The forward map i -> nearestCell(rotate(pos(i))) is not a bijection on the discrete grid, so each tick many destinations get 0 source claims even inside rigid plates. Filling those holes is what produced the visible artefacts, resolved in three steps:

- First attempt (stale-island speckle): a 0-claim non-divergent hole self-copied its own old crust. Where a neighbouring plate had swept over the surrounding cells, the skipped cell was left as an island of the old plate's id -> speckling.
- Second attempt (trails): switched to gatherSource — back-rotate the hole by its own plate's motion and copy the upstream cell. This fixed interior speckle but smeared a trail behind every moving plate, because a vacated trailing-edge cell was refilled with the departing plate's crust, so plates never cleared the ground they left.
- Third attempt (neighbour bleed): filled every hole from dominantNeighbourSource (the plate dominating the hole's already-resolved neighbours). This killed the trails but let the adjacent plate's id bleed across boundaries in dashed rows wherever a near-boundary hole's neighbours were dominated by the other plate.

Final scheme — discriminate the two hole types. For a hole, compute ownSource = gatherSource (back-rotation by the cell's own plate):

- If ownSource is a same-plate cell, the plate still rotates into the hole -> it is an interior aliasing gap; keep it same-plate (no foreign bleed, no speckle).
- Otherwise the plate has genuinely vacated the cell -> hand it to the plate sweeping in (dominantNeighbourSource), so an advancing plate takes over the trailing edge instead of the departing plate smearing a trail. A wholly vacated pocket with no claimed neighbour keeps ownSource.

This gives both: no trails and no boundary speckle.

Decisions worth flagging

- Hybrid scatter/gather was chosen over pure occupancy-counting or a full flux rewrite (the three options the ticket left open). It is the smallest correct change, is step-size independent, and keeps the existing scatter framework and event flags.
- Subduction is strictly density-driven for now (buoyant survives, denser subducts); ACCRETION_FRACTION and RIFT_DENSITY are exposed consts for 06/07 tuning.
- Interior collision drop and interior gather/neighbour backfill are tiny, roughly-cancelling discretisation noise, accepted within the ticket's mass tolerance.

Known limitation (deferred to 09)
Motion is still whole-cell and quantised: at the current settings a plate edge can shift ~2 cells per step (and the rotation makes displacement vary across a plate), so the boundary between two relatively-moving plates still churns within a ~2-cell band each tick. nearest-cell snapping cannot be smoothed by sub-stepping alone (sub-cell steps round to no motion), so genuinely smooth deep-time motion needs the flux / mass-transfer advection rewrite — folded into ticket 09 (stability + time control), whose real goal is smooth motion rather than just a faster clock.

Deferred as scoped: elevation response (06), crust lifecycle / rift filling (07), and smooth-motion advection + time controls (09).
