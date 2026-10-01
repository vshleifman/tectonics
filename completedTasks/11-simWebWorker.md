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

## Outcome

Done as part of the Rust/WASM worker plan
(`.cursor/plans/rust_wasm_sim_worker_d0e3112f.plan.md`).

- The simulation runs in `src/worker/sim.worker.ts` behind the message
  protocol in `src/worker/protocol.ts`. The main thread (`SimClient`) only
  reads snapshots, whose buffers ping-pong between the threads by transfer.
- The per-tick simulation is the Rust crate in `sim-rs/`, compiled to WASM.
  Before the TypeScript simulation was removed, the two produced identical
  metric trajectories on the benchmark scenario (`?bench=` in the browser,
  `npm run bench` headless) at levels 6 and 8.
- Level 8 (655k cells), 12 seeds, 200 ticks, headless: 911 ms/tick for the
  original TypeScript, 188 ms/tick after the Phase 1 quick wins, 98 ms/tick in
  WASM. In the browser the UI holds 60 FPS while the worker ticks flat out.
