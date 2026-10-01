import type { CellData } from "../data/CellData";
import { clamp01 } from "../math/scalar";
import { dot, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { simOptions, tuning } from "./tuning";

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

/**
 * Half-width of the band, as a fraction of the plate's extent along the
 * extension axis, that a candidate rift line is scored over.
 */
const CUT_BAND = 0.06;

/** Bins the plate is sliced into along the extension axis when scanning for a cut. */
const SCAN_BINS = 64;

/**
 * Rifting: where new plates come from.
 *
 * Everything else in this simulation destroys plates — they are consumed at
 * trenches, pinched in two and ground away, welded to their neighbours. Left at
 * that a world only ever loses plates, and ends as two hemispheres with no
 * ridges and nowhere to make new crust. Earth holds a roughly steady number
 * because rifting mints new ones at about the rate the others are retired, and
 * this is that half of the cycle.
 *
 * A plate tears when the tension across it beats the strength of the weakest
 * line through it.
 *
 * - **Tension** comes from the force balance: slabs pulling from opposite edges
 *   stretch a plate rather than tow it (see `PlateStress`). This is why the
 *   plates that break up are the large ones with several subducting margins,
 *   and why a small plate towed by a single slab does not.
 * - **Strength** is lowered by two things. First, inherited weakness: sutures
 *   and old rifted margins never fully heal, so the next break follows the last
 *   one — the Atlantic opened along very nearly the line where the Appalachians
 *   and Caledonides had been raised closing the ocean before it. Second, and
 *   only when {@link SimOptions.supercontinentInsulation} is on, mantle heat
 *   trapped under a large continent, which is the usual explanation for why
 *   supercontinents break up rather than lasting.
 *
 * The tear itself runs perpendicular to the extension, positioned at whichever
 * line across the plate is weakest — which is what makes an old suture the place
 * it happens rather than merely a place it could.
 *
 * Pure over the mesh interface and flat typed arrays; mutates `data.plateId`,
 * `data.weakness` and the live plate set in place.
 */
export const updateRifting = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  dtMyr: number,
  allocateId: () => number,
): void => {
  ageWeakness(mesh, data, dtMyr);
  if (simOptions.supercontinentInsulation) updateMantleHeat(mesh, data, dtMyr);

  let totalArea = 0;
  const cellsOf = new Map<number, number[]>();
  const areaOf = new Map<number, number>();
  for (let i = 0; i < mesh.cellCount; i++) {
    const area = mesh.area(i);
    totalArea += area;
    const id = data.plateId[i];
    if (id < 0) continue;
    areaOf.set(id, (areaOf.get(id) ?? 0) + area);
    const cells = cellsOf.get(id);
    if (cells) cells.push(i);
    else cellsOf.set(id, [i]);
  }

  const minArea = tuning.minRiftArea * totalArea;
  // Every plate is a candidate, not only the ones under slab tension: a
  // supercontinent sitting over its own trapped heat has no opposed slabs at all,
  // and it is exactly the case that has to be able to break.
  for (const [id, cells] of cellsOf) {
    const plateArea = areaOf.get(id) ?? 0;
    // Only a plate with room to become two plates can rift; a microplate under
    // tension simply stays a microplate.
    if (plateArea < 2 * minArea) continue;

    const stress = plateData.stress.get(id);
    // With no slab tension there is no preferred direction from the forces, so
    // the heat under the plate picks the line instead.
    const axis = stress?.axis ?? hottestAxis(mesh, data, cells);
    const cut = weakestCut(mesh, data, cells, axis);
    if (!cut) continue;

    // What is pulling the plate apart, against what holds it together.
    const drive =
      (stress?.tension ?? 0) +
      (simOptions.supercontinentInsulation
        ? tuning.insulationDrive * cut.heat
        : 0);
    const strength =
      tuning.riftStrength *
      Math.max(
        0,
        1 -
          tuning.weaknessRelief * cut.weakness -
          tuning.insulationWeakening * cut.heat,
      );
    if (drive < strength) continue;

    splitPlate(
      mesh,
      data,
      plateData,
      id,
      cells,
      axis,
      cut.offset,
      allocateId,
      minArea,
    );
  }
};

