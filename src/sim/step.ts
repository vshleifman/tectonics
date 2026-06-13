import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { advect } from "./advect";

/**
 * One simulation tick: advance plate motion by `dtMyr`.
 *
 * Reads crust from `src` and writes the advected result into `dst` (a separate
 * back buffer), touching only flat typed arrays via the mesh interface — never
 * rendering or the DOM. Keeping this signature pure means tectonics can later
 * move into a Web Worker without restructuring anything upstream.
 *
 * For now the only rule is rigid rotation + crust advection; boundary
 * classification and elevation response come in a later increment.
 */
export const step = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  plateData: PlateData,
  dtMyr: number,
): void => {
  advect(mesh, src, dst, plateData, dtMyr);
};
