---
name: Persistence
overview: "Resume after a reload: autosave the full Rust Sim state to IndexedDB from the worker and restore it on start-up, rejecting saves from a different sim build. Persist UI settings and tuning overrides in localStorage."
todos:
    - id: rs-serde
      content: Derive Serialize/Deserialize on the persistent sim structs; enable indexmap's serde feature; add postcard
      status: completed
    - id: rs-state
      content: Add Sim::save_state / Sim::from_state (header + postcard body, mesh rebuilt from level) and wasm exports
      status: completed
    - id: rs-version
      content: build.rs hashes sim-rs/src into SIM_SOURCE_HASH; from_state rejects a mismatch
      status: completed
    - id: rs-test
      content: Round-trip test - tick, save, restore, tick both, assert bit-identical state
      status: completed
    - id: ts-engine
      content: SimWorld.saveState, Engine.restore, WasmWorld built from an existing World
      status: completed
    - id: ts-worker
      content: Worker autosave to IndexedDB (dirty flag, interval, pause, save command) and resume on init
      status: completed
    - id: ts-protocol
      content: init gains resume flag; new `resumed` reply carries generation, level and tick
      status: completed
    - id: ts-settings
      content: localStorage load/save for Settings (minus running, seedMode) and tuning/simOptions overrides
      status: completed
    - id: ts-main
      content: main.ts applies resumed state to store, tuning and sliders; sends save on pagehide/visibility hidden
      status: completed
    - id: verify
      content: test:rs, typecheck, lint, then reload the app mid-run in both builds
      status: completed
isProject: false
---

# Persistence

Goal: reload the page and carry on with the same world, settings and tuning.
Saves from an older sim build are dropped. This is still the development phase,
so that's acceptable.

## 1. Rust: snapshot of `Sim`

### What gets saved

| Field | Saved? | Why |
|---|---|---|
| `mesh` | No | Rebuilt from `level` (deterministic icosphere) |
| `front: CellData` | Yes | The crust |
| `back: CellData` | No | Restored as `front.clone()`; the round-trip test confirms it is pure output |
| `plates: PlateData` | Yes | Omegas, sutures, quiet, welds, rifts, breakups, counters |
| `registry` | Yes | `next_id`, `live`, `has_prev` |
| `boundaries` | Yes | Cheap; avoids guessing whether `drive_plates` reads last tick's |
| `advection` | Yes | Holds running totals (`subducted_total`, `created_total`, `live_mass`) and last-step tallies that `steer_accretion` reads |
| `mantle` | Yes, whole | 642 sites; the geometry could be rebuilt but the saving isn't worth the split |
| `primordial` | Yes | Plumes and the `Rng` state |
| `heat` | Yes | Temperatures, `rate`, `heat_flow` |
| `dyn_topo`, `sea_level`, `elapsed`, `initial_mass`, `substeps` | Yes | |
| `tuning`, `options` | Yes | Needs `Serialize` added to the existing `Deserialize` derives |
| `scratch`, `raw_labels`, `timings` | No | Recreated empty. After the load, call `scratch.pack_plates(&plates)` in case anything reads `plate_omega` before the first tick |

Rough size at level 6 (40,962 cells), from about 60 per-cell 4-byte fields: ~10 MB.
IndexedDB handles that easily.

### Format

- `postcard` body behind a small header: magic `b"TECT"`, `SIM_SOURCE_HASH` (u64), `level` (u32).
- `build.rs` hashes every `sim-rs/src/**/*.rs` file into `SIM_SOURCE_HASH`. Any sim
  source change invalidates saves, so there's no manual version to forget to bump.
  The cost is that a comment edit also drops the save. That's fine for development.
- `Sim::save_state(&self) -> Vec<u8>` and `Sim::from_state(&[u8]) -> Result<Sim, String>`.
- Deserialise into a private `SimState` struct of borrowed/owned fields, so the
  skipped fields don't need `Default` or `#[serde(skip)]` sprinkled through `Sim`.

### Wasm surface (`lib.rs`)

- `World::save_state(&self) -> Vec<u8>`
- `#[wasm_bindgen(js_name = fromState)] World::from_state(bytes: &[u8]) -> Result<World, JsError>`
- `World::level()`, plus `World::tick_count()` if the worker's `tick` counter isn't kept in the TS envelope (it will be, see below)

### Test (`world.rs` tests)

1. Build a level-4 world and run 30 ticks.
2. Save it, restore it into a second `Sim`, then run 20 more ticks on both.
3. Assert bit-equal `CellData`, plate omegas, `elapsed`, `sea_level` and mass drift.

