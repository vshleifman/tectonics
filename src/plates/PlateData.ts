import type { CellData } from "../data/CellData";
import { makeRng } from "../data/CellData";
import { clamp01 } from "../math/scalar";
import { cross, normalise, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { RelabelReport } from "./PlateRegistry";

/**
 * Per-plate properties, keyed by *stable* plate id (see {@link PlateRegistry}).
 *
 * A `Map<stableId, …>` rather than a dense array because stable ids grow
 * monotonically and go sparse as plates split and merge; the plate count stays
 * small (tens), so the map is cheap while the per-cell hot arrays in `CellData`
 * remain flat and data-oriented.
 *
 * Motion is stored as an angular velocity `omega`: a vector whose direction is
 * the plate's Euler pole and whose magnitude is its spin rate (rad/Myr). This
 * is the only correct way to express rigid plate motion on a sphere — a flat
 * tangent "velocity" cannot stay consistent across an extended plate. Per-cell
 * linear velocity is `omega x position`, derived on demand and never stored.
 */
export interface PlateProperties {
  /** Angular velocity (rad/Myr); direction = Euler pole, magnitude = spin. */
  omega: [number, number, number];
  /** Whether the plate's crust is continental (buoyant) vs oceanic. */
  continental: boolean;
}

export class PlateData {
  /** Properties per live stable plate id. */
  readonly byId = new Map<number, PlateProperties>();

  /** Number of live plates. */
  get count(): number {
    return this.byId.size;
  }

  /**
   * Linear (tangent) velocity plate `id` imparts at world point `pos`: the
   * cross product `omega x pos`. For a point on the unit sphere this is a
   * vector tangent to the surface whose length is the local surface speed.
   * Returns the zero vector for an unknown id.
   */
  linearVelocity(id: number, pos: Vec3): Vec3 {
    const props = this.byId.get(id);
    if (!props) return [0, 0, 0];
    return cross(props.omega, pos);
  }

  /**
   * Unit-sphere centroid of every live plate, keyed by stable id. Computed by
   * averaging member-cell centres and renormalising; a good anchor for drawing
   * one velocity arrow per plate.
   */
  centroids(mesh: CellMesh, data: CellData): Map<number, Vec3> {
    const acc = new Map<number, [number, number, number]>();
    for (let i = 0; i < mesh.cellCount; i++) {
      const id = data.plateId[i];
      if (id < 0) continue;
      const pos = mesh.position(i);
      const sum = acc.get(id);
      if (sum) {
        sum[0] += pos[0];
        sum[1] += pos[1];
        sum[2] += pos[2];
      } else {
        acc.set(id, [pos[0], pos[1], pos[2]]);
      }
    }
    // Renormalise each accumulated sum back onto the unit sphere.
    const centroids = new Map<number, Vec3>();
    for (const [id, sum] of acc) centroids.set(id, normalise(sum));
    return centroids;
  }
}

/** Fraction of plates that start as (buoyant) continental crust. */
const CONTINENTAL_PROBABILITY = 0.35;

/** Angular speed range for a freshly seeded plate (rad/Myr). */
const MIN_SPEED = 0.002;
const MAX_SPEED = 0.02;

/**
 * How far a freshly rifted fragment's motion is jittered from its parent's, per
 * axis. Small, because a new fragment initially co-moves with the plate it broke
 * off (inherit-then-perturb): it shares history, then begins to diverge.
 */
const SPLIT_PERTURB = 0.15;

/** Oceanic crust ages out to here before being treated as fully cooled (Myr). */
const MAX_OCEANIC_AGE = 200;

/** Continental crust is ancient; its age is cosmetic (density is age-independent). */
const CONTINENTAL_AGE_MIN = 500;
const CONTINENTAL_AGE_MAX = 2000;

/** Densities (g/cm3): continental is buoyant; oceanic densifies as it cools. */
const CONTINENTAL_DENSITY = 2.7;
const OCEANIC_DENSITY_YOUNG = 2.9;
const OCEANIC_DENSITY_OLD = 3.0;

/**
 * Starting crust thickness (km): continental crust is far thicker than oceanic.
 * Thickness is the conserved lever for advection — see {@link CellData.thickness}.
 */
const CONTINENTAL_THICKNESS = 35;
const OCEANIC_THICKNESS = 7;

/** Density bounds, exported so the renderer's density ramp stays in sync. */
export const DENSITY_MIN = CONTINENTAL_DENSITY;
export const DENSITY_MAX = OCEANIC_DENSITY_OLD;

/**
 * Bring per-plate properties in line with a relabel.
 *
 * - Kept plates are left untouched, so a plate that survives a crack retains its
 *   Euler pole, speed and crust type (no flicker, stable colour).
 * - New plates inherit their parent's motion and crust then perturb it slightly
 *   when they split off an existing plate (a rifted fragment co-moves at first);
 *   genuinely new plates (first run, no parent) get fresh random values.
 * - Retired plates are dropped.
 *
 * Per-cell crust/age/density are NOT rewritten here: they advect with cells and
 * so persist across relabels (see {@link initialiseCrust}).
 */
export const reconcilePlateProperties = (
  plateData: PlateData,
  report: RelabelReport,
  seed = "tectonics",
): void => {
  for (const id of report.retiredIds) plateData.byId.delete(id);

  for (const id of report.newIds) {
    const parent = report.parentOf.get(id) ?? -1;
    const parentProps = parent >= 0 ? plateData.byId.get(parent) : undefined;
    const rng = makeRng(`${seed}:plate:${id}`);
    if (parentProps) {
      plateData.byId.set(id, {
        omega: perturbOmega(parentProps.omega, rng),
        continental: parentProps.continental,
      });
    } else {
      plateData.byId.set(id, freshPlate(rng));
    }
  }
};

/**
 * Seed per-cell crust type, age and derived density from each cell's plate.
 *
 * Run once when a world is built: thereafter crust advects with cells and must
 * persist (the crust "remembers its history"), so relabels deliberately leave
 * these arrays alone. Age is keyed per cell so it is stable and reproducible.
 */
export const initialiseCrust = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  seed = "tectonics",
): void => {
  for (let i = 0; i < mesh.cellCount; i++) {
    const id = data.plateId[i];
    const props = id >= 0 ? plateData.byId.get(id) : undefined;
    const isContinental = props?.continental ? 1 : 0;
    const rng = makeRng(`${seed}:cell:${i}`);
    data.crustType[i] = isContinental;
    if (isContinental) {
      data.age[i] =
        CONTINENTAL_AGE_MIN +
        rng() * (CONTINENTAL_AGE_MAX - CONTINENTAL_AGE_MIN);
      data.density[i] = CONTINENTAL_DENSITY;
      data.thickness[i] = CONTINENTAL_THICKNESS;
    } else {
      const age = rng() * MAX_OCEANIC_AGE;
      data.age[i] = age;
      const t = clamp01(age / MAX_OCEANIC_AGE);
      data.density[i] =
        OCEANIC_DENSITY_YOUNG +
        t * (OCEANIC_DENSITY_OLD - OCEANIC_DENSITY_YOUNG);
      data.thickness[i] = OCEANIC_THICKNESS;
    }
  }
};

/** A brand-new plate: random Euler pole, random speed, random crust type. */
const freshPlate = (rng: () => number): PlateProperties => {
  // Uniform random unit axis (the Euler pole).
  const z = rng() * 2 - 1;
  const phi = rng() * 2 * Math.PI;
  const r = Math.sqrt(Math.max(0, 1 - z * z));
  const speed = MIN_SPEED + rng() * (MAX_SPEED - MIN_SPEED);
  return {
    omega: [r * Math.cos(phi) * speed, r * Math.sin(phi) * speed, z * speed],
    continental: rng() < CONTINENTAL_PROBABILITY,
  };
};

/** Inherit a parent's angular velocity with a small per-axis jitter. */
const perturbOmega = (
  omega: readonly [number, number, number],
  rng: () => number,
): [number, number, number] => {
  const jitter = (): number => 1 + (rng() * 2 - 1) * SPLIT_PERTURB;
  return [omega[0] * jitter(), omega[1] * jitter(), omega[2] * jitter()];
};