/**
 * The axis to tear a plate along when its own forces suggest none: the direction
 * of the heat beneath it, so a plate splits away from its hottest ground. A dome
 * over an upwelling stretches the lithosphere above it, and the tear runs across
 * that stretch.
 */
const hottestAxis = (
  mesh: CellMesh,
  data: CellData,
  cells: readonly number[],
): Vec3 => {
  let x = 0;
  let y = 0;
  let z = 0;
  for (const cell of cells) {
    const w = data.mantleHeat[cell] * mesh.area(cell);
    const pos = mesh.position(cell);
    x += pos[0] * w;
    y += pos[1] * w;
    z += pos[2] * w;
  }
  const len = Math.hypot(x, y, z);
  // A plate with no heat at all has no preference; any axis will do, and the
  // weakness scan still picks the line.
  return len < 1e-12 ? [1, 0, 0] : [x / len, y / len, z / len];
};

/**
 * Weakness heals, but barely: a suture is still the softest line in a continent
 * hundreds of millions of years after the mountains above it have eroded flat.
 */
const ageWeakness = (mesh: CellMesh, data: CellData, dtMyr: number): void => {
  const keep = Math.exp(-dtMyr / Math.max(1e-6, tuning.weaknessDecayTime));
  for (let i = 0; i < mesh.cellCount; i++) data.weakness[i] *= keep;
};

/**
 * Mantle heat builds under continental crust and escapes under oceanic crust.
 *
 * The heat belongs to the mantle, which stays put while the plates slide over
 * it, so this is indexed by position and never advected with the crust: a
 * continent that drifts off its hot patch leaves the heat behind, and sits on
 * cold mantle at its new home.
 */
const updateMantleHeat = (
  mesh: CellMesh,
  data: CellData,
  dtMyr: number,
): void => {
  for (let i = 0; i < mesh.cellCount; i++) {
    const rate =
      data.crustType[i] === CONTINENTAL
        ? tuning.insulationRate
        : -tuning.insulationCooling;
    data.mantleHeat[i] = clamp01(data.mantleHeat[i] + rate * dtMyr);
  }
};

/** A candidate rift line: where it sits along the axis, and how soft it is. */
interface Cut {
  /** Position along the extension axis, as `dot(position, axis)`. */
  offset: number;
  /** Mean inherited weakness of the crust the line passes through, in [0, 1]. */
  weakness: number;
  /** Mean mantle heat under it, in [0, 1]. */
  heat: number;
}

/**
 * The softest line across the plate perpendicular to `axis`.
 *
 * The plate is stretched along `axis`, so it can only tear across it; what is
 * left to choose is *where*, and lithosphere tears where it is already broken.
 * Candidate lines are scanned across the plate's extent and scored by the mean
 * weakness and heat of the band they pass through. Lines too near either end are
 * skipped — a cut there shaves a sliver off rather than splitting the plate.
 */
