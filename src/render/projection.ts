/**
 * Maps points between the unit sphere and a flat Mercator map. The renderer,
 * labels and picker all consume these helpers so a single projection switch
 * re-places the entire scene without rebuilding any data.
 *
 * Convention (matching the icosphere mesh): y is up, so for a unit-sphere point
 * `lat = asin(y)` and `lon = atan2(z, x)`. The Mercator map lies on z = 0:
 * longitude maps to `[-1, 1]` and the clamped, flattened Mercator y to
 * `[-MAP_HALF_HEIGHT, MAP_HALF_HEIGHT]`, about half that, so the map is a
 * rectangle roughly twice as wide as it is tall.
 */

import { clamp } from "../math/scalar";

/** Which projection the scene is currently drawn in. */
export type Projection = "sphere" | "mercator";

/** Half-extent of the Mercator map in world units (the map spans -1..1 in x). */
export const MAP_HALF_EXTENT = 1;

/** Latitude clamp (~85.05 deg) so the poles don't stretch to infinity. */
export const MERCATOR_LAT_LIMIT = (85.05 * Math.PI) / 180;

/**
 * Divisor of latitude inside the Mercator stretch. True Mercator's 2 reaches
 * ~PI at the clamp and makes the map square; 2.5 flattens the high latitudes
 * so the map is about twice as wide as it is tall.
 */
const LAT_DIVISOR = 2.5;

/** Forward Mercator latitude stretch, flattened by {@link LAT_DIVISOR}. */
const mercatorY = (lat: number): number =>
  Math.log(Math.tan(Math.PI / 4 + lat / LAT_DIVISOR));

/** Half-height of the Mercator map in world units: where the clamp lands. */
export const MAP_HALF_HEIGHT = mercatorY(MERCATOR_LAT_LIMIT) / Math.PI;

/**
 * Shift `lon` by whole turns so it lands within PI of `refLon`. This is the
 * antimeridian seam fix: every point of a polygon is unwrapped relative to a
 * shared reference (the cell centre) so the polygon stays contiguous instead of
 * tearing across the whole map.
 */
export const unwrapLon = (lon: number, refLon: number): number => {
  let result = lon;
  while (result - refLon > Math.PI) result -= 2 * Math.PI;
  while (result - refLon < -Math.PI) result += 2 * Math.PI;
  return result;
};

/** Longitude of a unit-sphere point, for use as an unwrap reference. */
export const longitudeOf = (x: number, _y: number, z: number): number =>
  Math.atan2(z, x);

/**
 * Project a unit-sphere point into the active projection.
 *
 * - `lift` floats overlays above the faces: on the sphere it scales the radius
 *   (`1 + lift`); on the map it becomes a small +z offset toward the camera.
 * - `refLon` (mercator only) unwraps this point's longitude relative to a shared
 *   reference so multi-point shapes don't tear at the seam.
 */
export const projectPoint = (
  x: number,
  y: number,
  z: number,
  projection: Projection,
  lift = 0,
  refLon?: number,
): [number, number, number] => {
  if (projection === "sphere") {
    const r = 1 + lift;
    return [x * r, y * r, z * r];
  }
  let lon = Math.atan2(z, x);
  if (refLon !== undefined) lon = unwrapLon(lon, refLon);
  const lat = clamp(
    Math.asin(clamp(y, -1, 1)),
    -MERCATOR_LAT_LIMIT,
    MERCATOR_LAT_LIMIT,
  );
  return [lon / Math.PI, mercatorY(lat) / Math.PI, lift];
};

/**
 * Inverse of {@link projectPoint} for the Mercator map: a map point `(X, Y)`
 * back to a unit-sphere direction, used to pick junctions under the pointer.
 */
export const unprojectMercator = (
  X: number,
  Y: number,
): [number, number, number] => {
  const lon = X * Math.PI;
  const lat = LAT_DIVISOR * (Math.atan(Math.exp(Y * Math.PI)) - Math.PI / 4);
  const cosLat = Math.cos(lat);
  return [cosLat * Math.cos(lon), Math.sin(lat), cosLat * Math.sin(lon)];
};
