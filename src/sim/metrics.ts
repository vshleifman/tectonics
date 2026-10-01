import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";
import { CM_PER_YR_PER_RAD_MYR, type PlateData } from "../plates/PlateData";
import { arcLength, type BoundaryField, BoundaryKind } from "./boundaries";
import type { AdvectionField } from "./massBudget";

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

/**
 * What Earth actually looks like, for the diagnostics panel to sit its numbers
 * next to. The point of this module is that "closer to real geology" is a claim
 * that can be checked rather than eyeballed, and these are the checks:
 *
 * - **Continental area.** Continental crust covers ~40% of the globe once the
 *   submerged margins are counted (dry land alone is ~29%).
 * - **Sea-floor age.** Ocean floor is young and none of it is old: the mean is
 *   ~60 Myr and the oldest surviving crust is ~180 Myr, because everything
 *   older has been subducted. The age-area distribution is close to triangular
 *   — most of the sea floor is young — which is the signature of steady
 *   creation at ridges balanced by consumption at trenches.
 * - **Plate count.** ~7 major plates plus a similar number of minor ones, and
 *   roughly steady over time: rifting mints new plates about as fast as
 *   subduction and boundary death retire them.
 * - **Speed against trench length.** The Forsyth & Uyeda (1975) result: a
 *   plate's speed tracks the fraction of its perimeter that is a subduction
 *   zone consuming *its own* crust, and barely correlates with its area. Slab
 *   pull is the dominant driving force, so this correlation is the sharpest
 *   single test of whether the force balance is behaving like Earth's.
 */
export const EARTH = {
  continentalArea: 0.4,
  meanOceanAge: 60,
  maxOceanAge: 180,
  majorPlates: 7,
  /**
   * Plates of every size. Earth's seven majors are joined by about as many
   * minor plates again, plus a long tail of microplates whose count depends
   * entirely on where the catalogue draws the line.
   */
  totalPlates: 15,
  speedTrenchCorrelation: 0.8,
  /**
   * How long the ocean floor takes to replace itself. Earth turns over its
   * entire sea floor in about the age of its oldest surviving crust, which is
   * the same statement twice: nothing older than ~180 Myr is left *because* the
   * turnover takes that long. A world whose recycling time is far longer is not
   * recycling, and its sea floor will age without limit however many ridges it
   * has.
   */
  oceanRecyclingMyr: 180,
} as const;

/**
 * Width (Myr) of each sea-floor age bin, and how many bins are kept. Exported
 * because the chart that draws the histogram has to bin identically.
 */
export const AGE_BIN_MYR = 20;
export const AGE_BINS = 10;

/** Plates below this share of the globe are excluded from the correlation. */
const MIN_CORRELATION_AREA = 0.01;

/**
 * Share of the surface a plate must cover to count as major. Earth's seven
 * majors all clear 2% comfortably; its largest minor plates sit just under.
 */
const MAJOR_PLATE_AREA = 0.02;

/** What drives one plate: how fast it goes, and how much slab it has to pull it. */
export interface PlateDrive {
  id: number;
  /** Surface speed at the plate's centroid (cm/yr). */
  speed: number;
  /** Share of the plate's boundary along which its OWN crust is subducting. */
  trenchFraction: number;
  /** Share of the globe's surface this plate covers. */
  areaShare: number;
}