This test is the real guarantee that the skipped list above is right. It runs
single-threaded under the default features. Watch out for one thing: `PlateData.stress`
is a `HashMap`. If its iteration order feeds the result, the test will flake. Fix that
by switching it to `IndexMap`, as the other maps already are.

## 2. Worker

- **IndexedDB:** one database `tectonics`, one store `saves`, one key `autosave`.
  The value is a structured-clone envelope:
  `{ level, tick, cracked, seeds, sim: Uint8Array }`. Tuning and options are not in
  it beyond what `sim` holds: see section 4.
  The worker writes to IndexedDB itself, so nothing large crosses to the main thread.
- **Dirty flag:** set by `advance`, `reassign` (seed placement), `setTuning`, `setSimOptions` and `init`.
- **Autosave:** when dirty, every 10 s while running. Also immediately on `pause`
  and on a new `save` command. Serialising ~10 MB costs a few tens of ms, which is
  acceptable at that interval.
- **`init { resume: true }`:**
  - Read the slot and try `Engine.restore(bytes)`.
  - On success, rebuild `Plates` from `cracked`/`seeds`, set `tick`, post `meshReady`,
    then a `resumed` reply.
  - On missing data or a version mismatch, log one line, delete the slot and build fresh as now.
- **`init` without resume** (level change, new world): build fresh and mark dirty, so
  the old world is overwritten on the next save rather than coming back on reload.

## 3. Protocol and engine (TS)

- `SimCommand`: `init` gains `resume?: boolean`; add `{ type: "save" }`.
- `SimReply`: add `{ type: "resumed"; level; tick; branches; tuning; options }`.
- `SimWorld`: add `saveState(): Uint8Array`.
- `Engine`: add `restore(bytes) => SimWorld`. `WasmWorld` gets a constructor path
  that takes an already-built `World` (refactor the current constructor into
  `new WasmWorld(world, memory)`, with `create` doing `new pkg.World(level)`).

## 4. Settings in localStorage

New `src/ui/persist.ts`:

- **`loadSettings(defaults): Settings`:** read `tectonics.settings`, keep only
  known keys whose `typeof` matches the default, and wrap it all in try/catch.
- **Not persisted:** `running` (always start paused) and `seedMode`.
- **`saveSettings(store)`:** subscribe and write with a 300 ms debounce.
- **Tuning and SimOptions:** under `tectonics.tuning`, store only the keys that differ
  from `defaultTuning` / `defaultSimOptions`. On load, `Object.assign` them onto
  `tuning`/`simOptions`.
  - Storing a diff means a default you change in code still takes effect, unless
    you've overridden that one key.
  - "Reset tuning to defaults" clears the key.
- **On resume, the stored tuning wins**, not the world's own. It is what the user
  last chose, it is loaded before the sliders are built (so they need no refresh),
  and `init` sends it to the worker, which applies it over the restored world and
  re-solves the surface. (The first draft of this plan had the world's tuning win,
  which would have needed a reply carrying it and a slider refresh.)

## 5. main.ts

- Build the store from `loadSettings(...)` instead of the bare `INITIAL_*` literals.
- The first `client.init` passes `resume: true`.
- On `resumed`: `settings.set({ level })`, guarded so it doesn't trigger a rebuild, and
  the seed overlay is taken down if the world has ticked.
- On `pagehide` and on `visibilitychange` to hidden: `client.save()`. This is best
  effort, since the tab may die before IndexedDB commits; the interval save is the
  reliable one.

## Out of scope

- File export/import of saves (easy to add later, since it's the same envelope).
- Multiple save slots.
- The TS `Plates`/bench paths beyond what resume needs.

## Risks

- **`back` or `boundaries` being read before written:** the round-trip test catches it.
- **Threaded build:** postcard runs on one thread, and `from_state` touches no rayon
  state. Check a reload in a cross-origin isolated page.
- **Large levels:** level 7 is ~40 MB raw. If that matters later, compress with
  `CompressionStream("gzip")` in the worker. It's not needed now.

## As built

- Rust: `State` in `world.rs` holds the persistent parts as `Cow`s, so saving borrows
  and loading owns. `PlateData.stress` became an `IndexMap`, because a `HashMap`'s
  iteration order differs between instances and made re-saved bytes differ.
- `build.rs` hashes `sim-rs/src` into `SIM_SOURCE_HASH`, which the header carries.
- The round-trip test compares re-saved bytes, which covers every persisted field at once.
- Checked in headless Chrome against both builds (10-thread and single-thread): reload
  mid-run gives the same plate count and crust mass, the resumed world keeps running,
  a slider change survives the reload, and a save with a corrupted hash is refused with
  one log line and replaced by a fresh world.
- Not restored: the crack walk's random stream, so seeds dropped after a resume grow
  differently than they would have in the original session.
