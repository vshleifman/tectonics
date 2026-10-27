/// <reference lib="webworker" />
import {
  type BenchSpec,
  describeBench,
  runBench,
} from "../bench/benchScenario";
import { copyMeshArrays } from "../mesh/ArrayCellMesh";
import { Plates } from "../plates/Plates";
import type { SimWorld } from "../sim/SimWorld";
import { type Engine, loadEngine } from "./engines";
import {
  bufferList,
  meshBufferList,
  type SimCommand,
  type SimReply,
  type Snapshot,
  type SnapshotBuffers,
} from "./protocol";
import { clearSave, readSave, type SavedWorld, writeSave } from "./saveStore";

/**
 * The simulation thread. Owns the whole world -- mesh, crust, plates, faults
 * and ledgers -- and publishes snapshots of it to the main thread, which never
 * touches simulation state directly.
 *
 * Snapshot buffers ping-pong: the worker fills a free set and transfers it, the
 * main thread transfers it back once it has a newer one. Three sets are enough
 * for one to be on screen, one in flight and one being filled. When the main
 * thread falls behind the worker keeps ticking and only publishes the latest
 * state once a set comes back.
 *
 * The world is also saved to IndexedDB, from here, so that a reload can carry
 * on from it: see {@link persist}.
 */

const ctx = self as unknown as DedicatedWorkerGlobalScope;

/** Snapshot buffer sets in circulation per world. */
const POOL_SIZE = 3;

/** Metrics and the plate list are refreshed at most this often (ms). */
const STATS_INTERVAL_MS = 500;

/** A changing world is saved at least this often while it runs (ms). */
const AUTOSAVE_MS = 10_000;

/** A change to a world that is not running is saved after this long (ms). */
const IDLE_SAVE_MS = 1_500;

let world: SimWorld | null = null;
let plates: Plates | null = null;
let generation = 0;
let level = 0;
let tick = 0;
let lastSimMs = 0;

let running = false;
let ticksPerSecond = 2;
let timer: ReturnType<typeof setTimeout> | null = null;

const pool: SnapshotBuffers[] = [];
/** A state change is waiting for a free buffer set. */
let pending = false;
let faultsChanged = false;
let statsDue = true;
let lastStats = 0;

/** The world has changed since it was last saved. */
let dirty = false;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

/** Commands are handled strictly in order, even across an async world build. */
let queue: Promise<void> = Promise.resolve();

const post = (reply: SimReply, transfer: Transferable[] = []): void => {
  ctx.postMessage(reply, transfer);
};

const allocateBuffers = (w: SimWorld): SnapshotBuffers => {
  const cells = w.mesh.cellCount;
  const arcs = w.mesh.boundaryGraph().arcCount;
  return {
    elevation: new Float32Array(cells),
    plateId: new Int32Array(cells),
    crustType: new Uint8Array(cells),
    age: new Float32Array(cells),
    density: new Float32Array(cells),
    thickness: new Float32Array(cells),
    keel: new Float32Array(cells),
    lithosphere: new Float32Array(cells),
    sediment: new Float32Array(cells),
    heatFlow: new Float32Array(cells),
    mantleTemp: new Float32Array(cells),
    lowerSlab: new Float32Array(cells),
    dynTopo: new Float32Array(cells),
    cellKind: new Uint8Array(cells),
    arcKind: new Int8Array(arcs),
  };
};