/** A snapshot of how Earth-like the world currently is. See {@link EARTH}. */
export interface WorldMetrics {
  /** Simulated time since the world was built (Myr). */
  elapsedMyr: number;
  plateCount: number;
  /**
   * Plates covering at least {@link MAJOR_PLATE_AREA} of the surface. The
   * honest comparison against Earth's "seven major plates": a raw count is
   * dominated by however many slivers the grid happens to be carrying.
   */
  majorPlateCount: number;
  /** Share of the surface carrying continental crust, in [0, 1]. */
  continentalArea: number;
  /** Area-weighted mean age of oceanic crust (Myr). */
  meanOceanAge: number;
  /** Age of the oldest surviving oceanic crust (Myr). */
  maxOceanAge: number;
  /**
   * Myr for the present rate of subduction to consume the ocean floor once
   * over. `null` while nothing is subducting at all.
   */
  oceanRecyclingMyr: number | null;
  /**
   * Share of the ocean floor in each {@link AGE_BIN_MYR}-wide age bin, oldest
   * bin collecting everything beyond. Earth's is close to triangular.
   */
  ageHistogram: number[];
  /** Per-plate drive, largest plate first. */
  drives: PlateDrive[];
  /**
   * Share of total plate-boundary length by kind, in the order convergent,
   * divergent, transform, inactive. A closed sphere cannot diverge everywhere,
   * so a world with no convergence left in it has stopped recycling its crust —
   * which is the failure that shows up first and is hardest to see any other
   * way.
   */
  boundaryShare: [number, number, number, number];
  /**
   * Correlation between plate speed and the share of its perimeter that is a
   * trench consuming its own crust, over plates large enough to be meaningful.
   * `null` when there are too few such plates to correlate.
   */
  speedTrenchCorrelation: number | null;
}

/**
 * Measure the world against {@link EARTH}. Pure, read-only, and cheap enough to
 * run every tick: two passes over the cells and one over the boundary arcs.
 */
export const computeMetrics = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  boundaries: BoundaryField,
  advection: AdvectionField,
  dtMyr: number,
  elapsedMyr: number,
): WorldMetrics => {
  let totalArea = 0;
  let continentalArea = 0;
  let oceanArea = 0;
  let oceanAgeArea = 0;
  let maxOceanAge = 0;
  let oceanMass = 0;
  const ageArea = new Array<number>(AGE_BINS).fill(0);
  /** Area per live plate, for the area share and the centroid below. */
  const plateArea = new Map<number, number>();
  const centroidSum = new Map<number, [number, number, number]>();

  for (let i = 0; i < mesh.cellCount; i++) {
    const area = mesh.area(i);
    totalArea += area;
    if (data.crustType[i] === CONTINENTAL) {
      continentalArea += area;
    } else {
      const age = data.age[i];
      oceanArea += area;
      oceanAgeArea += age * area;
      if (age > maxOceanAge) maxOceanAge = age;
      const bin = Math.min(AGE_BINS - 1, Math.floor(age / AGE_BIN_MYR));
      ageArea[bin] += area;
      oceanMass += area * data.thickness[i];
    }

    const id = data.plateId[i];
    if (id < 0) continue;
    plateArea.set(id, (plateArea.get(id) ?? 0) + area);
    const pos = mesh.position(i);
    const sum = centroidSum.get(id);
    if (sum) {
      sum[0] += pos[0];
      sum[1] += pos[1];
      sum[2] += pos[2];
    } else {
      centroidSum.set(id, [pos[0], pos[1], pos[2]]);
    }
  }

  // Boundary pass: each plate's perimeter, and how much of it is a trench that
  // is eating that plate's own crust. Only the down-going side gets the credit,
  // because it is the slab hanging off that edge that pulls the plate along.
  const perimeter = new Map<number, number>();
  const trench = new Map<number, number>();
  const byKind: [number, number, number, number] = [0, 0, 0, 0];
  let boundaryLength = 0;
  const { arcCells, arcCount } = mesh.boundaryGraph();
  for (let a = 0; a < arcCount; a++) {
    const cellA = arcCells[a * 2];
    const cellB = arcCells[a * 2 + 1];
    if (cellA < 0 || cellB < 0) continue;
    const plateA = data.plateId[cellA];
    const plateB = data.plateId[cellB];
    if (plateA < 0 || plateB < 0 || plateA === plateB) continue;

    const len = arcLength(mesh, a);
    perimeter.set(plateA, (perimeter.get(plateA) ?? 0) + len);
    perimeter.set(plateB, (perimeter.get(plateB) ?? 0) + len);
    boundaryLength += len;
    const kind = boundaries.arcKind[a];
    byKind[kindIndex(kind)] += len;
    if (kind !== BoundaryKind.CONVERGENT) continue;

    // The side that goes down is the one that cannot stay up: continental crust
    // never subducts, and between two oceanic cells the denser one loses.
    const contA = data.crustType[cellA] === CONTINENTAL;
    const contB = data.crustType[cellB] === CONTINENTAL;
    if (contA && contB) continue; // a collision, not a subduction zone
    let down: number;
    if (contA) down = plateB;
    else if (contB) down = plateA;
    else down = data.density[cellA] >= data.density[cellB] ? plateA : plateB;
    trench.set(down, (trench.get(down) ?? 0) + len);
  }

  const drives: PlateDrive[] = [];
  for (const [id, area] of plateArea) {
    const sum = centroidSum.get(id) as [number, number, number];
    const norm = Math.hypot(sum[0], sum[1], sum[2]) || 1;
    const centroid: [number, number, number] = [
      sum[0] / norm,
      sum[1] / norm,
      sum[2] / norm,
    ];
    const v = plateData.linearVelocity(id, centroid);
    const edge = perimeter.get(id) ?? 0;
    drives.push({
      id,
      speed: Math.hypot(v[0], v[1], v[2]) * CM_PER_YR_PER_RAD_MYR,
      trenchFraction: edge > 0 ? (trench.get(id) ?? 0) / edge : 0,
      areaShare: totalArea > 0 ? area / totalArea : 0,
    });
  }
  drives.sort((a, b) => b.areaShare - a.areaShare);

  return {
    elapsedMyr,
    plateCount: plateData.count,
    majorPlateCount: drives.filter((d) => d.areaShare >= MAJOR_PLATE_AREA)
      .length,
    continentalArea: totalArea > 0 ? continentalArea / totalArea : 0,
    meanOceanAge: oceanArea > 0 ? oceanAgeArea / oceanArea : 0,
    maxOceanAge,
    oceanRecyclingMyr: recyclingTime(oceanMass, advection, dtMyr),
    ageHistogram: ageArea.map((a) => (oceanArea > 0 ? a / oceanArea : 0)),
    drives,
    boundaryShare: (boundaryLength > 0
      ? byKind.map((v) => v / boundaryLength)
      : byKind) as [number, number, number, number],
    speedTrenchCorrelation: correlate(
      drives.filter((d) => d.areaShare >= MIN_CORRELATION_AREA),
    ),
  };
};

