import type { BenchResult, BenchSpec } from "../bench/benchScenario";
import type { MeshArrays } from "../mesh/CellMesh";
import type { PlateSummary } from "../plates/PlateData";
import type { WorldMetrics } from "../sim/metrics";
import type { MassReadout, WorldFields } from "../sim/SimWorld";
import type { SimOptions, Tuning } from "../sim/tuning";

/**
 * The language-independent boundary between the UI and the simulation. The
 * worker owns all world state; the main thread only ever sees snapshots of it,
 * so whatever runs behind this protocol is invisible here.
 */

export type SimCommand =
  | {
      type: "init";
      level: number;
      branches: number;
      tuning: Tuning;
      options: SimOptions;
    }
  | { type: "addSeed"; junction: number }
  | { type: "clearFaults" }
  | { type: "setBranches"; count: number }
  | { type: "setTuning"; tuning: Tuning }
  | { type: "setSimOptions"; options: SimOptions }
  | { type: "run"; ticksPerSecond: number }
  | { type: "pause" }
  | { type: "step" }
  /** Hand a snapshot's buffers back to the worker for reuse (ping-pong). */
  | { type: "release"; generation: number; buffers: SnapshotBuffers }
  | { type: "bench"; spec: BenchSpec };

/** The per-cell and per-arc fields of a snapshot, transferred not copied. */
export type SnapshotBuffers = WorldFields;

/** The fault network, sent only when it has changed. */
export interface FaultState {
  cracked: Uint8Array;
  seeds: number[];
}

export interface Snapshot {
  /** Which world this belongs to; bumped on every rebuild. */
  generation: number;
  tick: number;
  elapsedMyr: number;
  plateCount: number;
  buffers: SnapshotBuffers;
  /** Flat `[id, wx, wy, wz]` quads, one per live plate. */
  plateOmegas: Float64Array;
  mass: MassReadout;
  /** Throttled: `null` when unchanged since the last snapshot that had them. */
  metrics: WorldMetrics | null;
  summaries: PlateSummary[] | null;
  faults: FaultState | null;
  /** Wall-clock ms the last tick took in the worker. */
  simMs: number;
}

export type SimReply =
  | {
      type: "meshReady";
      generation: number;
      mesh: MeshArrays;
      /** The simulation build running the world, for display. */
      engine: string;
    }
  | { type: "snapshot"; snapshot: Snapshot }
  | { type: "benchResult"; result: BenchResult }
  | { type: "error"; message: string };

/** Every transferable buffer in a set of snapshot fields. */
export const bufferList = (b: SnapshotBuffers): ArrayBuffer[] => [
  b.elevation.buffer as ArrayBuffer,
  b.plateId.buffer as ArrayBuffer,
  b.crustType.buffer as ArrayBuffer,
  b.age.buffer as ArrayBuffer,
  b.density.buffer as ArrayBuffer,
  b.thickness.buffer as ArrayBuffer,
  b.keel.buffer as ArrayBuffer,
  b.lithosphere.buffer as ArrayBuffer,
  b.sediment.buffer as ArrayBuffer,
  b.heatFlow.buffer as ArrayBuffer,
  b.mantleTemp.buffer as ArrayBuffer,
  b.lowerSlab.buffer as ArrayBuffer,
  b.dynTopo.buffer as ArrayBuffer,
  b.cellKind.buffer as ArrayBuffer,
  b.arcKind.buffer as ArrayBuffer,
];

/** Every transferable buffer in a set of mesh arrays. */
export const meshBufferList = (m: MeshArrays): ArrayBuffer[] => [
  m.centres.buffer as ArrayBuffer,
  m.areas.buffer as ArrayBuffer,
  m.neighbourOffsets.buffer as ArrayBuffer,
  m.neighbourList.buffer as ArrayBuffer,
  m.neighbourArcList.buffer as ArrayBuffer,
  m.polyOffsets.buffer as ArrayBuffer,
  m.polyCorners.buffer as ArrayBuffer,
  m.junctionPos.buffer as ArrayBuffer,
  m.arcEnds.buffer as ArrayBuffer,
  m.arcCells.buffer as ArrayBuffer,
  m.junctionArcOffsets.buffer as ArrayBuffer,
  m.junctionArcs.buffer as ArrayBuffer,
];
