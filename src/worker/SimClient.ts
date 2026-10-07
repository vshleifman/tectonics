import type { BenchResult, BenchSpec } from "../bench/benchScenario";
import type { Vec3 } from "../math/vec3";
import type { MeshArrays } from "../mesh/CellMesh";
import type { PlateVelocities } from "../render/CellRenderer";
import type { SimOptions, Tuning } from "../sim/tuning";
import {
  bufferList,
  type SimCommand,
  type SimReply,
  type Snapshot,
} from "./protocol";

/** Plate surface velocities from a snapshot's `[id, wx, wy, wz]` quads. */
export const platesFromOmegas = (omegas: Float64Array): PlateVelocities => {
  const byId = new Map<number, number>();
  for (let o = 0; o < omegas.length; o += 4) byId.set(omegas[o], o);
  return {
    linearVelocity: (id: number, p: Vec3): Vec3 => {
      const o = byId.get(id);
      if (o === undefined) return [0, 0, 0];
      const wx = omegas[o + 1];
      const wy = omegas[o + 2];
      const wz = omegas[o + 3];
      return [
        wy * p[2] - wz * p[1],
        wz * p[0] - wx * p[2],
        wx * p[1] - wy * p[0],
      ];
    },
  };
};

export interface SimClientHandlers {
  /**
   * A new world's mesh, sent once per rebuild before its first snapshot, with
   * the simulation build running it.
   */
  onMesh: (mesh: MeshArrays, generation: number, engine: string) => void;
  /** The latest world state; valid until the next snapshot arrives. */
  onSnapshot: (snapshot: Snapshot) => void;
  onBench?: (result: BenchResult) => void;
}

/**
 * The main thread's handle on the simulation worker. Turns UI intents into
 * protocol commands and keeps exactly one snapshot alive: when a newer one
 * arrives, the previous one's buffers go straight back to the worker.
 */
export class SimClient {
  private readonly worker: Worker;
  private latest: Snapshot | null = null;
  private generation = 0;

  constructor(private readonly handlers: SimClientHandlers) {
    this.worker = new Worker(new URL("./sim.worker.ts", import.meta.url), {
      type: "module",
    });
    this.worker.onmessage = (event: MessageEvent<SimReply>) =>
      this.receive(event.data);
  }

  /** The snapshot currently on screen, if any. */
  get snapshot(): Snapshot | null {
    return this.latest;
  }

  init(level: number, branches: number, tuning: Tuning, options: SimOptions) {
    this.send({
      type: "init",
      level,
      branches,
      tuning: { ...tuning },
      options: { ...options },
    });
  }

  addSeed(junction: number): void {
    this.send({ type: "addSeed", junction });
  }

  clearFaults(): void {
    this.send({ type: "clearFaults" });
  }

  setBranches(count: number): void {
    this.send({ type: "setBranches", count });
  }

  setTuning(tuning: Tuning): void {
    this.send({ type: "setTuning", tuning: { ...tuning } });
  }

  setSimOptions(options: SimOptions): void {
    this.send({ type: "setSimOptions", options: { ...options } });
  }

  run(ticksPerSecond: number): void {
    this.send({ type: "run", ticksPerSecond });
  }

  pause(): void {
    this.send({ type: "pause" });
  }

  step(): void {
    this.send({ type: "step" });
  }

  bench(spec: BenchSpec): void {
    this.send({ type: "bench", spec });
  }

  private send(cmd: SimCommand, transfer: Transferable[] = []): void {
    this.worker.postMessage(cmd, transfer);
  }

  private receive(reply: SimReply): void {
    switch (reply.type) {
      case "meshReady":
        this.generation = reply.generation;
        this.latest = null;
        this.handlers.onMesh(reply.mesh, reply.generation, reply.engine);
        return;
      case "snapshot": {
        const snapshot = reply.snapshot;
        // A snapshot from a world that has since been replaced is dropped; its
        // buffers are the wrong size for the new world.
        if (snapshot.generation !== this.generation) return;
        const previous = this.latest;
        this.latest = snapshot;
        this.handlers.onSnapshot(snapshot);
        if (previous) this.release(previous);
        return;
      }
      case "benchResult":
        this.handlers.onBench?.(reply.result);
        return;
      case "error":
        console.error(`Simulation worker: ${reply.message}`);
        return;
    }
  }

  private release(snapshot: Snapshot): void {
    const { buffers, generation } = snapshot;
    this.send({ type: "release", generation, buffers }, bufferList(buffers));
  }
}