/**
 * How long the present subduction rate would take to consume the ocean floor
 * once over. Read from the mass the last step actually destroyed rather than
 * from boundary lengths, so it counts crust that really went down.
 */
const recyclingTime = (
  oceanMass: number,
  advection: AdvectionField,
  dtMyr: number,
): number | null => {
  const perMyr = advection.subductedThisStep / Math.max(1e-9, dtMyr);
  return perMyr > 0 && oceanMass > 0 ? oceanMass / perMyr : null;
};

/** Slot in the boundary-composition tally for a boundary kind. */
const kindIndex = (kind: number): 0 | 1 | 2 | 3 => {
  switch (kind) {
    case BoundaryKind.CONVERGENT:
      return 0;
    case BoundaryKind.DIVERGENT:
      return 1;
    case BoundaryKind.TRANSFORM:
      return 2;
    default:
      return 3;
  }
};

/**
 * Pearson correlation of speed against trench fraction across plates. `null`
 * below three plates, or where either quantity has no spread at all (every
 * plate the same speed, or none of them subducting) — a correlation over that
 * is noise, not a reading.
 */
const correlate = (drives: readonly PlateDrive[]): number | null => {
  const n = drives.length;
  if (n < 3) return null;
  let sumX = 0;
  let sumY = 0;
  for (const d of drives) {
    sumX += d.trenchFraction;
    sumY += d.speed;
  }
  const meanX = sumX / n;
  const meanY = sumY / n;
  let sxy = 0;
  let sxx = 0;
  let syy = 0;
  for (const d of drives) {
    const dx = d.trenchFraction - meanX;
    const dy = d.speed - meanY;
    sxy += dx * dy;
    sxx += dx * dx;
    syy += dy * dy;
  }
  const denom = Math.sqrt(sxx * syy);
  return denom < 1e-12 ? null : sxy / denom;
};
