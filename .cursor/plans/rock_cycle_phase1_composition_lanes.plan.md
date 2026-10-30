---
name: "Rock cycle phase 1: composition lanes"
overview: "Replace the hand-copied plume_share and sediment_share tracers with one lane-major block mixed by mass in a single loop, so later phases add a lane with a constant. No behaviour change: the fingerprint must match bit for bit."
todos:
    - id: lanes
      content: Add Lanes (lane-major Vec<f32>, LANE_PLUME = 0, LANE_SEDIMENT = 1, LANES = 2) and a per-cell mix that applies mixed_share lane by lane, in cell_data.rs
      status: completed
    - id: advect
      content: Swap every per-tracer site in advect.rs for the lane block, and turn AdvectionField's welded_share/welded_sediment/spill_share/spill_sediment/spill_*_in into lane blocks
      status: completed
    - id: callers
      content: Move cohesion swap, lay_new_floor reset, erosion deposit, primordial raise and drip, metrics and lib.rs onto lanes
      status: completed
    - id: fingerprint
      content: Split Sim::fingerprint into dynamics_fingerprint and composition_fingerprint (combined hash unchanged) and print both from the native bench
      status: completed
    - id: verify
      content: Fingerprint identical to main at level 6 over 3000 Myr, 1 and 4 threads; advect pass time within noise
      status: completed
isProject: false
---

# Rock cycle phase 1: composition lanes

Part 1 of 6. Index: [rock_cycle_and_resources.plan.md](rock_cycle_and_resources.plan.md).

## Why

`plume_share` and `sediment_share` are mixed by hand at about 20 places. Most are in [sim-rs/src/advect.rs](sim-rs/src/advect.rs), and each has its own scratch arrays on `AdvectionField` (`welded_share`, `welded_sediment`, `spill_share`, `spill_sediment`, `spill_share_in`, `spill_sediment_in`). Phases 2–4 add four to six more lanes. Copied by hand, every advect change would touch every lane. This phase changes how the tracers are stored and nothing else.

## Changes

- [sim-rs/src/cell_data.rs](sim-rs/src/cell_data.rs):
  - `pub struct Lanes { n: usize, v: Vec<f32> }`, stored lane-major (lane `l` occupies `v[l*n..(l+1)*n]`), so each lane is a contiguous slice for `par::fill`, `Disjoint` and the WASM views.
  - Lane constants `LANE_PLUME = 0`, `LANE_SEDIMENT = 1`, `LANES = 2`.
  - `type Mix = [f64; LANES]` for the makeup of crust being added.
  - Helpers: `lane(l)`, `lane_mut(l)`, `get(i) -> [f32; LANES]`, `set(i, ..)`, `swap(a, b)`, `split_mut() -> [&mut [f32]; LANES]`, and `mix(i, before, added, &Mix)`, which calls the existing `mixed_share` once per lane. Keep `mixed_share` and its f64 arithmetic exactly as it is; bit-identity depends on it.
  - `CellData.lanes: Lanes` replaces `plume_share` and `sediment_share`.
- [sim-rs/src/advect.rs](sim-rs/src/advect.rs). Line numbers are as of `60d36f1`:
  - gather (near 810): one `gather` per lane in a loop;
  - continental welding and the free-standing weld in pass 3 (near 864 and 891): accumulate `kept * share` into a `welded_lanes: Lanes` on `AdvectionField`;
  - pass 4 gains (near 915–951): `Disjoint` per lane from `split_mut`, mixing accreted crust in with `Mix` all zero, as now;
  - surplus placement (near 1045), spill (near 1074–1120, 1221, 1246, 1301–1313) and the two pull loops (near 1445 and 1501): the same, with `spill_lanes` and `spill_lanes_in` replacing the per-tracer arrays.
  - Keep the order of every `add_f32` and every mix as it is; float addition order is what the fingerprint sees.
- Other callers:
  - the swap in [sim-rs/src/cohesion.rs](sim-rs/src/cohesion.rs) (near 36);
  - the reset in `lay_new_floor` in [sim-rs/src/crust_lifecycle.rs](sim-rs/src/crust_lifecycle.rs) (near 151);
  - the deposit in [sim-rs/src/erosion.rs](sim-rs/src/erosion.rs) (near 130), with `mix_lane(LANE_SEDIMENT, .., 1.0)`: only the sediment lane mixes on deposit, as before (a full `mix` would dilute the plume lane too);
  - `raise` (near 170) and `hot_drip` (near 375) in [sim-rs/src/primordial.rs](sim-rs/src/primordial.rs). Drip keeps `1 - plume - sediment` word for word;
  - `sediment_mass` in [sim-rs/src/metrics.rs](sim-rs/src/metrics.rs) (near 175);
  - the thread-parity test in [sim-rs/src/world.rs](sim-rs/src/world.rs) (near 882), which loops over lanes.

## Fingerprint

`Sim::fingerprint` hashes `plume_share` and then `sediment_share`, after `lithosphere`. Lane-major storage with plume first visits the same bits in the same order, so the combined hash does not change. Add:
- `dynamics_fingerprint`: every field except the lane block. Later phases also leave out their own composition fields here.
- `composition_fingerprint`: the lane block and later composition fields.

`fingerprint` keeps hashing everything in today's order. The native bench ([sim-rs/src/bin/bench.rs](sim-rs/src/bin/bench.rs), near 1537) prints all three.

## Exposure

- [sim-rs/src/lib.rs](sim-rs/src/lib.rs): `sediment_ptr` becomes `lane_ptr(LANE_SEDIMENT)`. Add `lane_ptr(lane: u32)` and `lane_count()` so later phases expose lanes without new exports.
- TypeScript stays as it is: `WasmWorld.ts` keeps reading the sediment view through the same pointer.

## Verification

- Native bench, level 6, 3000 Myr, at 1 and 4 threads: `fingerprint` matches `main` exactly. Any difference is a bug in this phase, not drift to accept.
- `cargo test`, including `the_world_is_the_same_on_one_thread_and_on_four`, `the_tracer_never_holds_more_plume_crust_than_plumes_made` and the erosion ledger tests, with no edits beyond field access.
- The advect pass time (timings slot 3) is within run-to-run noise of `main`. If the lane loops cost more than a few per cent, use fixed-size array loops over `LANES` (it is a `const`) before going further.
