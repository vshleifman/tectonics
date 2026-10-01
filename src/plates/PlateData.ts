import type { CellData } from "../data/CellData";
import { makeRng } from "../data/CellData";
import { clamp01 } from "../math/scalar";
import {
  cross,
  dot,
  length,
  normalise,
  tangent,
  type Vec3,
} from "../math/vec3";
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

  /**
   * How long (Myr) each pair of neighbouring plates has been moving as one
   * without being welded, keyed by {@link sutureKey}.
   *
   * A plate boundary does not need a collision to stop being a boundary. Let the
   * relative motion across it fall to nothing and the two plates are simply one
   * plate with a line drawn on it. Spreading in the Labrador Sea stopped about
   * 33 million years ago; Greenland has moved with North America ever since, and
   * is no longer a plate of its own. Nothing collided and nothing welded — the
   * boundary just went quiet.
   */
  readonly quietContacts = new Map<number, number>();

  /**
   * How hard each plate is being pulled apart, refreshed every time the motion
   * is solved (see `updatePlateMotion`). Derived, not owned: it lives here only
   * so that rifting can read what the force balance already worked out instead
   * of walking every boundary a second time.
   */
  readonly stress = new Map<number, PlateStress>();

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

  /**
   * One {@link PlateSummary} per live plate, ordered by stable id, for the plate
   * list in the UI. Walks the cells once to gather each plate's centroid, cell
   * count and area, then samples its motion at that centroid.
   *
   * A plate with no cells left (possible for a moment between a topology change
   * and the next relabel) is omitted: there is nowhere to sample it.
   */
  summaries(mesh: CellMesh, data: CellData): PlateSummary[] {
    interface Accumulator {
      sum: [number, number, number];
      cellCount: number;
      area: number;
    }
    const acc = new Map<number, Accumulator>();
    let totalArea = 0;

    for (let i = 0; i < mesh.cellCount; i++) {
      const area = mesh.area(i);
      totalArea += area;
      const id = data.plateId[i];
      if (id < 0) continue;
      const pos = mesh.position(i);
      const entry = acc.get(id);
      if (entry) {
        entry.sum[0] += pos[0];
        entry.sum[1] += pos[1];
        entry.sum[2] += pos[2];
        entry.cellCount++;
        entry.area += area;
      } else {
        acc.set(id, {
          sum: [pos[0], pos[1], pos[2]],
          cellCount: 1,
          area,
        });
      }
    }

    const summaries: PlateSummary[] = [];
    for (const [id, entry] of acc) {
      const centroid = normalise(entry.sum);
      const velocity = this.linearVelocity(id, centroid);
      const speed = length(velocity);
      summaries.push({
        id,
        cellCount: entry.cellCount,
        areaShare: totalArea > 0 ? entry.area / totalArea : 0,
        centroid,
        speed,
        heading: speed > 1e-9 ? bearingAt(centroid, velocity) : null,
      });
    }
    summaries.sort((a, b) => a.id - b.id);
    return summaries;
  }
}

/**
 * The extensional state of one plate: how hard it is being pulled apart, and
 * along which line.
 *
 * A plate with a single slab hanging off one edge is simply towed along. A plate
 * with slabs pulling from opposite edges is being *stretched*, and stretched
 * lithosphere eventually tears — this is what breaks a large plate up, and it is
 * why the plates that break are the big ones with several subducting margins.
 */
export interface PlateStress {
  /**
   * Tension per unit area: how much of the pull on this plate is opposed pull
   * that cancels in the total rather than moving the plate. Zero for a plate
   * towed in one direction, whatever its speed.
   */
  tension: number;
  /** Unit axis along which the plate is being stretched. */
  axis: Vec3;
}

/**
 * A plate boiled down to what a reader needs to identify it and see where it is
 * going: how much of the world it covers and how fast, and in which compass
 * direction, its surface is moving at its centroid. Derived on demand from the
 * live crust; nothing here is stored.
 */
export interface PlateSummary {
  /** Stable plate id — the same id the per-cell data view labels cells with. */
  id: number;
  /** Cells currently belonging to this plate. */
  cellCount: number;
  /** Share of the globe's surface this plate covers, in [0, 1]. */
  areaShare: number;
  /** Unit-sphere centroid, where the speed and heading below are sampled. */
  centroid: Vec3;
  /** Surface speed at the centroid (rad/Myr). */
  speed: number;
  /**
   * Compass bearing of the motion at the centroid: degrees clockwise from north,
   * in [0, 360). `null` when the plate is effectively still (no direction to
   * report) or sits exactly on a pole, where "north" is undefined.
   */
  heading: number | null;
}

/**
 * Earth's radius (km). The simulation works in radians of arc per Myr, which is
 * scale-free; this is the one place a real planet size enters, so that speeds
 * can be shown in the cm/yr geologists actually quote.
 */
export const EARTH_RADIUS_KM = 6371;

/** One rad/Myr of surface motion at Earth's radius, expressed in cm/yr. */
export const CM_PER_YR_PER_RAD_MYR = (EARTH_RADIUS_KM * 1e5) / 1e6;

/**
 * Compass bearing (degrees clockwise from north, in [0, 360)) of a tangent
 * vector `v` at unit-sphere point `p`, or `null` at the poles where north is
 * undefined. `y` is the polar axis (see `projection.ts`).
 */
const bearingAt = (p: Vec3, v: Vec3): number | null => {
  const north = tangent([0, 1, 0], p);
  if (length(north) < 1e-6) return null; // on a pole: no meaningful bearing
  const n = normalise(north);
  const east = normalise(cross(p, [0, 1, 0]));
  const degrees = (Math.atan2(dot(v, east), dot(v, n)) * 180) / Math.PI;
  return (degrees + 360) % 360;
};

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
