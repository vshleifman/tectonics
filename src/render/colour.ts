import type { CellData } from "../data/CellData";
import { clamp01, lerp } from "../math/scalar";
import { DENSITY_MAX, DENSITY_MIN } from "../plates/PlateData";

/** An RGB colour with each channel in [0, 1]. */
export type Rgb = [number, number, number];

/**
 * How cells are coloured: by elevation, flat (data view), by plate id, by crust
 * type (oceanic vs continental), or by density.
 */
export type ColourMode = "elevation" | "data" | "plate" | "crust" | "density";

const MIN_ELEVATION = -10;
const MAX_ELEVATION = 10;

/** Pick a cell's colour for the active mode. */
export const colourForCell = (
  data: CellData,
  mode: ColourMode,
  i: number,
  seaLevel: number,
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
    default:
      return colourForElevation(data.elevation[i], seaLevel);
  }
};

/** Oceanic crust reads cool blue; continental crust reads warm tan. */
const colourForCrust = (crustType: number): Rgb =>
  crustType === 1 ? [0.82, 0.71, 0.48] : [0.13, 0.32, 0.55];

/** Light (least dense) to dark (most dense) ramp over the density range. */
const colourForDensity = (density: number): Rgb => {
  const t = clamp01((density - DENSITY_MIN) / (DENSITY_MAX - DENSITY_MIN));
  return mix([0.95, 0.93, 0.82], [0.35, 0.12, 0.18], t);
};

/**
 * Map an elevation value (~-10..+10) to an RGB triple in [0, 1], with `seaLevel`
 * as the ocean/land boundary. Both the ocean and land ramps are normalised
 * relative to sea level so the full gradient is used at any threshold.
 */
const colourForElevation = (elevation: number, seaLevel: number): Rgb => {
  if (elevation < seaLevel) {
    // Ocean: deep navy (deepest) to shallow cyan (near sea level).
    const span = seaLevel - MIN_ELEVATION || 1;
    const t = clamp01((elevation - MIN_ELEVATION) / span);
    return mix([0.02, 0.05, 0.2], [0.1, 0.45, 0.6], t);
  }
  // Land: beach -> green -> brown -> snow, normalised above sea level.
  const span = MAX_ELEVATION - seaLevel || 1;
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
