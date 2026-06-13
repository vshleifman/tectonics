import type { Vec3 } from "../math/vec3";

export type { Vec3 };

/**
 * The dual edge skeleton of the tiling: the trivalent wireframe that runs along
 * cell borders. Used to define faults — a crack is a set of cracked arcs, and
 * cracking an arc severs the link between the cell pair it separates.
 *
 * - "Junctions" are the cell corners (where three cells meet).
 * - "Arcs" are the boundary segments between two junctions; each arc separates
 *   exactly one pair of adjacent cells.
 *
 * Everything is flat typed arrays so it ports cleanly to a Web Worker / WASM.
 */
export interface BoundaryGraph {
  /** Number of junctions (cell corners). */
  readonly junctionCount: number;

  /** Junction positions on the unit sphere, flat xyz triples. */
  readonly junctionPos: Float32Array;

  /** Number of arcs (boundary segments). */
  readonly arcCount: number;

  /** Junction endpoints per arc, flat pairs (length = 2 * arcCount). */
  readonly arcEnds: Int32Array;

  /** Cell pair separated by each arc, flat pairs (length = 2 * arcCount). */
  readonly arcCells: Int32Array;

  /** Incident arcs per junction in CSR form (length = junctionCount + 1). */
  readonly junctionArcOffsets: Int32Array;

  /** Arc indices incident to each junction, sliced by junctionArcOffsets. */
  readonly junctionArcs: Int32Array;
}

/**
 * The single most important abstraction in the system.
 *
 * Downstream code (data, rendering, simulation) consumes ONLY this interface
 * and never references the concrete tiling. `IcosphereMesh` implements it now;
 * a `VoronoiMesh` can implement it later and the swap is a constructor change.
 *
 * A "cell" is just an index `i` in the range [0, cellCount).
 */
export interface CellMesh {
  /** Total number of cells. Indices are valid for [0, cellCount). */
  readonly cellCount: number;

  /** Adjacent cell indices for cell `i`. Precomputed once at build. */
  neighbors(i: number): readonly number[];

  /** Cell centre on the unit sphere. */
  position(i: number): Vec3;

  /** Corner vertices of the cell face, ordered around the centre. */
  polygon(i: number): readonly Vec3[];

  /** Cell area on the unit sphere (for later conservation maths). */
  area(i: number): number;

  /**
   * The dual edge skeleton (cell-border wireframe) used to define faults.
   * Precomputed once at build; safe to retain by reference.
   */
  boundaryGraph(): BoundaryGraph;

  /**
   * Arc index for each entry of the neighbour list, in the SAME CSR layout as
   * {@link neighbors}. Lets plate assignment ask "is the link to this
   * neighbour cracked?" in O(1) by indexing the cracked-arc array.
   */
  neighbourArcs(i: number): readonly number[];
}