/** Send the current state if a buffer set is free, or remember to later. */
const publish = (): void => {
  if (!world || !plates) return;
  const buffers = pool.pop();
  if (!buffers) {
    pending = true;
    return;
  }
  pending = false;

  const fields = world.fields();
  buffers.elevation.set(fields.elevation);
  buffers.plateId.set(fields.plateId);
  buffers.crustType.set(fields.crustType);
  buffers.age.set(fields.age);
  buffers.density.set(fields.density);
  buffers.thickness.set(fields.thickness);
  buffers.keel.set(fields.keel);
  buffers.lithosphere.set(fields.lithosphere);
  buffers.sediment.set(fields.sediment);
  buffers.heatFlow.set(fields.heatFlow);
  buffers.mantleTemp.set(fields.mantleTemp);
  buffers.lowerSlab.set(fields.lowerSlab);
  buffers.dynTopo.set(fields.dynTopo);
  buffers.cellKind.set(fields.cellKind);
  buffers.arcKind.set(fields.arcKind);

  const now = performance.now();
  const withStats = statsDue || now - lastStats >= STATS_INTERVAL_MS;
  if (withStats) {
    lastStats = now;
    statsDue = false;
  }
  const plateOmegas = world.plateOmegas().slice();
  const snapshot: Snapshot = {
    generation,
    tick,
    elapsedMyr: world.elapsedMyr,
    plateCount: world.plateCount(),
    buffers,
    plateOmegas,
    mass: world.mass(),
    metrics: withStats ? world.metrics() : null,
    summaries: withStats ? world.summaries() : null,
    faults: faultsChanged
      ? { cracked: plates.cracked.slice(), seeds: [...plates.seeds] }
      : null,
    simMs: lastSimMs,
  };
  faultsChanged = false;
  const transfer: Transferable[] = [
    ...bufferList(buffers),
    plateOmegas.buffer as ArrayBuffer,
  ];
  if (snapshot.faults)
    transfer.push(snapshot.faults.cracked.buffer as ArrayBuffer);
  post({ type: "snapshot", snapshot }, transfer);
};

/**
 * Write the world to storage if it has changed. Serialising is synchronous and
 * takes a few tens of milliseconds at level 6; the write itself is not.
 */
const persist = async (): Promise<void> => {
  if (!dirty || !world || !plates) return;
  dirty = false;
  const stored = await writeSave({
    level,
    tick,
    cracked: plates.cracked.slice(),
    seeds: [...plates.seeds],
    sim: world.saveState(),
  });
  if (!stored) dirty = true;
};

/**
 * Note a change worth saving, and make sure a save is coming: soon if the
 * world is idle, otherwise on the autosave interval, so a running world is
 * written out once per interval however often it changes.
 */
const markDirty = (): void => {
  dirty = true;
  if (saveTimer !== null) return;
  saveTimer = setTimeout(
    () => {
      saveTimer = null;
      void persist();
    },
    running ? AUTOSAVE_MS : IDLE_SAVE_MS,
  );
};

const advance = (): void => {
  if (!world) return;
  const start = performance.now();
  world.tick();
  lastSimMs = performance.now() - start;
  tick++;
  markDirty();
};

const stopLoop = (): void => {
  if (timer !== null) clearTimeout(timer);
  timer = null;
};

/**
 * Self-rescheduling rather than a fixed interval, so a change of rate takes
 * effect on the next tick. A tick slower than the interval simply runs back to
 * back with the next one.
 */
const loop = (): void => {
  timer = null;
  if (!running || !world) return;
  const start = performance.now();
  advance();
  publish();
  const spent = performance.now() - start;
  timer = setTimeout(loop, Math.max(0, 1000 / ticksPerSecond - spent));
};

const reassign = (): void => {
  if (!world || !plates) return;
  world.reassignPlates(plates.cracked, false);
  markDirty();
  faultsChanged = true;
  statsDue = true;
  publish();
};

/**
 * Re-solve sea level and redraw after a tuning change, once however many
 * arrive together, so a dragged slider costs one refresh at a time.
 */
let refreshQueued = false;
const refreshSoon = (): void => {
  if (refreshQueued) return;
  refreshQueued = true;
  setTimeout(() => {
    refreshQueued = false;
    if (!world) return;
    world.refreshSurface();
    markDirty();
    statsDue = true;
    publish();
  }, 0);
};

