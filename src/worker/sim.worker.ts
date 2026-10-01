/// <reference lib="webworker" />
import {
  type BenchSpec,
  describeBench,
  runBench,
} from "../bench/benchScenario";
import { copyMeshArrays } from "../mesh/ArrayCellMesh";
import { Plates } from "../plates/Plates";
import type { SimWorld } from "../sim/SimWorld";
import { loadWorldFactory } from "./engines";
import {
  bufferList,
  meshBufferList,
  type SimCommand,
  type SimReply,
  type Snapshot,
  type SnapshotBuffers,
} from "./protocol";

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
 */

const ctx = self as unknown as DedicatedWorkerGlobalScope;

/** Snapshot buffer sets in circulation per world. */
const POOL_SIZE = 3;

/** Metrics and the plate list are refreshed at most this often (ms). */
const STATS_INTERVAL_MS = 500;

let world: SimWorld | null = null;
let plates: Plates | null = null;
let generation = 0;
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

const advance = (): void => {
  if (!world) return;
  const start = performance.now();
  world.tick();
  lastSimMs = performance.now() - start;
  tick++;
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
  faultsChanged = true;
  statsDue = true;
  publish();
};

const handle = async (cmd: SimCommand): Promise<void> => {
  switch (cmd.type) {
    case "init": {
      stopLoop();
      world?.dispose();
      world = null;
      const create = await loadWorldFactory();
      const next = create(cmd.level);
      next.setTuning(cmd.tuning);
      next.setSimOptions(cmd.options);
      plates = new Plates(next.mesh);
      plates.setBranchesPerSeed(cmd.branches);
      next.reassignPlates(plates.cracked, true);
      world = next;
      generation++;
      tick = 0;
      lastSimMs = 0;
      pool.length = 0;
      for (let i = 0; i < POOL_SIZE; i++) pool.push(allocateBuffers(next));
      const mesh = copyMeshArrays(next.mesh.arrays);
      post({ type: "meshReady", generation, mesh }, meshBufferList(mesh));
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
      return;
    case "setSimOptions":
      world?.setSimOptions(cmd.options);
      return;
    case "run":
      ticksPerSecond = Math.max(0.1, cmd.ticksPerSecond);
      running = true;
      if (timer === null) loop();
      return;
    case "pause":
      running = false;
      stopLoop();
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
  const create = await loadWorldFactory();
  const result = runBench("wasm", create, spec);
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
