import type { CellData } from "../data/CellData";
import { makeRng } from "../data/CellData";
import { clamp01 } from "../math/scalar";
import { cross, normalise, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import { baselineElevation } from "../sim/elevation";
import { tuning } from "../sim/tuning";
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

  /**
   * Displacement (radians) this plate has built up but not yet taken, so that
   * motion can be applied in whole cells.
   *
   * A plate is a rigid sheet: every part of it moves together. Resampling each
   * cell independently every tick instead lets some cells shift while their
   * neighbours do not, which smears a plate rather than moving it. So a plate
   * banks its motion here and, once it has earned a full cell width, shifts its
   * whole crust one cell in a single step.
   */
  pending: number;
}

/**
 * Key for an unordered pair of plate ids, so a suture is stored once however the
 * two plates are named.
 */
export const sutureKey = (a: number, b: number): number =>
  a < b ? a * 1e6 + b : b * 1e6 + a;

export class PlateData {
  /** Properties per live stable plate id. */
  readonly byId = new Map<number, PlateProperties>();

  /**
   * How long (Myr) each pair of plates has been welded together at a suture,
   * keyed by {@link sutureKey}. A continental collision that closes hard enough
   * stops being a boundary the two plates can pull apart at: the crust is joined
   * across it. Sutured plates are held to a common motion, and once they have
   * moved as one for long enough they stop being two plates at all.
   */
  readonly sutures = new Map<number, number>();

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

/**
 * Density bounds, derived live from {@link tuning} so the renderer's density
 * ramp stays in sync when the densities are tweaked. The continental (buoyant)
 * floor is the minimum; fully cooled oceanic crust is the maximum.
 */
export const densityMin = (): number => tuning.continentalDensity;
export const densityMax = (): number => tuning.oceanicDensityOld;

/**
 * Oceanic crust density (g/cm3) for a given age: young ridge crust starts light
 * and densifies monotonically toward `tuning.oceanicDensityOld` as it cools,
 * saturating at `tuning.maxOceanicAge`. The single density-vs-age curve shared
 * by initial seeding and the per-step ageing in the crust lifecycle.
 */
export const oceanicDensityFromAge = (age: number): number => {
  const t = clamp01(age / tuning.maxOceanicAge);
  return (
    tuning.oceanicDensityYoung +
    t * (tuning.oceanicDensityOld - tuning.oceanicDensityYoung)
  );
};

/**
 * Bring per-plate properties in line with a relabel.
 *
 * - Kept plates are left untouched, so a plate that survives a crack retains its
 *   Euler pole and speed (no flicker, stable colour).
 * - New plates that split off an existing plate inherit their parent's motion
 *   (a rifted fragment co-moves at first), then relax toward their own
 *   density-driven force balance (see `updatePlateMotion`).
 * - Genuinely new plates (first run, no parent) start at rest; their motion
 *   emerges from the crust they carry rather than from an RNG.
 * - Retired plates are dropped.
 *
 * Per-cell crust/age/density are NOT rewritten here: they advect with cells and
 * so persist across relabels (see {@link initialiseCrust}).
 */
export const reconcilePlateProperties = (
  plateData: PlateData,
  report: RelabelReport,
): void => {
  for (const id of report.retiredIds) plateData.byId.delete(id);

  for (const id of report.newIds) {
    const parent = report.parentOf.get(id) ?? -1;
    const parentProps = parent >= 0 ? plateData.byId.get(parent) : undefined;
    if (parentProps) {
      plateData.byId.set(id, {
        omega: [
          parentProps.omega[0],
          parentProps.omega[1],
          parentProps.omega[2],
        ],
        pending: 0,
      });
    } else {
      plateData.byId.set(id, { omega: [0, 0, 0], pending: 0 });
    }
  }
};

/**
 * Seed per-cell crust: every cell starts as young oceanic crust with an age-
 * driven density. Continental crust is not seeded — it emerges later through
 * accretion at convergent boundaries (future ticket).
 *
 * Run once when a world is built: thereafter crust advects with cells and must
 * persist (the crust "remembers its history"), so relabels deliberately leave
 * these arrays alone. The per-cell age spread (and thus the 2.9..3.0 density
 * spread) is what gives each plate a density gradient to drift along, so motion
 * is non-trivial from the very first step. Age is keyed per cell so it is stable
 * and reproducible.
 *
 * Elevation is seeded from the same age-driven isostatic baseline the elevation
 * response relaxes toward, so a fresh world reads as plausible ocean (deepening
 * with age) rather than random jitter, and boundary relief grows from there.
 */
export const initialiseCrust = (
  mesh: CellMesh,
  data: CellData,
  seed = "tectonics",
): void => {
  for (let i = 0; i < mesh.cellCount; i++) {
    const rng = makeRng(`${seed}:cell:${i}`);
    const age = rng() * tuning.maxOceanicAge;
    data.crustType[i] = 0; // oceanic
    data.age[i] = age;
    data.density[i] = oceanicDensityFromAge(age);
    data.thickness[i] = tuning.oceanicThickness;
    data.elevation[i] = baselineElevation(
      data.crustType[i],
      age,
      tuning.oceanicThickness,
    );
  }
};
