# 11 — Density-driven plate motion (slab pull)

## Goal

Replace the random per-plate Euler poles with motion that emerges from the
crust each plate carries, so plates drift because their dense edges sink rather
than because they were seeded with a random spin.

## Why it matters

Plate motion is currently arbitrary: a fresh plate gets a uniformly random
Euler pole and speed (`freshPlate` in `src/plates/PlateData.ts`), and splits
just inherit-then-perturb their parent. Nothing about a plate's crust — its
density, age, or which boundaries it touches — influences where it goes. That
makes boundary behaviour feel unmotivated and decouples the most important
driver of real tectonics (slab pull: a dense, old plate edge sinking at a
convergent boundary and dragging the rest of the plate after it) from the
simulation. Tying motion to crust closes the loop between geology and movement
and keeps boundary type emergent rather than baked in at seed time.

## Scope

- Derive each plate's angular velocity `omega` from a force balance over its own
  boundary cells instead of from an RNG:
  - A slab-pull term at convergent boundaries, weighted by the subducting
    crust's `density`/`age` (older, denser oceanic crust pulls harder).
  - Optionally a weaker ridge-push term at divergent boundaries.
- Sum the per-boundary tangential forces about the plate centroid into a single
  Euler pole (`omega = normalise(torque axis) * speed`), keeping motion a rigid
  rotation on the sphere.
- Recompute `omega` periodically (each step, or every N steps) so motion
  responds as crust ages, subducts, and boundaries shift — not just once at
  seed time.
- Retire the random `freshPlate` pole and the `perturbOmega` split jitter, or
  keep them only as a tiny tie-breaker for plates with no active boundary force.

## Why "toward the fault seed" was rejected

An earlier idea was to aim each plate at the origin point of the fault that
created it. It is discarded because the seed is only the topological origin of a
crack, not a physical subduction zone; it would force boundaries near seeds to
be permanently convergent (undercutting the emergent classification from 04);
and a plate's rim can touch faults from several seeds, so the mapping is
ambiguous. Density-driven motion captures the same intuition (a heavy edge drags
the plate) without any of those problems.

## Acceptance criteria

- Plates with old/dense oceanic edges accelerate toward those edges over a run;
  plates of light continental crust drift only weakly.
- Boundary classification still emerges from relative velocity (no boundary type
  is hard-coded by the motion model).
- Disabling the force term (or zero crust contrast) reproduces near-stationary
  plates rather than NaNs or runaway spin.
- A velocity-arrow readout visibly correlates with crust density/age.

## Dependencies

- 04 (boundary classification) — done; provides per-arc convergent/divergent
  labels and signed closing speed to weight the forces.
- Reads per-cell `density`/`age` from `CellData` and plate centroids from
  `PlateData.centroids`.
- Plays into the cross-cutting subduction rule (overview): forces should agree
  with whichever crust the subduction rule consumes.

## Open questions

- How often to recompute `omega` (every step vs every N steps) and whether to
  smooth it so motion does not jitter as boundaries flicker.
- Whether plate inertia / a relaxation term is needed, or instantaneous force
  balance is enough for the timescale.
- Speed calibration: mapping summed force to a `rad/Myr` spin that stays within
  the existing `MIN_SPEED`/`MAX_SPEED` feel without hand-tuning per world.
- Interaction with splits — should a freshly rifted fragment start from its
  parent's `omega` and relax toward its own force balance, or recompute cold.
