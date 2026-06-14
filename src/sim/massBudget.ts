import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";

/**
 * Per-step scratch and outputs for conservative advection, allocated once per
 * world and refilled in place (mirrors {@link BoundaryField}).
 *
 * The scatter scheme moves crust forward from each source cell into the cell
 * its plate motion carries it to. `dest` and `winner` are scratch for that two
 * pass resolution; `riftCell` and `subducted` are the geological events that
 * elevation response (06) and crust lifecycle (07) consume; the scalars track
 * the conservation budget surfaced in the UI.
 *
 * Flat typed arrays throughout so advection stays a pure step that can later
 * move to a Worker / WASM.
 */
export class AdvectionField {
  /** Forward destination cell for each source cell (scratch). */
  readonly dest: Int32Array;

  /** Winning source cell claiming each destination, or -1 if unclaimed (scratch). */
  readonly winner: Int32Array;

  /** Extra thickness (km) accreted onto each destination this step (scratch). */
  readonly accreted: Float32Array;

  /** 1 where a divergent gap opened this step (a rift, consumed by 07). */
  readonly riftCell: Uint8Array;

  /** 1 where crust subducted into this destination this step (consumed by 06/07). */
  readonly subducted: Uint8Array;

  /** Total live crust mass after the step: sum of `area(i) * thickness[i]`. */
  liveMass = 0;

  /** Crust mass subducted this step. */
  subductedThisStep = 0;

  /** Cumulative crust mass subducted since the world was built. */
  subductedTotal = 0;

  constructor(cellCount: number) {
    this.dest = new Int32Array(cellCount);
    this.winner = new Int32Array(cellCount);
    this.accreted = new Float32Array(cellCount);
    this.riftCell = new Uint8Array(cellCount);
    this.subducted = new Uint8Array(cellCount);
  }
}

/** Total live crust mass: sum over cells of `area(i) * thickness[i]`. */
export const totalCrustMass = (mesh: CellMesh, data: CellData): number => {
  let mass = 0;
  for (let i = 0; i < mesh.cellCount; i++) {
    mass += mesh.area(i) * data.thickness[i];
  }
  return mass;
};
