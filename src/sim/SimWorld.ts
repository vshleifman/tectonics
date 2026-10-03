import type { ArrayCellMesh } from "../mesh/ArrayCellMesh";
import type { PlateSummary } from "../plates/PlateData";
import type { WorldMetrics } from "./metrics";
import type { SimOptions, Tuning } from "./tuning";

/**
 * The passes of one tick, in the order they run. Each engine reports how long
 * the last tick spent in each, indexed the same way.
 */
export const PASS_NAMES = [
  "motion",
  "classify",
  "sutures",
  "advect",
  "identity",
  "rifting",
  "lifecycle",
  "elevation",
  "reclassify",
] as const;

/**
 * The per-cell and per-arc state a snapshot carries to the main thread. Views
 * may be invalidated by the next call into the engine, so copy before keeping.
 */
export interface WorldFields {
  elevation: Float32Array;
  plateId: Int32Array;
  crustType: Uint8Array;
  age: Float32Array;
  density: Float32Array;
  thickness: Float32Array;
  /** Cratonic keel per cell, in [0, 1]. */
  keel: Float32Array;
  cellKind: Uint8Array;
  arcKind: Int8Array;
}

export interface MassReadout {
  liveMass: number;
  /** Conservation drift as a fraction of the world's initial crust mass. */
  drift: number;
}

/**
 * One simulated world, whatever language it runs in. The worker and the
 * benchmark only ever talk to this, which is what lets the TypeScript and Rust
 * implementations be swapped and compared.
 */
export interface SimWorld {
  readonly mesh: ArrayCellMesh;
  readonly elapsedMyr: number;
  /**
   * Re-derive plates from the cracked-arc flags. `initCrust` seeds fresh crust
   * and resets the mass ledger, which is what building a world means.
   */
  reassignPlates(cracked: Uint8Array, initCrust: boolean): number;
  tick(): void;
  setTuning(tuning: Tuning): void;
  setSimOptions(options: SimOptions): void;
  fields(): WorldFields;
  plateCount(): number;
  /** Flat `[id, wx, wy, wz]` quads, one per live plate. */
  plateOmegas(): Float64Array;
  metrics(): WorldMetrics;
  summaries(): PlateSummary[];
  mass(): MassReadout;
  /** Milliseconds the last tick spent in each of {@link PASS_NAMES}. */
  passTimings(): Float64Array;
  dispose(): void;
}
