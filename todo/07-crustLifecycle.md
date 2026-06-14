# 07 — Crust lifecycle: creation at rifts, destruction at subduction

## Goal

Create and destroy crust in the right places, keeping the conservation budget
honest.

## Why it matters

Plates that spread must make new crust; plates that converge must consume it.
Without this, divergent boundaries leave holes and the mass budget never
balances over deep time.

## Scope

- Rift cells (from 05): spawn young oceanic crust (oceanic type, age 0, ridge
  elevation, young-oceanic density) and assign an owning plate.
- Subducted crust (from 05): removed from the budget, with the lost mass
  recorded for conservation accounting.
- Age every cell each step and recompute oceanic density from age.

## Acceptance criteria

- Crust view shows young crust appearing at ridges and ageing outward.
- The oldest oceanic crust is what subducts.
- Net mass change equals created minus destroyed.

## Dependencies

- 05 (needs rift/subduction events).
- Cross-cutting decision: rift crust ownership; subduction rule.
