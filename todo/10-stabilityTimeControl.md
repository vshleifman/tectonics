# 10 — Numerical stability & time control

## Goal

Stable deep-time runs and better control over the simulation clock.

## Why it matters

Hundreds of millions of years means many steps; small errors compound, and large
time steps move crust too far per step for the advection assumptions to hold.
Players also need to control simulation speed and inspect elapsed time.

## Scope

- Sub-stepping so crust never moves much more than one cell per internal step.
- Time controls: speed (Myr/tick), step-once, and an elapsed-Myr readout.
- Watch elevation and mass for unbounded drift; clamp or relax where needed.

## Acceptance criteria

- At high subdivision and high speed the world stays stable (no exploding
  elevation, no runaway mass).
- Time readout advances correctly; step-once and speed controls work.

## Dependencies

- 05, 07, 08 (stability only matters once the step does real physics).
