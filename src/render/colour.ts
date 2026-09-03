import type { CellData } from "../data/CellData";
import { clamp01, lerp } from "../math/scalar";
import { densityMax, densityMin } from "../plates/PlateData";
import { BoundaryKind } from "../sim/boundaries";
import { tuning } from "../sim/tuning";

/** An RGB colour with each channel in [0, 1]. */
export type Rgb = [number, number, number];

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

/** Pick a cell's colour for the active mode. */
export const colourForCell = (
  data: CellData,
  mode: ColourMode,
  i: number,
  seaLevel: number,
  boundaryKind?: Uint8Array,
): Rgb => {
  switch (mode) {
    case "data":
      return [1, 1, 1];
    case "plate":
      return colourForPlate(data.plateId[i]);
    case "crust":
      return colourForCrust(data.crustType[i]);
    case "density":
      return colourForDensity(data.density[i]);
    case "boundary":
      return colourForBoundary(
        boundaryKind ? boundaryKind[i] : BoundaryKind.NONE,
      );
    default:
      return colourForElevation(data.elevation[i], seaLevel);
  }
};

/**
 * Debug palette for the boundary view: interiors and inactive boundaries read
 * neutral grey, convergent red, divergent blue, transform yellow.
 */
const colourForBoundary = (kind: number): Rgb => {
  switch (kind) {
    case BoundaryKind.CONVERGENT:
      return [0.86, 0.21, 0.18];
    case BoundaryKind.DIVERGENT:
      return [0.18, 0.45, 0.86];
    case BoundaryKind.TRANSFORM:
      return [0.92, 0.82, 0.22];
    default:
      return [0.22, 0.24, 0.28];
  }
};

/** Oceanic crust reads cool blue; continental crust reads warm tan. */
const colourForCrust = (crustType: number): Rgb =>
  crustType === 1 ? [0.82, 0.71, 0.48] : [0.13, 0.32, 0.55];

/** Light (least dense) to dark (most dense) ramp over the density range. */
const colourForDensity = (density: number): Rgb => {
  const min = densityMin();
  const max = densityMax();
  const t = clamp01((density - min) / (max - min || 1));
  return mix([0.95, 0.93, 0.82], [0.35, 0.12, 0.18], t);
};

/**
 * The highest ground the crust model can actually build: continental baseline
 * plus the uplift a maximally thick crustal column gives, using the very
 * constants {@link baselineElevation} works from. Deriving it rather than
 * hard-coding a range is the point -- the ramp and the elevations it colours
 * cannot drift apart when the tuning is changed.
 */
const landCeiling = (): number =>
  tuning.continentBase +
  Math.max(0, tuning.maxCrustThickness - tuning.continentThicknessRef) *
    tuning.continentThicknessGain;

/**
 * Map an elevation (km) to an RGB triple in [0, 1], with `seaLevel` as the
 * ocean/land boundary.
 *
 * Both ramps are anchored to the elevations the simulation actually produces,
 * not to the `elevationMin`/`elevationMax` safety clamp. Those are +-10 km while
 * the model spans roughly -6..+6, and within that the data is packed tighter
 * still: sea floor piles up at the abyssal baseline and land rarely passes 3 km.
 * Spread over the clamp, the ocean used about a third of its gradient and the
 * land a fifth -- so the whole map came out in two or three near-identical
 * shades and the relief was invisible.
 *
 * The ocean therefore breaks at the ridge crest: everything from the abyssal
 * plain up to the crest gets a full gradient of its own (that band alone is most
 * of the sea floor), and the shallow water above the crest gets another, so
 * shelves still read as distinct from ridges.
 */
const colourForElevation = (elevation: number, seaLevel: number): Rgb => {
  const abyss = Math.min(tuning.abyssElevation, seaLevel);
  const crest = Math.min(Math.max(tuning.ridgeElevation, abyss), seaLevel);
  const deepest: Rgb = [0.02, 0.05, 0.2];
  const midWater: Rgb = [0.07, 0.3, 0.5];
  const shallow: Rgb = [0.25, 0.62, 0.72];

  if (elevation < seaLevel) {
    if (elevation < crest) {
      // Abyssal plain up to the ridge crest: the bulk of the sea floor.
      const span = crest - abyss || 1;
      return mix(deepest, midWater, clamp01((elevation - abyss) / span));
    }
    // Crest up to sea level: ridges, shelves, drowned margins.
    const span = seaLevel - crest || 1;
    return mix(midWater, shallow, clamp01((elevation - crest) / span));
  }

  // Land: beach -> green -> brown -> snow, over the range the crust can build.
  const span = Math.max(landCeiling() - seaLevel, 0.5);
  const t = clamp01((elevation - seaLevel) / span);
  if (t < 0.15) return mix([0.78, 0.72, 0.5], [0.25, 0.5, 0.2], t / 0.15);
  if (t < 0.6)
    return mix([0.25, 0.5, 0.2], [0.45, 0.35, 0.22], (t - 0.15) / 0.45);
  return mix([0.45, 0.35, 0.22], [0.95, 0.95, 0.98], (t - 0.6) / 0.4);
};

/** Golden-ratio hue stepping gives every plate a distinct, stable colour. */
const colourForPlate = (plateId: number): Rgb => {
  if (plateId < 0) return [0.5, 0.5, 0.5];
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

/** Per-channel linear interpolation between two colours. */
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  lerp(a[0], b[0], t),
  lerp(a[1], b[1], t),
  lerp(a[2], b[2], t),
];
