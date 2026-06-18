import type { CellData } from "../data/CellData";
import { makeRng } from "../data/CellData";
import { clamp01 } from "../math/scalar";
import { cross, normalise, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import { baselineElevation } from "../sim/elevation";
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

/** Oceanic crust ages out to here before being treated as fully cooled (Myr). */
export const MAX_OCEANIC_AGE = 200;

/**
 * Densities (g/cm3). Crust starts oceanic and densifies as it cools; the
 * continental (buoyant) floor is reserved for crust that later differentiates
 * through accretion, and anchors the renderer's density ramp. Exported so the
 * crust lifecycle (08) seeds rift crust and differentiates continental crust
 * from the same constants the seeding uses.
 */
export const CONTINENTAL_DENSITY = 2.7;
export const OCEANIC_DENSITY_YOUNG = 2.9;
export const OCEANIC_DENSITY_OLD = 3.0;

/**
 * Starting oceanic crust thickness (km). Thickness is the conserved lever for
 * advection — see {@link CellData.thickness}. Exported so rift crust is minted
 * at the same thickness fresh crust is seeded with.
 */
export const OCEANIC_THICKNESS = 7;

/** Density bounds, exported so the renderer's density ramp stays in sync. */
export const DENSITY_MIN = CONTINENTAL_DENSITY;
export const DENSITY_MAX = OCEANIC_DENSITY_OLD;

/**
 * Oceanic crust density (g/cm3) for a given age: young ridge crust starts light
 * and densifies monotonically toward {@link OCEANIC_DENSITY_OLD} as it cools,
 * saturating at {@link MAX_OCEANIC_AGE}. The single density-vs-age curve shared
 * by initial seeding and the per-step ageing in the crust lifecycle.
 */
export const oceanicDensityFromAge = (age: number): number => {
  const t = clamp01(age / MAX_OCEANIC_AGE);
  return (
    OCEANIC_DENSITY_YOUNG + t * (OCEANIC_DENSITY_OLD - OCEANIC_DENSITY_YOUNG)
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
      });
    } else {
      plateData.byId.set(id, { omega: [0, 0, 0] });
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
    const age = rng() * MAX_OCEANIC_AGE;
    data.crustType[i] = 0; // oceanic
    data.age[i] = age;
    data.density[i] = oceanicDensityFromAge(age);
    data.thickness[i] = OCEANIC_THICKNESS;
    data.elevation[i] = baselineElevation(
      data.crustType[i],
      age,
      OCEANIC_THICKNESS,
    );
  }
};