const weakestCut = (
  mesh: CellMesh,
  data: CellData,
  cells: readonly number[],
  axis: Vec3,
): Cut | null => {
  let lo = Number.POSITIVE_INFINITY;
  let hi = Number.NEGATIVE_INFINITY;
  for (const cell of cells) {
    const t = dot(mesh.position(cell), axis);
    if (t < lo) lo = t;
    if (t > hi) hi = t;
  }
  const span = hi - lo;
  if (!Number.isFinite(span) || span <= 1e-6) return null;

  // Bin the plate along the axis in one pass, then score candidate lines off the
  // bins. Scoring each candidate by walking every cell instead is the same
  // answer for tens of times the work, on a plate that can be most of the world.
  const areaBin = new Float64Array(SCAN_BINS);
  const weakBin = new Float64Array(SCAN_BINS);
  const heatBin = new Float64Array(SCAN_BINS);
  for (const cell of cells) {
    const t = (dot(mesh.position(cell), axis) - lo) / span;
    const bin = Math.min(SCAN_BINS - 1, Math.max(0, Math.floor(t * SCAN_BINS)));
    const area = mesh.area(cell);
    areaBin[bin] += area;
    weakBin[bin] += data.weakness[cell] * area;
    heatBin[bin] += data.mantleHeat[cell] * area;
  }

  const half = Math.max(1, Math.round(CUT_BAND * SCAN_BINS));
  let best: Cut | null = null;
  let bestScore = -Infinity;
  // Keep clear of the ends, where a "split" is really a shaving.
  const first = Math.round(0.25 * SCAN_BINS);
  const last = Math.round(0.75 * SCAN_BINS);
  for (let centre = first; centre <= last; centre++) {
    let weight = 0;
    let weakness = 0;
    let heat = 0;
    for (
      let b = Math.max(0, centre - half);
      b <= Math.min(SCAN_BINS - 1, centre + half);
      b++
    ) {
      weight += areaBin[b];
      weakness += weakBin[b];
      heat += heatBin[b];
    }
    if (weight <= 0) continue;
    const score = weakness / weight;
    if (score > bestScore) {
      bestScore = score;
      best = {
        offset: lo + (span * (centre + 0.5)) / SCAN_BINS,
        weakness: score,
        heat: heat / weight,
      };
    }
  }
  return best;
};

/**
 * Cut a plate in two along the line at `offset`, giving the far side a new
 * stable id.
 *
 * Only the largest connected piece on the far side is taken, so a ragged plate
 * whose far side is in several pieces does not hand them all one id and call
 * them a rigid plate; whatever is left over is resolved by
 * `updatePlateIdentity` on the same tick. Both new margins are stamped as weak,
 * because a rifted margin is exactly the kind of line that tears again.
 */
const splitPlate = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  id: number,
  cells: readonly number[],
  axis: Vec3,
  offset: number,
  allocateId: () => number,
  minArea: number,
): void => {
  const far = new Set<number>();
  for (const cell of cells) {
    if (dot(mesh.position(cell), axis) > offset) far.add(cell);
  }
  if (far.size === 0) return;

  // Largest connected piece of the far side.
  const seen = new Set<number>();
  let best: number[] = [];
  let bestArea = 0;
  const queue: number[] = [];
  for (const start of far) {
    if (seen.has(start)) continue;
    queue.length = 0;
    queue.push(start);
    seen.add(start);
    const piece: number[] = [];
    let area = 0;
    for (let head = 0; head < queue.length; head++) {
      const cell = queue[head];
      piece.push(cell);
      area += mesh.area(cell);
      for (const n of mesh.neighbors(cell)) {
        if (far.has(n) && !seen.has(n)) {
          seen.add(n);
          queue.push(n);
        }
      }
    }
    if (area > bestArea) {
      bestArea = area;
      best = piece;
    }
  }
  if (bestArea < minArea) return;

  const newId = allocateId();
  const parent = plateData.byId.get(id);
  plateData.byId.set(newId, {
    omega: parent ? [...parent.omega] : [0, 0, 0],
    pending: 0,
  });
  for (const cell of best) data.plateId[cell] = newId;

  // Stamp the new margin on both sides: a rifted margin is a line that tears
  // again, and this is what carries the Wilson cycle forward.
  for (const cell of best) {
    for (const n of mesh.neighbors(cell)) {
      if (data.plateId[n] === newId) continue;
      data.weakness[cell] = Math.max(
        data.weakness[cell],
        tuning.riftMarginWeakness,
      );
      data.weakness[n] = Math.max(data.weakness[n], tuning.riftMarginWeakness);
    }
  }
};
