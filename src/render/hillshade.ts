import type { CellMesh } from "../mesh/CellMesh";

/** Planet radius in km, to turn a slope per unit-sphere distance into a real one. */
const RADIUS_KM = 6371;

/** The two dials on the shading. */
export interface HillshadeOptions {
  /**
   * Vertical exaggeration. Real slopes are tiny (a few km over ~100 km cells),
   * so unexaggerated the relief would shade as flat.
   */
  exaggeration: number;
  /** How far the shade swings the colour either side of its flat-ground value. */
  strength: number;
}

/** Sun from the north-west at 45 degrees, the cartographic convention. */
const AZIMUTH = (315 * Math.PI) / 180;
const ALTITUDE = Math.PI / 4;
const LIGHT_E = Math.sin(AZIMUTH) * Math.cos(ALTITUDE);
const LIGHT_N = Math.cos(AZIMUTH) * Math.cos(ALTITUDE);
const LIGHT_UP = Math.sin(ALTITUDE);

const MIN_SHADE = 0.55;
const MAX_SHADE = 1.25;

/** Seabed relief is shaded this much as land, so the ocean does not look lit. */
const OCEAN_WEIGHT = 0.3;

/**
 * Fill `out` with a per-cell brightness multiplier, 1 on flat ground. Each cell's
 * gradient is a least-squares plane through its neighbours' heights, taken in the
 * tangent frame at the cell centre (east, north), then lit from a fixed
 * north-west sun. The light is per cell, so it reads the same on the sphere and
 * the map.
 */
export const writeHillshade = (
  out: Float32Array,
  mesh: CellMesh,
  elevation: ArrayLike<number>,
  { exaggeration, strength }: HillshadeOptions,
): void => {
  const { cellCount, centres, neighbourOffsets, neighbourList } = mesh;
  const flat = LIGHT_UP;
  for (let i = 0; i < cellCount; i++) {
    const px = centres[i * 3];
    const py = centres[i * 3 + 1];
    const pz = centres[i * 3 + 2];

    // East = up x p, north = p x east. Degenerate at the poles, where any
    // tangent axis will do.
    let ex = pz;
    let ez = -px;
    let el = Math.hypot(ex, ez);
    if (el < 1e-6) {
      ex = 1;
      ez = 0;
      el = 1;
    }
    ex /= el;
    ez /= el;
    const nx = py * ez;
    const ny = pz * ex - px * ez;
    const nz = -py * ex;

    let aee = 0;
    let aen = 0;
    let ann = 0;
    let be = 0;
    let bn = 0;
    const h0 = elevation[i];
    for (let k = neighbourOffsets[i]; k < neighbourOffsets[i + 1]; k++) {
      const j = neighbourList[k];
      const dx = centres[j * 3] - px;
      const dy = centres[j * 3 + 1] - py;
      const dz = centres[j * 3 + 2] - pz;
      const de = dx * ex + dz * ez;
      const dn = dx * nx + dy * ny + dz * nz;
      const dh = elevation[j] - h0;
      aee += de * de;
      aen += de * dn;
      ann += dn * dn;
      be += de * dh;
      bn += dn * dh;
    }
    const det = aee * ann - aen * aen;
    if (det < 1e-12) {
      out[i] = 1;
      continue;
    }
    const scale = exaggeration / RADIUS_KM;
    const sx = ((ann * be - aen * bn) / det) * scale;
    const sy = ((aee * bn - aen * be) / det) * scale;

    // Surface normal (-sx, -sy, 1) against the light direction.
    const lit =
      (-sx * LIGHT_E - sy * LIGHT_N + LIGHT_UP) / Math.hypot(sx, sy, 1);
    const weight = h0 < 0 ? OCEAN_WEIGHT : 1;
    const shade = 1 + strength * weight * (lit / flat - 1);
    out[i] = Math.min(MAX_SHADE, Math.max(MIN_SHADE, shade));
  }
};
