import { clamp01, lerp } from "../math/scalar";
import { densityMax, densityMin } from "../plates/PlateData";
import { BoundaryKind } from "../sim/boundaries";

/** An RGB colour with each channel in [0, 1]. */
export type Rgb = readonly [number, number, number];

/**
 * How cells are coloured: by elevation, flat (data view), by plate id, by crust
 * type (oceanic vs continental), by density, or by boundary type.
 */
export type ColourMode =
  | "elevation"
  | "data"
  | "plate"
  | "crust"
  | "density"
  | "boundary";

/** The per-cell fields the colour ramps read. */
export interface ColourFields {
  elevation: ArrayLike<number>;
  plateId: ArrayLike<number>;
  crustType: ArrayLike<number>;
  density: ArrayLike<number>;
  keel: ArrayLike<number>;
}

const WHITE: Rgb = [1, 1, 1];
const NO_PLATE: Rgb = [0.5, 0.5, 0.5];
const CONTINENTAL: Rgb = [0.82, 0.71, 0.48];
/** Cratons in the deep rose-brown geological maps give Archean shields. */
const CRATON: Rgb = [0.55, 0.3, 0.27];
const OCEANIC: Rgb = [0.13, 0.32, 0.55];
const DENSITY_LIGHT: Rgb = [0.95, 0.93, 0.82];
const DENSITY_DARK: Rgb = [0.35, 0.12, 0.18];

/** A `0xRRGGBB` literal as an `Rgb`. */
const hex = (v: number): Rgb => [
  ((v >> 16) & 0xff) / 255,
  ((v >> 8) & 0xff) / 255,
  (v & 0xff) / 255,
];

/** A colour ramp: ascending positions (km) paired with the colour at each. */
type Stops = readonly (readonly [number, Rgb])[];

/**
 * Hypsometric tints by height above sea level (km), after the atlas convention
 * Wikipedia's topographic maps follow: dark green lowland through pale green,
 * beige and tan to brown, with grey and white kept for the highest summits.
 * Bands are narrow near the coast and widen with height because most land is
 * low, and that is where small differences in height matter most.
 */
const LAND_STOPS: Stops = [
  [0.0, hex(0xacd0a5)],
  [0.05, hex(0x94bf8b)],
  [0.2, hex(0xa8c68f)],
  [0.5, hex(0xbdcc96)],
  [0.75, hex(0xd1d7ab)],
  [1.0, hex(0xe1e4b5)],
  [1.25, hex(0xefebc0)],
  [1.5, hex(0xe8e1b6)],
  [2.0, hex(0xded6a3)],
  [2.5, hex(0xd3ca9d)],
  [3.0, hex(0xcab982)],
  [3.5, hex(0xc3a76b)],
  [4.0, hex(0xb9985a)],
  [4.5, hex(0xaa8753)],
  [5.0, hex(0xac9a7c)],
  [5.5, hex(0xcac3b8)],
  [6.0, hex(0xf5f4f2)],
];

/**
 * Bathymetric tints by depth below sea level (km), in the GEBCO style: pale
 * cyan shelves darkening steadily to deep navy on the abyssal plain and in the
 * trenches, so ridges and trenches stay legible against the plain.
 */
const SEA_STOPS: Stops = [
  [0.0, hex(0xd4f0fc)],
  [0.2, hex(0xa8dcf0)],
  [0.5, hex(0x7fc5e6)],
  [1.0, hex(0x5aaedb)],
  [2.0, hex(0x3d8fc9)],
  [3.0, hex(0x2a72b3)],
  [4.0, hex(0x1d5899)],
  [5.0, hex(0x14427d)],
  [6.0, hex(0x0c2d5e)],
  [7.0, hex(0x071b40)],
];

/** Write `c` into `out` at `o` as RGBA8. */
const put = (out: Uint8Array, o: number, c: Rgb): void => {
  out[o] = Math.round(c[0] * 255);
  out[o + 1] = Math.round(c[1] * 255);
  out[o + 2] = Math.round(c[2] * 255);
  out[o + 3] = 255;
};

/** Write the blend of `a` and `b` by `t` into `out` at `o` as RGBA8. */
const putMix = (
  out: Uint8Array,
  o: number,
  a: Rgb,
  b: Rgb,
  t: number,
): void => {
  out[o] = Math.round(lerp(a[0], b[0], t) * 255);
  out[o + 1] = Math.round(lerp(a[1], b[1], t) * 255);
  out[o + 2] = Math.round(lerp(a[2], b[2], t) * 255);
  out[o + 3] = 255;
};

/**
 * Fill `out` (RGBA8, four bytes per cell) with every cell's colour for the
 * active mode. The crust view shades continent toward craton by keel. Allocation-free per cell: the ramps' anchors are resolved once
 * up front, and plate hues are memoised per id.
 */
