import { createNoise3D } from "simplex-noise";
import type { CellMesh } from "../mesh/CellMesh";

/**
 * Data-oriented per-cell storage: flat typed arrays indexed by cell.
 *
 * There is deliberately no `Cell` class with methods — this layout iterates at
 * near-native speed, has zero GC pressure, serialises trivially, and is the
 * structure to keep even if hot loops later move to a Web Worker / WASM / GPU.
 */
export class CellData {
    /** Prototype elevation scale roughly -10..+10. */
    readonly elevation: Float32Array;

    /** Plate membership. Present now but unused (all 0) until the plates phase. */
    readonly plateId: Int32Array;

    constructor(cellCount: number) {
        this.elevation = new Float32Array(cellCount);
        this.plateId = new Int32Array(cellCount);
    }
}

/**
 * Seed a deterministic test-pattern elevation so cells render with distinct,
 * structured colours. Layered 3D simplex noise sampled at each cell centre
 * gives continent-like blobs; this is purely to prove the render path, not a
 * simulation step.
 */
export const seedTestElevation = (
    mesh: CellMesh,
    data: CellData,
    seed = "tectonics",
): void => {
    const noise = createNoise3D(makeRng(seed));
    const octaves = 4;
    const lacunarity = 2;
    const gain = 0.5;
    const baseFrequency = 1.6;

    for (let i = 0; i < mesh.cellCount; i++) {
        const [x, y, z] = mesh.position(i);
        let amplitude = 1;
        let frequency = baseFrequency;
        let sum = 0;
        let norm = 0;
        for (let o = 0; o < octaves; o++) {
            sum +=
                amplitude * noise(x * frequency, y * frequency, z * frequency);
            norm += amplitude;
            amplitude *= gain;
            frequency *= lacunarity;
        }
        // Normalised to [-1, 1], then shaped and scaled to the elevation range.
        const n = sum / norm;
        data.elevation[i] = shapeElevation(n) * 10;
    }
};

/** Bias the noise so there is a clear sea-level split between low and high. */
const shapeElevation = (n: number): number => {
    const ridged = Math.sign(n) * Math.pow(Math.abs(n), 1.2);
    return Math.max(-1, Math.min(1, ridged));
};

/** Tiny deterministic PRNG (mulberry32) seeded from a string. */
const makeRng = (seed: string): (() => number) => {
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
