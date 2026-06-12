import type { CellMesh } from "../mesh/CellMesh";
import type { CellData } from "../data/CellData";

/**
 * Simulation step — a NO-OP for Prototype 1.
 *
 * Intentionally a pure function over the mesh interface and flat typed arrays:
 * it reads/writes only `data`'s arrays and never touches rendering or the DOM.
 * Keeping this signature stable now means tectonics can be implemented here and
 * later moved into a Web Worker without restructuring anything upstream.
 */
export const step = (_mesh: CellMesh, _data: CellData): void => {
    // No tectonics yet. See DesignDoc "Out of Scope (Later Phases)".
};
