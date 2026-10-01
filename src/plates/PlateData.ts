import type { Vec3 } from "../math/vec3";
import { tuning } from "../sim/tuning";

/**
 * A plate boiled down to what a reader needs to identify it and see where it is
 * going: how much of the world it covers and how fast, and in which compass
 * direction, its surface is moving at its centroid. Derived by the simulation
 * from the live crust each time the plate list refreshes; nothing here is
 * stored.
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
 * Density bounds, derived live from {@link tuning} so the renderer's density
 * ramp stays in sync when the densities are tweaked. The continental (buoyant)
 * floor is the minimum; fully cooled oceanic crust is the maximum.
 */
export const densityMin = (): number => tuning.continentalDensity;
export const densityMax = (): number => tuning.oceanicDensityOld;
