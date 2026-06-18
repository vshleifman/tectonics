# 12 — Subduction speckle fix: melt subducting crust at convergent cells

## Goal

Stop subducting-plate cells from surviving on the overriding plate's side of a
convergent boundary, so plates read as solid regions instead of an intermixed
multicoloured band.

## Why it matters

The forward-scatter advection is not a bijection, so along a convergent boundary
the overriding plate's own cell sometimes misses a destination, leaving it
claimed only by the subducting plate. Today subduction fires only on a 2+ claim
collision, so a lone foreign claimant is accepted as the winner and never
ledgered: denser foreign crust lands on the wrong side (speckle) and "survives"
instead of melting back into the asthenosphere.

## Scope

All contained to `src/sim/advect.ts`; the `advect` signature, `AdvectionField`,
and `step` contract are unchanged.

- Extract the inline ledger arithmetic into a `subduct(loser, into)` helper
  (accrete `ACCRETION_FRACTION` onto the survivor, record the rest in the
  subduction ledger) shared by both subduction paths.
- Pass 1: before winner resolution, if a destination is a convergent cell and
  the incoming source is foreign (different plate) and denser-or-equal to the
  destination's own crust, melt it via `subduct` and skip it — it never occupies
  the cell. Foreign buoyant crust still falls through to normal resolution so
  genuine overriding / boundary migration is preserved.
- Pass 2: fill convergent holes from the own-plate back-rotation (`gatherSource`)
  only, skipping the `dominantNeighbourSource` fallback that can otherwise bleed
  the advancing subducting plate's id across the boundary.

## Acceptance criteria

- Convergent boundaries in the Plates view read as a clean line between two solid
  plate colours, not an intermixed multicoloured band.
- Every melted cell (lone or collision loser) is ledgered, so the mass drift
  readout stays near zero.

## Dependencies

- 05 (conservative advection) and 08 (crust lifecycle / ageing-driven density).
- Cross-cutting decision: subduction rule (strictly density-driven here).

## Out of scope

- Symmetric handling of a foreign buoyant single claimant overriding a cell
  (boundary migration, not a speckle source).
- Whole-cell quantised motion / ~2-cell boundary churn, deferred to the
  smooth-motion advection rewrite (09/10).
