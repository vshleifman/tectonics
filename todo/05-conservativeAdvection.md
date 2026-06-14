# 05 — Conservative advection (convergence/divergence resolution)

## Goal

Replace the lossy gather with a scheme that conserves crust and exposes pile-up
and gaps for geology to act on.

## Why it matters

The current advection duplicates crust where plates converge and smears stale
crust where they diverge. Without conservation, terrain and deep-time runs are
meaningless. This ticket turns those artefacts into explicit, handleable events.

## Scope

- Detect convergence (a source claimed by more than one destination) and
  divergence (cells no plate advects into).
- At convergence: denser crust subducts, buoyant crust survives and thickens.
- At divergence: mark the gap as a rift needing fresh crust (handled in 07).
- Track an area/mass budget so conservation can be verified.

## Acceptance criteria

- Total tracked crust mass stays within a small tolerance over thousands of
  steps (verified via a mass readout).
- No visible duplication smear at convergent edges.

## Dependencies

- 04 (needs boundary classification).
- Cross-cutting decision: conservation strategy.

## Open questions

- Detection approach (e.g. forward-scatter occupancy counting vs hybrid
  scatter/gather vs explicit mass-transfer accounting) — to be decided in
  planning.
