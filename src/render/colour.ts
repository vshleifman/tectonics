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
  | "lithosphere"
  | "sediment"
  | "heat-flow"
  | "mantle-temp"
  | "slab-graveyard"
  | "dyn-topo"
  | "boundary";

/**
 * How the elevation view spends its colours. `"absolute"` is the atlas ramp,
 * ocean depth included. `"land"` paints everything at or below sea level in
 * one colour and stretches the continental colours across the low relief.
 */
export type ElevationStyle = "absolute" | "land";

/** The per-cell fields the colour ramps read. */
export interface ColourFields {
  elevation: ArrayLike<number>;
  plateId: ArrayLike<number>;
  crustType: ArrayLike<number>;
  density: ArrayLike<number>;
  keel: ArrayLike<number>;
  lithosphere: ArrayLike<number>;
  sediment: ArrayLike<number>;
  heatFlow: ArrayLike<number>;
  mantleTemp: ArrayLike<number>;
  lowerSlab: ArrayLike<number>;
  dynTopo: ArrayLike<number>;
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

/** Lid thickness (km): bare ridge through cooled ocean plate to deep keel. */
const LITHOSPHERE_STOPS: Stops = [
  [0, hex(0xfff3d0)],
  [30, hex(0xf2ca52)],
  [60, hex(0x2fb8a0)],
  [100, hex(0x2563a8)],
  [150, hex(0x3b2a7a)],
  [250, hex(0x120a2a)],
];

/** Share of the crust that is reworked sediment: bare crust to all silt. */
const SEDIMENT_STOPS: Stops = [
  [0, hex(0x2b2f3a)],
  [0.1, hex(0x6b5b3e)],
  [0.4, hex(0xc9a24d)],
  [1, hex(0xf6e7b4)],
];

/** Surface heat flow (mW/m²): cold old plates through hot ridge crust. */
const HEAT_FLOW_STOPS: Stops = [
  [30, hex(0x172554)],
  [48, hex(0x2563a8)],
  [65, hex(0x2fb8a0)],
  [100, hex(0xf2ca52)],
  [200, hex(0xd94b32)],
  [490, hex(0xfff3d0)],
];

/** Upper-mantle anomaly (K): chilled by slabs through neutral to hot under continents. */
const MANTLE_TEMP_STOPS: Stops = [
  [-200, hex(0x172554)],
  [-100, hex(0x2563a8)],
  [-30, hex(0x9cc9e8)],
  [0, hex(0xf2efe6)],
  [30, hex(0xf6c58a)],
  [100, hex(0xd94b32)],
  [200, hex(0x7a1020)],
];

/** Slab piled in the lower mantle (site areas of plate): bare mantle through deep cold graveyards. */
const LOWER_SLAB_STOPS: Stops = [
  [0, hex(0xf4efe6)],
  [3, hex(0xcfe0e8)],
  [6, hex(0x8fbcd9)],
  [10, hex(0x4a86c0)],
  [15, hex(0x2a4f96)],
  [25, hex(0x1b1546)],
];

/** Dynamic topography (km): dragged down over downwellings through neutral to lifted over hot mantle. */
const DYN_TOPO_STOPS: Stops = [
  [-1.5, hex(0x2d1e5c)],
  [-0.8, hex(0x5a5aa8)],
  [-0.25, hex(0xb4b8dc)],
  [0, hex(0xf2efe6)],
  [0.25, hex(0xe0d29a)],
  [0.8, hex(0xb7863a)],
  [1.5, hex(0x5e3a12)],
];

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

/**
 * One colour for every cell at or below sea level. Depth is not encoded, so
 * the scale can spend its whole range on the continents.
 */
const LAND_DETAIL_OCEAN: Rgb = hex(0x1a5276);

/**
 * Height above sea level (km), from the coast to about 8 km. The hue range is
 * wider than the atlas ramp, and the steps are a kilometre apart so low land
 * and high summits both stay separable. White is kept for the top of the scale.
 */
const LAND_DETAIL_STOPS: Stops = [
  [0.0, hex(0xc5e8c8)],
  [1.0, hex(0x43a047)],
  [2.0, hex(0x1b5e20)],
  [3.0, hex(0xc0ca33)],
  [4.0, hex(0xf9a825)],
  [5.0, hex(0xef6c00)],
  [6.0, hex(0xd84315)],
  [7.0, hex(0x8d6e63)],
  [8.0, hex(0xf5f4f2)],
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
  boundaryKind?: ArrayLike<number>,
  elevationStyle: ElevationStyle = "absolute",
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
    case "lithosphere":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, LITHOSPHERE_STOPS, data.lithosphere[i]);
      }
      return;
    case "sediment":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, SEDIMENT_STOPS, data.sediment[i]);
      }
      return;
    case "heat-flow":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, HEAT_FLOW_STOPS, data.heatFlow[i]);
      }
      return;
    case "mantle-temp":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, MANTLE_TEMP_STOPS, data.mantleTemp[i]);
      }
      return;
    case "slab-graveyard":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, LOWER_SLAB_STOPS, data.lowerSlab[i]);
      }
      return;
    case "dyn-topo":
      for (let i = 0; i < cellCount; i++) {
        putRamp(out, i * 4, DYN_TOPO_STOPS, data.dynTopo[i]);
      }
      return;
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
      writeElevationColours(
        out,
        cellCount,
        data.elevation,
        elevationStyle,
      );
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
 * already measured from sea level, which is 0. The
 * tints are absolute, as on a printed map: a given height above sea level is
 * always the same colour, whatever the rest of the world looks like.
 */
