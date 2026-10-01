import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { advect } from "./advect";
import type { BoundaryField } from "./boundaries";
import { applyCrustLifecycle } from "./crustLifecycle";
import { applyElevation } from "./elevation";
import type { AdvectionField } from "./massBudget";
import { updatePlateIdentity } from "./plateLifecycle";
import { updateRifting } from "./rifting";
import { updateSutures } from "./sutures";

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
 * rift / subduction events and the conservation budget. The crust lifecycle
 * then mints young crust at those rifts, ages the rest and differentiates
 * continental crust; the elevation response finally builds relief on the
 * freshly aged `dst` from those same boundaries.
 *
 * Suture bookkeeping runs first, on `src`, so that a collision that has welded
 * into a single mass -- or two plates that have finally merged into one -- is
 * already in effect when the crust moves. Plate identity is reconciled straight
 * after the crust moves, since that is what consumes plates and pinches them
 * apart; `allocatePlateId` mints the stable id a fragment needs. Rifting runs
 * last, the one step that *creates* plates, so that the tension it reads is the
 * tension of the plates as they now stand.
 */
export const step = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  plateData: PlateData,
  dtMyr: number,
  boundaries: BoundaryField,
  out: AdvectionField,
  allocatePlateId: () => number,
): void => {
  updateSutures(mesh, src, plateData, boundaries, dtMyr);
  advect(mesh, src, dst, plateData, dtMyr, boundaries, out);
  updatePlateIdentity(mesh, dst, plateData, allocatePlateId);
  updateRifting(mesh, dst, plateData, dtMyr, allocatePlateId);
  applyCrustLifecycle(mesh, dst, dtMyr, out);
  applyElevation(mesh, dst, dtMyr, boundaries);
};