/**
 * The saved world, if there is one this build can run. A save that cannot be
 * used (another build wrote it, or it is damaged) is deleted, so that it is
 * reported once and the world built in its place is saved over it.
 */
const resumeSaved = async (
  engine: Engine,
): Promise<{ world: SimWorld; saved: SavedWorld } | null> => {
  const saved = await readSave();
  if (!saved) return null;
  try {
    const restored = engine.restore(saved.sim);
    if (saved.cracked.length !== restored.mesh.boundaryGraph().arcCount) {
      restored.dispose();
      throw new Error("the fault network does not fit the mesh");
    }
    return { world: restored, saved };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    console.info(`Starting a new world; the saved one is unusable: ${reason}`);
    await clearSave();
    return null;
  }
};

const handle = async (cmd: SimCommand): Promise<void> => {
  switch (cmd.type) {
    case "init": {
      stopLoop();
      world?.dispose();
      world = null;
      const engine = await loadEngine();
      const resumed = cmd.resume ? await resumeSaved(engine) : null;
      const next = resumed?.world ?? engine.create(cmd.level);
      next.setTuning(cmd.tuning);
      next.setSimOptions(cmd.options);
      plates = new Plates(next.mesh);
      plates.setBranchesPerSeed(cmd.branches);
      if (resumed) {
        plates.restore(resumed.saved.cracked, resumed.saved.seeds);
        // The world was saved under whatever tuning it last ran on; the user's
        // may have moved since.
        next.refreshSurface();
      } else {
        next.reassignPlates(plates.cracked, true);
      }
      world = next;
      generation++;
      level = resumed?.saved.level ?? cmd.level;
      tick = resumed?.saved.tick ?? 0;
      lastSimMs = 0;
      pool.length = 0;
      for (let i = 0; i < POOL_SIZE; i++) pool.push(allocateBuffers(next));
      const mesh = copyMeshArrays(next.mesh.arrays);
      post(
        { type: "meshReady", generation, mesh, engine: engine.label },
        meshBufferList(mesh),
      );
      if (resumed) post({ type: "resumed", generation, level, tick });
      // A new world replaces the saved one; a resumed one is already saved.
      if (!resumed) markDirty();
      faultsChanged = true;
      statsDue = true;
      publish();
      if (running) loop();
      return;
    }
    case "addSeed":
      plates?.addSeed(cmd.junction);
      reassign();
      return;
    case "clearFaults":
      plates?.clear();
      reassign();
      return;
    case "setBranches":
      plates?.setBranchesPerSeed(cmd.count);
      return;
    case "setTuning":
      world?.setTuning(cmd.tuning);
      refreshSoon();
      return;
    case "setSimOptions":
      world?.setSimOptions(cmd.options);
      markDirty();
      return;
    case "run":
      ticksPerSecond = Math.max(0.1, cmd.ticksPerSecond);
      running = true;
      if (timer === null) loop();
      return;
    case "pause":
      running = false;
      stopLoop();
      await persist();
      return;
    case "save":
      await persist();
      return;
    case "step":
      advance();
      statsDue = true;
      publish();
      return;
    case "release":
      // Buffers from a world that has since been rebuilt are the wrong size.
      if (
        cmd.generation === generation &&
        world &&
        cmd.buffers.elevation.length === world.mesh.cellCount
      ) {
        pool.push(cmd.buffers);
        if (pending) publish();
      }
      return;
    case "bench":
      await bench(cmd.spec);
      return;
  }
};

const bench = async (spec: BenchSpec): Promise<void> => {
  const engine = await loadEngine();
  const result = runBench(engine.label, engine.create, spec);
  console.log(describeBench(result));
  post({ type: "benchResult", result });
};

ctx.onmessage = (event: MessageEvent<SimCommand>) => {
  const cmd = event.data;
  queue = queue
    .then(() => handle(cmd))
    .catch((error: unknown) => {
      console.error(error);
      post({
        type: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    });
};
