# 06 — Elevation response (terrain genesis)

## Goal

Make boundaries build relief so the simulation produces recognisable terrain.

## Why it matters

This is the visible payoff of the whole feature: mountains, trenches, and ridges
emerging from plate interaction. It is also the hardest tuning problem.

## Scope

Elevation rules per boundary type and crust pairing:

- Convergent oceanic-continental: trench plus volcanic mountain arc.
- Convergent continental-continental: collision range (both sides rise).
- Convergent oceanic-oceanic: island arc plus trench.
- Divergent: mid-ocean ridge flanked by deepening ocean.
- Transform: minimal vertical change.
- Couple elevation to crust age/density (oceanic crust deepens as it ages).

## Acceptance criteria

- Running from a cracked planet yields plausible terrain — mountains at
  collisions, trenches at subduction, ridges at rifts — visible in the existing
  elevation colour mode.

## Dependencies

- 05 (needs convergence/divergence resolved).

## Open questions

- How literal vs. tuneable to be initially. Start with simple monotonic rules
  and expose the constants for tuning.
