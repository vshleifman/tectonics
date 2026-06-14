import type { CellData } from "../data/CellData";
import { dot, normalise, sub, tangent, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";

/**
 * What kind of plate boundary an edge (or boundary cell) is, from the relative
 * velocity of the two plates across it. `NONE` covers both plate interiors and
 * inactive boundaries where the plates barely move relative to one another.
 */
export const BoundaryKind = {
  NONE: 0,
  CONVERGENT: 1,
  DIVERGENT: 2,
  TRANSFORM: 3,
} as const;

export type BoundaryKindValue =
  (typeof BoundaryKind)[keyof typeof BoundaryKind];

/**
 * Relative speed below which a boundary is treated as inactive (`NONE`). Keeps
 * near-co-moving plates from flickering between weak classifications.
 */
const ACTIVE_SPEED_EPS = 1e-5;

/**
 * Boundary classification for one mesh, in two synchronised representations:
 *
 * - Canonical **per-arc** (`arcKind`, `arcClosing`): the source of truth, since
 *   an arc separates exactly one cell pair. `arcClosing` is the signed normal
 *   relative speed (positive = closing, negative = opening), kept so downstream
 *   geology can scale ridge/trench/mountain magnitude by it.
 * - Derived **per-cell** (`cellKind`, `cellIntensity`): the dominant boundary
 *   kind touching each cell and its `|closing|` intensity, the cheap form the
 *   renderer and elevation response read.
 *
 * Flat typed arrays throughout, allocated once per world and refilled in place,
 * so classification stays a pure step that can later move to a Worker / WASM.
 */
export class BoundaryField {
  /** Boundary kind per arc, indexed by arc id. */
  readonly arcKind: Int8Array;

  /** Signed normal relative speed per arc (+ closing, - opening). */
  readonly arcClosing: Float32Array;

  /** Dominant boundary kind touching each cell. */
  readonly cellKind: Uint8Array;

  /** Intensity (max incident `|closing|`) of each cell's dominant boundary. */
  readonly cellIntensity: Float32Array;

  constructor(cellCount: number, arcCount: number) {
    this.arcKind = new Int8Array(arcCount);
    this.arcClosing = new Float32Array(arcCount);
    this.cellKind = new Uint8Array(cellCount);
    this.cellIntensity = new Float32Array(cellCount);
  }
}

/**
 * Classify every plate boundary on the mesh into the supplied {@link
 * BoundaryField} (refilled in place).
 *
 * An arc is a boundary when the two cells it separates belong to different
 * plates. The relative surface velocity `vRel = vA - vB` at the arc midpoint is
 * split into a component along the across-boundary normal `n` (cell A -> cell B)
 * and the remaining tangential component:
 *
 * - normal dominates and closing  -> convergent
 * - normal dominates and opening  -> divergent
 * - tangential dominates          -> transform (sliding)
 *
 * `|vn| >= |vt|` splits convergent/divergent from transform at ~45 deg. Pure
 * over the mesh interface and flat typed arrays.
 */
export const classifyBoundaries = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  out: BoundaryField,
): void => {
  const { arcKind, arcClosing, cellKind, cellIntensity } = out;
  const { arcCells, arcCount } = mesh.boundaryGraph();

  arcKind.fill(BoundaryKind.NONE);
  arcClosing.fill(0);
  cellKind.fill(BoundaryKind.NONE);
  cellIntensity.fill(0);

  for (let a = 0; a < arcCount; a++) {
    const cellA = arcCells[a * 2];
    const cellB = arcCells[a * 2 + 1];
    if (cellA < 0 || cellB < 0) continue;

    const plateA = data.plateId[cellA];
    const plateB = data.plateId[cellB];
    if (plateA < 0 || plateB < 0 || plateA === plateB) continue;

    const posA = mesh.position(cellA);
    const posB = mesh.position(cellB);
    const mid = normalise([
      posA[0] + posB[0],
      posA[1] + posB[1],
      posA[2] + posB[2],
    ]);

    const vA = plateData.linearVelocity(plateA, mid);
    const vB = plateData.linearVelocity(plateB, mid);
    const vRel: Vec3 = [vA[0] - vB[0], vA[1] - vB[1], vA[2] - vB[2]];

    // Across-boundary unit normal, from cell A toward cell B in the tangent
    // plane at the midpoint. `vn > 0` then means A approaches B (closing).
    const normal = normalise(tangent(sub(posB, posA), mid));
    const vn = dot(vRel, normal);
    const tx = vRel[0] - vn * normal[0];
    const ty = vRel[1] - vn * normal[1];
    const tz = vRel[2] - vn * normal[2];
    const vt = Math.hypot(tx, ty, tz);
    const speed = Math.hypot(vRel[0], vRel[1], vRel[2]);

    let kind: BoundaryKindValue = BoundaryKind.NONE;
    if (speed >= ACTIVE_SPEED_EPS) {
      if (Math.abs(vn) >= vt) {
        kind = vn > 0 ? BoundaryKind.CONVERGENT : BoundaryKind.DIVERGENT;
      } else {
        kind = BoundaryKind.TRANSFORM;
      }
    }

    arcKind[a] = kind;
    arcClosing[a] = vn;
    if (kind === BoundaryKind.NONE) continue;

    // The dominant boundary at a cell is the incident one with the strongest
    // normal speed, so the cell reads as its most active interaction.
    const intensity = Math.abs(vn);
    if (intensity > cellIntensity[cellA]) {
      cellIntensity[cellA] = intensity;
      cellKind[cellA] = kind;
    }
    if (intensity > cellIntensity[cellB]) {
      cellIntensity[cellB] = intensity;
      cellKind[cellB] = kind;
    }
  }
};
