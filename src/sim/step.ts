import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { advect } from "./advect";
import type { BoundaryField } from "./boundaries";
import type { AdvectionField } from "./massBudget";

/**
 * One simulation tick: advance plate motion by `dtMyr`.
 *
 * Reads crust from `src` and writes the advected result into `dst` (a separate
 * back buffer), touching only flat typed arrays via the mesh interface — never
 * rendering or the DOM. Keeping this signature pure means tectonics can later
 * move into a Web Worker without restructuring anything upstream.
 *
 * `boundaries` must already be classified against `src` so the conservative
 * advection sees the current convergent/divergent edges; `out` collects the
 * rift / subduction events and the conservation budget. Elevation response and
 * crust lifecycle act on those outputs in later increments.
 */
export const step = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  plateData: PlateData,
  dtMyr: number,
  boundaries: BoundaryField,
  out: AdvectionField,
): void => {
  advect(mesh, src, dst, plateData, dtMyr, boundaries, out);
};
