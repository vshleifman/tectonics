# 08 — Plate identity reconciliation under motion

## Goal

Keep per-plate properties in sync as plates grow, shrink, merge, or vanish
during the simulation.

## Why it matters

`plateId` advects with crust, but per-plate properties are currently reconciled
only at seed/crack time. During a run a plate can disappear (fully subducted) or
new rift crust can need an owner, so identity bookkeeping drifts out of sync.

## Scope

- Retire plate ids with zero member cells so per-plate state and the plate-count
  readout stay accurate.
- Decide how rift crust gets an owner (inherit the spreading plate vs new plate).
- Ensure the live plate count shown in the UI matches what is on screen.

## Acceptance criteria

- Long runs do not leak retired plate ids.
- Plate-count readout matches the visible plates.

## Dependencies

- 06 and 07 (plates appear/disappear once geology and lifecycle are active).
- Cross-cutting decision: rift crust ownership.
