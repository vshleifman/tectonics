import type { CellData } from "../data/CellData";
import { add, length, normalise, sub } from "../math/vec3";
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
 * There is a second, quieter way for two plates to become one, handled here as
 * well because it is the same bookkeeping: a boundary that stops moving stops
 * being a boundary (see {@link PlateData.quietContacts}). Welding is how two
 * continents are joined; going quiet is how two plates are found to have been
 * one for a while already.
 *
 * Three stages, all of which are things a real collision does:
 *
 * - **Welding.** Any continent-continent margin that is closing at all sutures.
 *   There is no minimum rate, because rate is not what decides it: continental
 *   crust is too buoyant to subduct, so once two continents are in contact and
 *   being driven together the margin has to lock however slowly it closes.
 *   Africa and Eurasia converge at about five millimetres a year in the western
 *   Mediterranean and are locked solid, building the Atlas and the Betics.
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
  /** Every adjacent plate pair, and the fastest relative motion across it. */
  const relativeSpeed = new Map<number, number>();

  for (let a = 0; a < arcCount; a++) {
    const cellA = arcCells[a * 2];
    const cellB = arcCells[a * 2 + 1];
    if (cellA < 0 || cellB < 0) continue;
    const plateA = data.plateId[cellA];
    const plateB = data.plateId[cellB];
    if (plateA < 0 || plateB < 0 || plateA === plateB) continue;

    // Every adjacent pair is tracked for going quiet, whatever it is made of.
    const pair = sutureKey(plateA, plateB);
    const mid = normalise(add(mesh.position(cellA), mesh.position(cellB)));
    const vA = plateData.linearVelocity(plateA, mid);
    const vB = plateData.linearVelocity(plateB, mid);
    const apart = length(sub(vA, vB));
    if (apart > (relativeSpeed.get(pair) ?? 0)) {
      relativeSpeed.set(pair, apart);
    }

    // Welding, from here on, is a continent-on-continent affair.
    if (
      data.crustType[cellA] !== CONTINENTAL ||
      data.crustType[cellB] !== CONTINENTAL
    ) {
      continue;
    }
    const key = pair;
    inContact.add(key);
    if (sutures.has(key)) {
      // Mark the seam itself. A suture is the weakest line in the continent it
      // joins, for as long as the continent lasts, and the next rift will look
      // for it: this is the memory that makes an ocean reopen roughly where the
      // last one closed.
      data.weakness[cellA] = Math.max(
        data.weakness[cellA],
        tuning.sutureWeakness,
      );
      data.weakness[cellB] = Math.max(
        data.weakness[cellB],
        tuning.sutureWeakness,
      );
    }
    if (boundaries.arcKind[a] !== BoundaryKind.CONVERGENT) continue;
    const closing = boundaries.arcClosing[a];
    if (closing > (hardest.get(key) ?? 0)) hardest.set(key, closing);
  }

  // A suture only survives while the two masses are still in contact; break the
  // contact and the weld is gone with it.
  for (const key of [...sutures.keys()]) {
    if (!inContact.has(key)) sutures.delete(key);
  }

  for (const key of inContact) {
    const existing = sutures.get(key);
    if (existing !== undefined) {
      // Already welded: it holds as long as the continents stay in contact,
      // whether or not they are still being driven together.
      sutures.set(key, existing + dtMyr);
    } else if ((hardest.get(key) ?? 0) > 0) {
      // Closing at all is enough. The threshold that used to be here was a
      // filter for numerical jitter wearing geology's clothes; the classifier
      // has already dropped anything below `activeSpeedEps` before this point.
      sutures.set(key, dtMyr);
    }
  }

  // --- Boundaries that have gone quiet --------------------------------------
  // Tracked for every adjacent pair, welded or not. A pair that stops moving
  // relative to one another is already one plate in all but name; a pair that
  // starts moving again resets the clock.
  const { quietContacts } = plateData;
  for (const key of [...quietContacts.keys()]) {
    if (!relativeSpeed.has(key)) quietContacts.delete(key); // no longer touching
  }
  for (const [key, apart] of relativeSpeed) {
    if (apart > tuning.mergeQuietSpeed) {
      quietContacts.delete(key);
    } else {
      quietContacts.set(key, (quietContacts.get(key) ?? 0) + dtMyr);
    }
  }

  // --- Merge plates that have been one plate for long enough ----------------
  // Two routes in, one outcome. A suture has to have held long enough *and* the
  // two halves have to actually be moving as one; a quiet boundary is the second
  // condition on its own, sustained for longer because nothing welded it.
  const merging: number[] = [];
  for (const [key, age] of sutures) {
    if (age < tuning.sutureMergeTime) continue;
    const a = plateData.byId.get(Math.floor(key / 1e6));
    const b = plateData.byId.get(key % 1e6);
    if (!a || !b) continue;
    const drift = length(sub(a.omega, b.omega));
    if (drift <= tuning.mergeQuietSpeed) merging.push(key);
  }
  for (const [key, age] of quietContacts) {
    if (age >= tuning.boundaryDeathTime) merging.push(key);
  }

  for (const key of merging) {
    const a = Math.floor(key / 1e6);
    const b = key % 1e6;
    if (!plateData.byId.has(a) || !plateData.byId.has(b)) continue;
    mergePlates(mesh, data, plateData, a, b);
  }
};

/**
 * Fold plate `b` into plate `a` (or the other way round -- the larger keeps its
 * identity, so the map's colours stay put).
 *
 * There is no size limit on the result. There used to be: a plate drove itself
 * by the density asymmetry of the crust it carried, that asymmetry cancelled as
 * a plate grew, and merging without a cap ended in a few vast frozen plates. The
 * force balance no longer works that way -- a plate is pulled by its slabs and
 * held back by the mantle under it -- so a large plate is slow for the reason a
 * large plate really is slow, and needs no cap to stop it. Africa and Eurasia
 * are enormous and barely move; the Pacific is enormous and does not.
 */
const mergePlates = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  a: number,
  b: number,
): void => {
  let countA = 0;
  let countB = 0;
  for (let i = 0; i < mesh.cellCount; i++) {
    if (data.plateId[i] === a) countA++;
    else if (data.plateId[i] === b) countB++;
  }
  const keep = countA >= countB ? a : b;
  const gone = keep === a ? b : a;
  for (let i = 0; i < mesh.cellCount; i++) {
    if (data.plateId[i] === gone) data.plateId[i] = keep;
  }
  plateData.byId.delete(gone);

  // Whatever the retired plate was joined to is now the survivor's business,
  // and the seam between the two is no longer a boundary at all.
  rehome(plateData.sutures, gone, keep);
  rehome(plateData.quietContacts, gone, keep);
};

/** Move every ledger entry naming `gone` onto `keep`, dropping their shared one. */
const rehome = (
  ledger: Map<number, number>,
  gone: number,
  keep: number,
): void => {
  for (const [key, age] of [...ledger]) {
    const x = Math.floor(key / 1e6);
    const y = key % 1e6;
    if (x !== gone && y !== gone) continue;
    ledger.delete(key);
    const partner = x === gone ? y : x;
    if (partner === keep) continue;
    const moved = sutureKey(keep, partner);
    ledger.set(moved, Math.max(ledger.get(moved) ?? 0, age));
  }
};
