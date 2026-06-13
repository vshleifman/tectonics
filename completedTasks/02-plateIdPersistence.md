What changed
New: src/plates/PlateRegistry.ts — the persistence tracker, kept pure and free of plate properties so it's unit-testable and the flood fill stays untouched. relabel(raw, rawCount, out) implements your algorithm exactly:

tally overlap[rawLabel] = Map<prevId, count>,
each raw label claims its max-overlap predecessor,
conflict resolution — on a split, the largest-overlap claimant keeps the id, others are treated as new,
allocate from a monotonic nextId (winners inherit; losers/orphans get fresh ids tagged with parentOf),
write stable ids back into data.plateId in place (all reads finish before writes, so prev-in/stable-out can share the array),
emit { keptIds, newIds, retiredIds, parentOf }.
It tracks liveIds for retirements and hasPrev so the first run after reset() treats prev as empty → ids 0,1,2,… as before.

PlateData.ts — now keyed by stable id via Map<number, PlateProperties> (omega + continental), with linearVelocity(id, pos) and centroids() returning a Map<id, Vec3>. Replaced initialisePlateProperties with:

reconcilePlateProperties(plateData, report) — kept plates untouched (no flicker, stable pole/colour), new plates inherit-then-perturb from their parent (or fresh random with no parent), retired plates dropped.
initialiseCrust(...) — seeds per-cell crust/age/density once at world build; relabels no longer rewrite it, so crust advects/persists with cells.
Plates.ts — assignPlates(out: Int32Array) now writes raw labels into a caller-provided scratch buffer (pure component labeller).

CellRenderer.ts — updateVelocities iterates the centroid map by stable id.

main.ts — added a module-level PlateRegistry + reusable rawLabels scratch and a single reassignPlates(initCrust) helper used by build/seed/clear. reset() runs only on subdivision rebuild; clearing faults is now a merge (identity survives).

Policy decisions I made (and how to change them)
Split child: inherit-then-perturb (your lean). Parent's omega jittered per-axis by SPLIT_PERTURB = 0.15; continental flag copied. Tune in PlateData.ts.
Merge/conflict winner: largest overlap (your numbered algorithm), with ties broken toward the smaller (older) id — a nod to your older-id-wins stability preference without overriding area.
Side benefit confirmed: colourForPlate keys hue off the (now stable) id, so a surviving plate keeps its colour across edits.
One consequence worth flagging: because the prototype starts as a single plate, persistent per-cell crust means the crust view stays uniform until rifting introduces genuinely new cells — that's the geologically-correct advection behaviour you described, but it does remove the old per-plate crust re-randomisation in the crust/density views.
