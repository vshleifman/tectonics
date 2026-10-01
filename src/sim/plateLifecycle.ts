import type { CellData } from "../data/CellData";
import { dot, normalise, sub, tangent } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { tuning } from "./tuning";

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

/**
 * Plate identity over deep time: plates are born and plates die, and the set of
 * live ids has to keep up with the crust.
 *
 * `plateId` rides along with the crust through advection, so the *set* of plates
 * changes underneath the id bookkeeping without anything announcing it. Two
 * things happen that nothing else accounts for:
 *
 * - **A plate is consumed.** Subduct the last of a plate's crust and the plate
 *   is gone. Left in `byId` it keeps a stale angular velocity, holds sutures
 *   open, and inflates the plate count — the world reads as having plates it no
 *   longer has.
 * - **A plate is pinched in two.** Consuming a narrow neck cuts a plate into
 *   separate pieces, and separate pieces are separate plates: they are no longer
 *   one rigid sheet and nothing makes them move together. This is how the
 *   Farallon plate ended: ground down against North America until what was left
 *   became the Juan de Fuca, Cocos and Rivera plates. Advection used to forbid
 *   this outright, keeping each plate's largest piece and handing the rest to
 *   whoever surrounded them, which made plate fragmentation impossible.
 *
 * Fragments below {@link Tuning.minPlateArea} are absorbed into a neighbour
 * rather than promoted: a handful of cells is a rounding artefact of the grid,
 * not a microplate, and giving each one an Euler pole of its own is how a world
 * fills up with noise.
 *
 * Buoyant crust arriving at a trench is handled here too, for the same reason:
 * it is a change of which plate a piece of crust belongs to, with the crust
 * itself staying exactly where it is. See {@link dockTerranes}.
 *
 * Pure over the mesh interface and flat typed arrays; mutates `data.plateId` and
 * the live plate set in place.
 */
export const updatePlateIdentity = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  allocateId: () => number,
): void => {
  dockTerranes(mesh, data, plateData);
  const { cellCount } = mesh;
  const component = new Int32Array(cellCount).fill(-1);
  const compPlate: number[] = [];
  const compArea: number[] = [];
  const compCells: number[][] = [];
  let totalArea = 0;
  for (let i = 0; i < cellCount; i++) totalArea += mesh.area(i);

  // Label the connected components of each plate.
  const queue: number[] = [];
  for (let i = 0; i < cellCount; i++) {
    if (component[i] >= 0 || data.plateId[i] < 0) continue;
    const plate = data.plateId[i];
    const comp = compPlate.length;
    compPlate.push(plate);
    const cells: number[] = [];
    let area = 0;
    queue.length = 0;
    queue.push(i);
    component[i] = comp;
    for (let head = 0; head < queue.length; head++) {
      const c = queue[head];
      cells.push(c);
      area += mesh.area(c);
      const nbrs = mesh.neighbors(c);
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (component[n] < 0 && data.plateId[n] === plate) {
          component[n] = comp;
          queue.push(n);
        }
      }
    }
    compArea.push(area);
    compCells.push(cells);
  }

  // The largest piece of each plate keeps its identity, so a plate that merely
  // sheds a sliver does not change its name.
  const largest = new Map<number, number>();
  for (let c = 0; c < compPlate.length; c++) {
    const best = largest.get(compPlate[c]);
    if (best === undefined || compArea[c] > compArea[best]) {
      largest.set(compPlate[c], c);
    }
  }

  const minArea = tuning.minPlateArea * totalArea;
  const absorb: number[] = [];
  for (let c = 0; c < compPlate.length; c++) {
    if (largest.get(compPlate[c]) === c) continue;
    if (compArea[c] < minArea) {
      absorb.push(c);
      continue;
    }
    // A genuine fragment: its own plate from here on, inheriting the motion it
    // had a moment ago as one sheet with its parent. Its own force balance takes
    // over from the next step.
    const id = allocateId();
    const parent = plateData.byId.get(compPlate[c]);
    plateData.byId.set(id, {
      omega: parent ? [...parent.omega] : [0, 0, 0],
      pending: 0,
    });
    for (const cell of compCells[c]) data.plateId[cell] = id;
  }

  // Slivers are handed to whichever neighbouring plate surrounds them, eroded
  // from the outside in so that a sliver several cells across is consumed even
  // though its interior touches nothing but itself. Only the label moves; the
  // crust stays where it is, which is what accreting a fragment amounts to.
  const dissolving = new Set(absorb);
  const frontier: number[] = [];
  for (const c of absorb) {
    for (const cell of compCells[c]) {
      const nbrs = mesh.neighbors(cell);
      for (let k = 0; k < nbrs.length; k++) {
        if (!dissolving.has(component[nbrs[k]])) {
          frontier.push(cell);
          break;
        }
      }
    }
  }
  for (let head = 0; head < frontier.length; head++) {
    const cell = frontier[head];
    if (!dissolving.has(component[cell])) continue; // already changed hands
    const nbrs = mesh.neighbors(cell);
    let taker = -1;
    for (let k = 0; k < nbrs.length; k++) {
      if (!dissolving.has(component[nbrs[k]]) && data.plateId[nbrs[k]] >= 0) {
        taker = nbrs[k];
        break;
      }
    }
    if (taker < 0) continue;
    data.plateId[cell] = data.plateId[taker];
    component[cell] = component[taker];
    for (let k = 0; k < nbrs.length; k++) {
      if (dissolving.has(component[nbrs[k]])) frontier.push(nbrs[k]);
    }
  }

  // Retire whatever is left holding no crust: plates consumed at trenches, and
  // plates whose every cell has just changed hands.
  const live = new Set<number>();
  for (let i = 0; i < cellCount; i++) {
    if (data.plateId[i] >= 0) live.add(data.plateId[i]);
  }
  for (const id of [...plateData.byId.keys()]) {
    if (!live.has(id)) plateData.byId.delete(id);
  }
  for (const key of [...plateData.sutures.keys()]) {
    const a = Math.floor(key / 1e6);
    const b = key % 1e6;
    if (!live.has(a) || !live.has(b)) plateData.sutures.delete(key);
  }
};

