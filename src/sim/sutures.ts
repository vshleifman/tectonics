import type { CellData } from "../data/CellData";
import { length } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import { type PlateData, sutureKey } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";
import { tuning } from "./tuning";

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

/**
 * Suturing: when two continents stop being two plates.
 *
 * A continental collision does not stay a boundary. The two masses weld along
 * the line where they met -- India and Asia are joined at the Indus suture, and
 * nothing there can pull them apart again. Without that, a collision is only
 * ever a contact: the boundary resists, the plates stall, whatever tiny relative
 * motion is left reads as divergence, and they drift apart again like repelling
 * magnets. Continents can never fuse, so a supercontinent can never assemble.
 *
 * Three stages, all of which are things a real collision does:
 *
 * - **Welding.** A continent-continent margin that is closing hard enough
 *   ({@link Tuning.sutureClosingFraction} of the maximum plate speed) sutures.
 *   A gentle touch does not: the crust has to be driven together.
 * - **Coupling.** Sutured plates are pulled toward a shared angular velocity, in
 *   proportion to how strong the suture is (see `updatePlateMotion`). They stop
 *   moving independently, which is what welded crust means, and the boundary
 *   stops being able to open.
 * - **Merging.** Once a suture has held for {@link Tuning.sutureMergeTime} and
 *   the two plates are genuinely moving as one, they are merged into a single
 *   plate. The seam remains in the crust -- the thickened orogen built while
 *   they collided -- but there is no longer a plate boundary there.
 */
export const updateSutures = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  boundaries: BoundaryField,
  dtMyr: number,
): void => {
  const { arcCells, arcCount } = mesh.boundaryGraph();
  const { sutures } = plateData;

  /** Pairs with continental crust on both sides of a shared arc this step. */
  const inContact = new Set<number>();
  /** Hardest closing rate seen along each such pair's margin. */
  const hardest = new Map<number, number>();

  for (let a = 0; a < arcCount; a++) {
    const cellA = arcCells[a * 2];
    const cellB = arcCells[a * 2 + 1];
    if (cellA < 0 || cellB < 0) continue;
    const plateA = data.plateId[cellA];
    const plateB = data.plateId[cellB];
    if (plateA < 0 || plateB < 0 || plateA === plateB) continue;
    if (
      data.crustType[cellA] !== CONTINENTAL ||
      data.crustType[cellB] !== CONTINENTAL
    ) {
      continue;
    }
    const key = sutureKey(plateA, plateB);
    inContact.add(key);
    if (boundaries.arcKind[a] !== BoundaryKind.CONVERGENT) continue;
    const closing = boundaries.arcClosing[a];
    if (closing > (hardest.get(key) ?? 0)) hardest.set(key, closing);
  }

  // A suture only survives while the two masses are still in contact; break the
  // contact and the weld is gone with it.
  for (const key of [...sutures.keys()]) {
    if (!inContact.has(key)) sutures.delete(key);
  }

  const needed = tuning.sutureClosingFraction * tuning.maxSpeed;
  for (const key of inContact) {
    const existing = sutures.get(key);
    if (existing !== undefined) {
      // Already welded: it holds as long as the continents stay in contact,
      // whether or not they are still being driven together.
      sutures.set(key, existing + dtMyr);
    } else if ((hardest.get(key) ?? 0) >= needed) {
      sutures.set(key, dtMyr);
    }
  }

  // --- Merge plates that have been one mass for long enough ----------------
  const tolerance = tuning.sutureSyncTolerance * tuning.maxSpeed;
  for (const [key, age] of [...sutures]) {
    if (age < tuning.sutureMergeTime) continue;
    const a = Math.floor(key / 1e6);
    const b = key % 1e6;
    const propsA = plateData.byId.get(a);
    const propsB = plateData.byId.get(b);
    if (!propsA || !propsB) {
      sutures.delete(key);
      continue;
    }
    // They must actually be moving as one, not merely touching.
    const drift = length([
      propsA.omega[0] - propsB.omega[0],
      propsA.omega[1] - propsB.omega[1],
      propsA.omega[2] - propsB.omega[2],
    ]);
    if (drift > tolerance) continue;

    // Keep the larger plate's identity so the map's colours stay put.
    let countA = 0;
    let countB = 0;
    let areaA = 0;
    let areaB = 0;
    let total = 0;
    for (let i = 0; i < mesh.cellCount; i++) {
      const area = mesh.area(i);
      total += area;
      if (data.plateId[i] === a) {
        countA++;
        areaA += area;
      } else if (data.plateId[i] === b) {
        countB++;
        areaB += area;
      }
    }
    // A plate drives itself by the density asymmetry of the crust it carries,
    // and that asymmetry cancels as a plate grows toward covering everything.
    // Past this size the two stay welded and travel as one, but they are not
    // collapsed into a single plate, or the world freezes.
    if (areaA + areaB > tuning.sutureMaxMergedArea * total) continue;
    const keep = countA >= countB ? a : b;
    const gone = keep === a ? b : a;
    for (let i = 0; i < mesh.cellCount; i++) {
      if (data.plateId[i] === gone) data.plateId[i] = keep;
    }
    plateData.byId.delete(gone);
    // Any suture the retired plate held is now the survivor's, and its own
    // seam is no longer a boundary at all.
    for (const [otherKey, otherAge] of [...sutures]) {
      const x = Math.floor(otherKey / 1e6);
      const y = otherKey % 1e6;
      if (x !== gone && y !== gone) continue;
      sutures.delete(otherKey);
      const partner = x === gone ? y : x;
      if (partner === keep) continue;
      const moved = sutureKey(keep, partner);
      sutures.set(moved, Math.max(sutures.get(moved) ?? 0, otherAge));
    }
  }
};
