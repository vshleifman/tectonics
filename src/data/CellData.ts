import type { CellMesh } from "../mesh/CellMesh";

/**
 * Data-oriented per-cell storage: flat typed arrays indexed by cell.
 *
 * There is deliberately no `Cell` class with methods — this layout iterates at
 * near-native speed, has zero GC pressure, serialises trivially, and is the
 * structure to keep even if hot loops later move to a Web Worker / WASM / GPU.
 */
export class CellData {
  /** Elevation per cell: freshly-cooled crust, ~-2..+2. */
  readonly elevation: Float32Array;

  /** Plate membership. Present now but unused (all 0) until the plates phase. */
  readonly plateId: Int32Array;

  /** Crust type per cell: 0 = oceanic, 1 = continental. */
  readonly crustType: Uint8Array;

  /** Crust age per cell, in millions of years (Myr). */
  readonly age: Float32Array;

  /** Crust density per cell (g/cm3), derived from crust type and age. */
  readonly density: Float32Array;

  /**
   * Crust thickness per cell (km). The conserved lever: crust mass at cell `i`
   * is `mesh.area(i) * thickness[i]`. Convergence thickens the surviving cell
   * (accretion); subduction removes the loser's mass to a ledger.
   */
  readonly thickness: Float32Array;

  /**
   * How much weaker than intact lithosphere this cell is, in [0, 1].
   *
   * Lithosphere remembers where it has been broken. A suture -- the seam where
   * two continents were welded together -- and the rifted margin left behind
   * when a continent split are both permanently weaker than the crust around
   * them, and both stay weak for billions of years. So the next rift does not
   * open somewhere new: it reopens an old line. The Atlantic tore Pangaea apart
   * along very nearly the suture where the Appalachians and Caledonides had been
   * built when the previous ocean closed. This field is what carries that memory
   * from one Wilson cycle to the next.
   */
  readonly weakness: Float32Array;

  /**
   * Heat accumulated in the mantle beneath this cell, in [0, 1].
   *
   * Continental crust is a blanket: it conducts heat poorly, so mantle under a
   * large continent cannot shed its heat and warms until the lithosphere above
   * is weak enough to tear. This is the leading explanation for why
   * supercontinents do not last -- Pangaea assembled and then broke up, and so
   * did Rodinia and Columbia before it. Oceanic crust is thin and lets heat out,
   * so the field cools wherever the continent is gone.
   *
   * Only maintained while {@link SimOptions.supercontinentInsulation} is on;
   * with it off the field stays zero and costs nothing.
   */
  readonly mantleHeat: Float32Array;

  constructor(cellCount: number) {
    this.elevation = new Float32Array(cellCount);
    this.plateId = new Int32Array(cellCount);
    this.crustType = new Uint8Array(cellCount);
    this.age = new Float32Array(cellCount);
    this.density = new Float32Array(cellCount);
    this.thickness = new Float32Array(cellCount);
    this.weakness = new Float32Array(cellCount);
    this.mantleHeat = new Float32Array(cellCount);
  }
}

/** Freshly-cooled crust: mostly uniform, with faint variation within -2..+2. */
const ELEVATION_VARIATION = 2;

/**
 * Seed a near-uniform starting elevation. The planet begins as a freshly-cooled
 * crust: every cell sits close to sea level with only slight low-amplitude
 * jitter so faces remain faintly distinct. Plate interactions and simulated
 * geomorphology will produce real relief later.
 */
export const seedUniformElevation = (
  mesh: CellMesh,
  data: CellData,
  seed = "tectonics",
): void => {
  const rng = makeRng(seed);
  for (let i = 0; i < mesh.cellCount; i++) {
    // Centred on 0 with a small symmetric jitter, kept inside [-2, +2].
    data.elevation[i] = (rng() * 2 - 1) * ELEVATION_VARIATION;
  }
};

/** Tiny deterministic PRNG (mulberry32) seeded from a string. */
export const makeRng = (seed: string): (() => number) => {
  let h = 1779033703 ^ seed.length;
  for (let i = 0; i < seed.length; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i), 3432918353);
    h = (h << 13) | (h >>> 19);
  }
  let a = h >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};