const writeElevationColours = (
  out: Uint8Array,
  cellCount: number,
  elevation: ArrayLike<number>,
  style: ElevationStyle,
): void => {
  for (let i = 0; i < cellCount; i++) {
    const h = elevation[i];
    if (style === "land") {
      if (h <= 0) put(out, i * 4, LAND_DETAIL_OCEAN);
      else putRamp(out, i * 4, LAND_DETAIL_STOPS, h);
      continue;
    }
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

/** One label along a gradient legend, placed at `at` in [0, 1] from the left. */
export interface LegendTick {
  at: number;
  label: string;
}

/** A discrete colour in a legend, matching a colour the renderer paints. */
export interface LegendSwatch {
  colour: string;
  label: string;
}

/** What the view panel draws under the view dropdown for the active mode. */
export interface ViewLegend {
  gradient: string | null;
  ticks: readonly LegendTick[];
  swatches: readonly LegendSwatch[];
  note: string;
}

const stopsToGradient = (colours: readonly Rgb[]): string => {
  const parts = colours.map((colour, index) => {
    const pct = colours.length === 1 ? 0 : (index / (colours.length - 1)) * 100;
    return `${toCssRgb(colour)} ${pct.toFixed(1)}%`;
  });
  return `linear-gradient(to right, ${parts.join(", ")})`;
};

const absoluteElevationLegend = (): ViewLegend => {
  const sea = [...SEA_STOPS].reverse().map((stop) => stop[1]);
  const land = LAND_STOPS.map((stop) => stop[1]);
  const colours = [...sea, ...land];
  const seaLevelAt = (sea.length - 1) / (colours.length - 1);
  return {
    gradient: stopsToGradient(colours),
    ticks: [
      { at: 0, label: "Deep" },
      { at: seaLevelAt, label: "Sea level" },
      { at: 1, label: "6 km" },
    ],
    swatches: [],
    note: "Height and depth, measured from sea level.",
  };
};

const landElevationLegend = (): ViewLegend => {
  const land = LAND_DETAIL_STOPS.map((stop) => stop[1]);
  const colours = [LAND_DETAIL_OCEAN, ...land];
  const atKm = (km: number): number => {
    const index = LAND_DETAIL_STOPS.findIndex((stop) => stop[0] >= km);
    const stopIndex = index < 0 ? LAND_DETAIL_STOPS.length - 1 : index;
    return (stopIndex + 1) / (colours.length - 1);
  };
  return {
    gradient: stopsToGradient(colours),
    ticks: [
      { at: 0, label: "Sea level" },
      { at: atKm(4), label: "4 km" },
      { at: 1, label: "8 km" },
    ],
    swatches: [],
    note: "Ocean is one colour. Continental colours are stretched so low land stays distinct.",
  };
};

/**
 * The colour key for a view mode, built from the same ramps the renderer uses.
 * `density` supplies the live tuning bounds, because that ramp moves with them.
 */
export const legendForView = (
  mode: ColourMode,
  style: ElevationStyle,
  density: { min: number; max: number },
): ViewLegend => {
  switch (mode) {
    case "elevation":
      return style === "land"
        ? landElevationLegend()
        : absoluteElevationLegend();
    case "data":
      return {
        gradient: null,
        ticks: [],
        swatches: [
          { colour: toCssRgb(WHITE), label: "Cell" },
          { colour: "rgb(8, 10, 14)", label: "Edge" },
        ],
        note: "Every cell is labelled with its properties.",
      };
    case "crust":
      return {
        gradient: null,
        ticks: [],
        swatches: [
          { colour: toCssRgb(OCEANIC), label: "Oceanic" },
          { colour: toCssRgb(CONTINENTAL), label: "Continental" },
          { colour: toCssRgb(CRATON), label: "Craton" },
        ],
        note: "Continental crust darkens toward craton as the keel thickens.",
      };
    case "density":
      return {
        gradient: stopsToGradient([DENSITY_LIGHT, DENSITY_DARK]),
        ticks: [
          { at: 0, label: density.min.toFixed(2) },
          { at: 1, label: density.max.toFixed(2) },
        ],
        swatches: [],
        note: "Crust density alone: light continental, dark oceanic. Whether a column sinks is up to its lid too (see the lithosphere view).",
      };
    case "lithosphere":
      return {
        gradient: stopsToGradient(LITHOSPHERE_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "0" },
          { at: 0.2, label: "30" },
          { at: 0.6, label: "100" },
          { at: 1, label: "250 km" },
        ],
        swatches: [],
        note: "Cold mantle lid under the crust: bare at ridges, thinned under rifts and plumes, deepest under cratonic keels.",
      };
    case "sediment":
      return {
        gradient: stopsToGradient(SEDIMENT_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "0" },
          { at: 1, label: "100%" },
        ],
        swatches: [],
        note: "Share of the crust, by mass, that eroded off land and was laid down again on a shelf, slope or basin.",
      };
    case "heat-flow":
      return {
        gradient: stopsToGradient(HEAT_FLOW_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "30" },
          { at: 0.35, label: "65" },
          { at: 1, label: "490 mW/m²" },
        ],
        swatches: [],
        note: "Surface heat loss: coolest old floor and keels to hottest young ridge crust.",
      };
    case "mantle-temp":
      return {
        gradient: stopsToGradient(MANTLE_TEMP_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "−200" },
          { at: 0.5, label: "0" },
          { at: 1, label: "+200 K" },
        ],
        swatches: [],
        note: "Upper mantle against the planet's mantle temperature: chilled where slabs sink, warmed under large continents.",
      };
    case "slab-graveyard":
      return {
        gradient: stopsToGradient(LOWER_SLAB_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "0" },
          { at: 0.6, label: "10" },
          { at: 1, label: "25 site areas" },
        ],
        swatches: [],
        note: "Slab piled on the core, sunk at old trenches and fading over hundreds of Myr: plumes rise at the edges of the piles.",
      };
    case "dyn-topo":
      return {
        gradient: stopsToGradient(DYN_TOPO_STOPS.map((stop) => stop[1])),
        ticks: [
          { at: 0, label: "−1.5" },
          { at: 0.5, label: "0" },
          { at: 1, label: "+1.5 km" },
        ],
        swatches: [],
        note: "How far the mantle's flow holds the surface up or down: lifted over hot upper mantle such as under a supercontinent, dragged down over sinking slabs.",
      };
    case "boundary":
      return {
        gradient: null,
        ticks: [],
        swatches: [
          { colour: toCssRgb(BOUNDARY_CONVERGENT), label: "Convergent" },
          { colour: toCssRgb(BOUNDARY_DIVERGENT), label: "Divergent" },
          { colour: toCssRgb(BOUNDARY_TRANSFORM), label: "Transform" },
          { colour: toCssRgb(BOUNDARY_NONE), label: "Interior" },
        ],
        note: "Each cell takes the colour of its dominant boundary.",
      };
    default:
      return { gradient: null, ticks: [], swatches: [], note: "" };
  }
};
