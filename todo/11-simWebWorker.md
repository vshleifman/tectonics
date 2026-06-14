# 11 — Move simulation into a Web Worker (optional)

## Goal

Keep rendering at 60fps under heavy per-step physics by running the simulation
off the main thread.

## Why it matters

Once the step does real work (04-10), a large world could stutter the UI. The
step is already a pure function over typed arrays, so it can move to a worker
without restructuring upstream.

## Scope

- Run the simulation step in a Web Worker, transferring cell-data buffers across
  and posting the advected result back.

## Acceptance criteria

- UI stays responsive (no frame drops) while the simulation runs on a large
  world.
- Simulation output is identical to running on the main thread.

## Dependencies

- Worth doing only after 04-10 make the step expensive enough to justify it.
