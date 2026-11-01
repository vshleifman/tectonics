import type { Vec3 } from "../math/vec3";
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
  "erosion",
] as const;

/**
 * Metamorphic facies, in the order `facies` numbers them (sim-rs
 * `metamorphism::Facies`). The classifier lives in Rust; the view reads its
 * result.
 */
export const FACIES = [
  "None",
  "Zeolite",
  "Greenschist",
  "Amphibolite",
  "Granulite",
  "Blueschist",
  "Eclogite",
] as const;

/**
 * The resources whose potential the simulation derives, in the order its
 * `resources` buffer lays them out (sim-rs `resources::RESOURCE_NAMES`).
 * `WasmWorld` checks this against the simulation's own list, so the two cannot
 * drift apart unnoticed.
 */
export const RESOURCE_NAMES = [
  "Porphyry Cu-Mo-Au",
  "Epithermal Au-Ag",
  "VMS Cu-Zn",
  "Magmatic Ni-Cu-PGE",
  "Podiform chromite",
  "Orogenic gold",
  "Granite Sn-W-Li",
  "Kimberlite diamonds",
] as const;

/** `arcAge` of crust that has never had arc magmatism: the largest f32, as in Rust's `NEVER`. */
export const NEVER_ARC = 3.4028234663852886e38;

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
  /** Thickness (km) of the cold mantle lid beneath each cell. */
  lithosphere: Float32Array;
  /** Share of each cell's crust that is reworked sediment, in [0, 1]. */
  sediment: Float32Array;
  /** Share of each cell's crust that is mafic igneous rock (basalt, gabbro), in [0, 1]. */
  mafic: Float32Array;
  /** Share of each cell's crust that is felsic igneous rock (granitoid), in [0, 1]. */
  felsic: Float32Array;
  /** Share of each cell's crust that is loose sediment cover of felsic clastic rock, in [0, 1]. */
  coverFelsic: Float32Array;
  /** Share of each cell's crust that is loose sediment cover of mafic clastic rock, in [0, 1]. */
  coverMafic: Float32Array;
  /** Myr since each cell's crust last took in arc crust, or {@link NEVER_ARC}. */
  arcAge: Float32Array;
  /** Km of basement erosion has cut through at each cell: how deep the rock now at the surface once lay. */
  exhumed: Float32Array;
  /** The hottest (°C) the rock now at each cell's surface has been. */
  peakTemp: Float32Array;
  /** The greatest pressure (kbar) the rock now at each cell's surface has been under. */
  peakPressure: Float32Array;
  /** Metamorphic facies of that rock, as numbered in {@link FACIES}. */
  facies: Uint8Array;
  /**
   * Potential (0 to 1) of each resource at each cell, one cell-long run per
   * resource in the order of {@link RESOURCE_NAMES}.
   */
  resources: Float32Array;
  /** Heat leaving each cell (mW/m²). */
  heatFlow: Float32Array;
  /** Upper-mantle temperature beneath each cell (K), above or below the planet's mantle temperature. */
  mantleTemp: Float32Array;
  /** Slab piled in the lower mantle beneath each cell, as a share of the area over it. */
  lowerSlab: Float32Array;
  /** Dynamic topography (km) over each cell: how far the mantle's flow lifts or lowers the surface. */
  dynTopo: Float32Array;
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
  /**
   * Re-solve sea level and rewrite elevation from the current crust, taking
   * no time, so a change to the ocean volume shows while paused.
   */
  refreshSurface(): void;
  setSimOptions(options: SimOptions): void;
  /**
   * Hold a hand on the globe from the next tick on: plates near `centre` (a
   * unit vector) are drawn toward it at `speed` rad/Myr, or pushed away from
   * it if `speed` is negative, fading out at `radius` radians, on top of their
   * own forces. A hand that `grip`s also folds together the plates meeting
   * near its centre, if drawing them in, or rifts the plate under it there, if
   * pushing them apart.
   */
  setHand(centre: Vec3, radius: number, speed: number, grip: boolean): void;
  /** Let go of the hand; the plates go back to their own forces. */
  clearHand(): void;
  fields(): WorldFields;
  plateCount(): number;
  /** Flat `[id, wx, wy, wz]` quads, one per live plate. */
  plateOmegas(): Float64Array;
  metrics(): WorldMetrics;
  summaries(): PlateSummary[];
  mass(): MassReadout;
  /**
   * Everything that carries history, as bytes only the same build of the
   * simulation can read back (see {@link Engine.restore}).
   */
  saveState(): Uint8Array;
  /** Milliseconds the last tick spent in each of {@link PASS_NAMES}. */
  passTimings(): Float64Array;
  dispose(): void;
}