export const writeCellColours = (
  out: Uint8Array,
  cellCount: number,
  data: ColourFields,
  mode: ColourMode,
  seaLevel: number,
  boundaryKind?: ArrayLike<number>,
): void => {
  switch (mode) {
    case "data":
      for (let i = 0; i < cellCount; i++) put(out, i * 4, WHITE);
      return;
    case "plate": {
      const cache = new Map<number, Rgb>();
      for (let i = 0; i < cellCount; i++) {
        const id = data.plateId[i];
        let c = cache.get(id);
        if (!c) {
          c = colourForPlate(id);
          cache.set(id, c);
        }
        put(out, i * 4, c);
      }
      return;
    }
    case "crust":
      for (let i = 0; i < cellCount; i++) {
        if (data.crustType[i] === 1) {
          putMix(out, i * 4, CONTINENTAL, CRATON, clamp01(data.keel[i]));
        } else {
          put(out, i * 4, OCEANIC);
        }
      }
      return;
    case "density": {
      const min = densityMin();
      const span = densityMax() - min || 1;
      for (let i = 0; i < cellCount; i++) {
        const t = clamp01((data.density[i] - min) / span);
        putMix(out, i * 4, DENSITY_LIGHT, DENSITY_DARK, t);
      }
      return;
    }
    case "boundary":
      for (let i = 0; i < cellCount; i++) {
        put(
          out,
          i * 4,
          colourForBoundary(boundaryKind ? boundaryKind[i] : BoundaryKind.NONE),
        );
      }
      return;
    default:
      writeElevationColours(out, cellCount, data.elevation, seaLevel);
  }
};

/**
 * Debug palette for the boundary view: interiors and inactive boundaries read
 * neutral grey, convergent red, divergent blue, transform yellow.
 */
const BOUNDARY_CONVERGENT: Rgb = [0.86, 0.21, 0.18];
const BOUNDARY_DIVERGENT: Rgb = [0.18, 0.45, 0.86];
const BOUNDARY_TRANSFORM: Rgb = [0.92, 0.82, 0.22];
const BOUNDARY_NONE: Rgb = [0.22, 0.24, 0.28];

export const colourForBoundary = (kind: number): Rgb => {
  switch (kind) {
    case BoundaryKind.CONVERGENT:
      return BOUNDARY_CONVERGENT;
    case BoundaryKind.DIVERGENT:
      return BOUNDARY_DIVERGENT;
    case BoundaryKind.TRANSFORM:
      return BOUNDARY_TRANSFORM;
    default:
      return BOUNDARY_NONE;
  }
};

/**
 * Write the colour at `x` along `stops` into `out` at `o`, interpolating
 * between neighbouring stops and holding the end colours beyond either end.
 */
const putRamp = (out: Uint8Array, o: number, stops: Stops, x: number): void => {
  if (x <= stops[0][0]) {
    put(out, o, stops[0][1]);
    return;
  }
  for (let s = 1; s < stops.length; s++) {
    const [hi, c1] = stops[s];
    if (x < hi) {
      const [lo, c0] = stops[s - 1];
      putMix(out, o, c0, c1, (x - lo) / (hi - lo));
      return;
    }
  }
  put(out, o, stops[stops.length - 1][1]);
};

/**
 * Map elevations (km) to colours with fixed hypsometric and bathymetric tints,
 * measured from `seaLevel` so that raising the sea floods lowland blue. The
 * tints are absolute, as on a printed map: a given height above sea level is
 * always the same colour, whatever the rest of the world looks like.
 */
const writeElevationColours = (
  out: Uint8Array,
  cellCount: number,
  elevation: ArrayLike<number>,
  seaLevel: number,
): void => {
  for (let i = 0; i < cellCount; i++) {
    const h = elevation[i] - seaLevel;
    if (h < 0) putRamp(out, i * 4, SEA_STOPS, -h);
    else putRamp(out, i * 4, LAND_STOPS, h);
  }
};

/** Golden-ratio hue stepping gives every plate a distinct, stable colour. */
export const colourForPlate = (plateId: number): Rgb => {
  if (plateId < 0) return NO_PLATE;
  const hue = (plateId * 0.6180339887498949) % 1;
  return hslToRgb(hue, 0.55, 0.55);
};

const hslToRgb = (h: number, s: number, l: number): Rgb => {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = h * 6;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let r = 0;
  let g = 0;
  let b = 0;
  if (hp < 1) [r, g, b] = [c, x, 0];
  else if (hp < 2) [r, g, b] = [x, c, 0];
  else if (hp < 3) [r, g, b] = [0, c, x];
  else if (hp < 4) [r, g, b] = [0, x, c];
  else if (hp < 5) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const m = l - c / 2;
  return [r + m, g + m, b + m];
};

/** An `Rgb` as a CSS colour, for DOM swatches that must match the 3D scene. */
export const toCssRgb = ([r, g, b]: Rgb): string =>
  `rgb(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)})`;
