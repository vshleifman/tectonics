/**
 * A point on (or near) the unit sphere, stored as a plain tuple so it ports
 * cleanly to flat typed arrays / WASM without object overhead.
 */
export type Vec3 = readonly [number, number, number];

/**
 * The single most important abstraction in the system.
 *
 * Downstream code (data, rendering, simulation) consumes ONLY this interface
 * and never references the concrete tiling. `IcosphereMesh` implements it now;
 * a `VoronoiMesh` can implement it later and the swap is a constructor change.
 *
 * A "cell" is just an index `i` in the range [0, cellCount).
 */
export interface CellMesh {
    /** Total number of cells. Indices are valid for [0, cellCount). */
    readonly cellCount: number;

    /** Adjacent cell indices for cell `i`. Precomputed once at build. */
    neighbors(i: number): readonly number[];

    /** Cell centre on the unit sphere. */
    position(i: number): Vec3;

    /** Corner vertices of the cell face, ordered around the centre. */
    polygon(i: number): readonly Vec3[];

    /** Cell area on the unit sphere (for later conservation maths). */
    area(i: number): number;
}
