import { readVec3, type Vec3 } from "../math/vec3";
import type { BoundaryGraph, CellMesh, MeshArrays } from "./CellMesh";

/** A deep copy, for handing a mesh to another thread while keeping our own. */
export const copyMeshArrays = (m: MeshArrays): MeshArrays => ({
  cellCount: m.cellCount,
  centres: m.centres.slice(),
  areas: m.areas.slice(),
  neighbourOffsets: m.neighbourOffsets.slice(),
  neighbourList: m.neighbourList.slice(),
  neighbourArcList: m.neighbourArcList.slice(),
  polyOffsets: m.polyOffsets.slice(),
  polyCorners: m.polyCorners.slice(),
  junctionPos: m.junctionPos.slice(),
  arcEnds: m.arcEnds.slice(),
  arcCells: m.arcCells.slice(),
  junctionArcOffsets: m.junctionArcOffsets.slice(),
  junctionArcs: m.junctionArcs.slice(),
});

/**
 * A {@link CellMesh} over flat {@link MeshArrays}, however they were built. The
 * derived per-arc geometry is computed on first use, so a consumer that never
 * reads it (the worker's crack walk) never pays for it.
 */
export class ArrayCellMesh implements CellMesh {
  readonly cellCount: number;
  readonly centres: Float32Array;
  readonly areas: Float32Array;
  readonly neighbourOffsets: Int32Array;
  readonly neighbourList: Int32Array;
  readonly neighbourArcList: Int32Array;

  private readonly polyOffsets: Int32Array;
  private readonly polyCorners: Float32Array;
  private readonly boundary: BoundaryGraph;

  private arcMidCache: Float32Array | null = null;
  private arcNormalCache: Float32Array | null = null;

  constructor(readonly arrays: MeshArrays) {
    this.cellCount = arrays.cellCount;
    this.centres = arrays.centres;
    this.areas = arrays.areas;
    this.neighbourOffsets = arrays.neighbourOffsets;
    this.neighbourList = arrays.neighbourList;
    this.neighbourArcList = arrays.neighbourArcList;
    this.polyOffsets = arrays.polyOffsets;
    this.polyCorners = arrays.polyCorners;
    this.boundary = {
      junctionCount: arrays.junctionPos.length / 3,
      junctionPos: arrays.junctionPos,
      arcCount: arrays.arcEnds.length / 2,
      arcEnds: arrays.arcEnds,
      arcCells: arrays.arcCells,
      junctionArcOffsets: arrays.junctionArcOffsets,
      junctionArcs: arrays.junctionArcs,
    };
  }

  get arcMid(): Float32Array {
    if (!this.arcMidCache) this.buildArcGeometry();
    return this.arcMidCache as Float32Array;
  }

  get arcNormal(): Float32Array {
    if (!this.arcNormalCache) this.buildArcGeometry();
    return this.arcNormalCache as Float32Array;
  }

  /** Midpoint and A->B tangent normal of every arc, in one pass. */
  private buildArcGeometry(): void {
    const { arcCount, arcCells } = this.boundary;
    const c = this.centres;
    const mid = new Float32Array(arcCount * 3);
    const normal = new Float32Array(arcCount * 3);
    for (let a = 0; a < arcCount; a++) {
      const p = arcCells[a * 2] * 3;
      const q = arcCells[a * 2 + 1] * 3;
      let mx = c[p] + c[q];
      let my = c[p + 1] + c[q + 1];
      let mz = c[p + 2] + c[q + 2];
      const ml = Math.hypot(mx, my, mz) || 1;
      mx /= ml;
      my /= ml;
      mz /= ml;
      const dx = c[q] - c[p];
      const dy = c[q + 1] - c[p + 1];
      const dz = c[q + 2] - c[p + 2];
      const radial = dx * mx + dy * my + dz * mz;
      const tx = dx - radial * mx;
      const ty = dy - radial * my;
      const tz = dz - radial * mz;
      const tl = Math.hypot(tx, ty, tz) || 1;
      mid[a * 3] = mx;
      mid[a * 3 + 1] = my;
      mid[a * 3 + 2] = mz;
      normal[a * 3] = tx / tl;
      normal[a * 3 + 1] = ty / tl;
      normal[a * 3 + 2] = tz / tl;
    }
    this.arcMidCache = mid;
    this.arcNormalCache = normal;
  }

  neighbors(i: number): readonly number[] {
    return Array.from(
      this.neighbourList.subarray(
        this.neighbourOffsets[i],
        this.neighbourOffsets[i + 1],
      ),
    );
  }

  position(i: number): Vec3 {
    return readVec3(this.centres, i);
  }

  polygon(i: number): readonly Vec3[] {
    const out: Vec3[] = [];
    for (let k = this.polyOffsets[i]; k < this.polyOffsets[i + 1]; k++) {
      out.push(readVec3(this.polyCorners, k));
    }
    return out;
  }

  area(i: number): number {
    return this.areas[i];
  }

  boundaryGraph(): BoundaryGraph {
    return this.boundary;
  }

  neighbourArcs(i: number): readonly number[] {
    return Array.from(
      this.neighbourArcList.subarray(
        this.neighbourOffsets[i],
        this.neighbourOffsets[i + 1],
      ),
    );
  }
}