/**
 * Terrane accretion: buoyant crust cannot subduct, so it docks.
 *
 * An oceanic plate carrying an island arc, an oceanic plateau or a scrap of
 * continent subducts perfectly happily until that buoyant block reaches the
 * trench. The block cannot go down. What happens instead is that it is welded
 * onto the overriding plate and the subduction zone jumps to the far side of it,
 * where there is ordinary sea floor to consume again. It is how Taiwan is being
 * built now, how the Banda arc is colliding with Australia, and how most of the
 * western third of North America was assembled -- terrane after terrane docked
 * against the continent, each one arriving on a plate that then carried on
 * subducting behind it.
 *
 * Without this, buoyant crust arriving at a trench simply stops: advection
 * refuses to drag it under, the front stalls, and the block sits at the boundary
 * indefinitely instead of joining the continent it has run into.
 *
 * The test for "a block riding an oceanic plate" rather than "a continent in a
 * head-on collision" is what the crust around it is made of. A continental cell
 * whose own plate is oceanic where it meets the neighbour is a terrane, and it
 * docks; a continental cell backed by more of its own continent is one side of a
 * collision, and that is a suture, handled elsewhere.
 */
const dockTerranes = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
): void => {
  const { cellCount } = mesh;
  // Collected first and applied after, so that a block several cells across
  // docks as one piece rather than each cell changing the ground under the next.
  const docking: number[] = [];
  const onto: number[] = [];

  for (let i = 0; i < cellCount; i++) {
    if (data.crustType[i] !== CONTINENTAL) continue;
    const plate = data.plateId[i];
    if (plate < 0) continue;

    const nbrs = mesh.neighbors(i);
    let ownContinental = 0;
    let ownTotal = 0;
    let target = -1;
    let closing = 0;
    for (let k = 0; k < nbrs.length; k++) {
      const n = nbrs[k];
      const other = data.plateId[n];
      if (other === plate) {
        ownTotal++;
        if (data.crustType[n] === CONTINENTAL) ownContinental++;
        continue;
      }
      if (other < 0) continue;
      // Only somewhere the two are being driven together, and only against
      // crust that can hold this block up rather than sink beneath it.
      if (data.crustType[n] !== CONTINENTAL) continue;
      const rate = closingRate(mesh, data, plateData, i, n);
      if (rate > closing) {
        closing = rate;
        target = other;
      }
    }

    // A terrane is a buoyant block on an otherwise oceanic plate. Backed by its
    // own continent it is not a terrane, it is one half of a collision.
    if (target < 0 || closing <= tuning.activeSpeedEps) continue;
    if (ownTotal > 0 && ownContinental * 2 > ownTotal) continue;

    docking.push(i);
    onto.push(target);
  }

  for (let k = 0; k < docking.length; k++) {
    data.plateId[docking[k]] = onto[k];
  }
};

/**
 * How fast cells `a` and `b` are being driven together across the edge between
 * them: the relative surface velocity of their two plates, along the normal
 * pointing from `a` to `b`. Positive means closing.
 */
const closingRate = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  a: number,
  b: number,
): number => {
  const posA = mesh.position(a);
  const posB = mesh.position(b);
  const mid = normalise([
    posA[0] + posB[0],
    posA[1] + posB[1],
    posA[2] + posB[2],
  ]);
  const vA = plateData.linearVelocity(data.plateId[a], mid);
  const vB = plateData.linearVelocity(data.plateId[b], mid);
  const normal = normalise(tangent(sub(posB, posA), mid));
  return dot(sub(vA, vB), normal);
};
