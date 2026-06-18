import type { CellData } from "../data/CellData";
import type { CellMesh } from "../mesh/CellMesh";
import {
  CONTINENTAL_DENSITY,
  OCEANIC_DENSITY_YOUNG,
  OCEANIC_THICKNESS,
  oceanicDensityFromAge,
} from "../plates/PlateData";
import { baselineElevation } from "./elevation";
import type { AdvectionField } from "./massBudget";
import { totalCrustMass } from "./massBudget";

/**
 * Crust type codes mirrored from {@link CellData.crustType} (0 oceanic, 1
 * continental).
 */
const OCEANIC = 0;
const CONTINENTAL = 1;

/**
 * Tuning constants for the crust lifecycle (the 08 tuning sink, mirroring the
 * const blocks in `elevation.ts` and `plateForces.ts`). Reasoned starting
 * values, not empirically calibrated.
 */

/**
 * Crust thickness (km) above which accreted oceanic crust differentiates into
 * continental crust. Convergence thickens the surviving cell via accretion (see
 * `advect.ts`); once a cell piles up past this it stops behaving as subductable
 * ocean and becomes buoyant continental crust (one-way). Set above the starting
 * {@link OCEANIC_THICKNESS} so only genuinely thickened crust converts.
 */
const CONTINENTAL_THICKNESS_THRESHOLD = 15;

/**
 * Crust creation, destruction and ageing: the lifecycle that keeps the crust
 * budget honest over deep time.
 *
 * Runs after {@link advect} on the freshly advected `data`, consuming the
 * `riftCell` flags advect raised (subduction destruction is already ledgered in
 * advect). In one pass over the mesh:
 *
 * - **Rift creation** — each cell advect left as a divergent gap is stamped with
 *   fresh young oceanic crust (oceanic, age 0, ridge-crest elevation, young
 *   density, reference thickness), keeping the spreading-plate ownership advect
 *   assigned provisionally. The minted mass is added to the creation ledger.
 * - **Ageing** — every other cell ages by `dtMyr`. Oceanic crust recomputes its
 *   density from the shared age curve, so the oldest ocean is the densest and so
 *   the first to subduct at the next convergence.
 * - **Continental differentiation** — oceanic crust thickened past
 *   {@link CONTINENTAL_THICKNESS_THRESHOLD} by accretion flips to buoyant
 *   continental crust (one-way), the deferred-from-07 path that makes the
 *   continental elevation rules fire.
 *
 * Finally `liveMass` is recomputed here (not in advect) because the rift crust
 * only exists once this pass fills it, and `createdThisStep` / `createdTotal`
 * are updated so the UI can report net mass change as created minus destroyed.
 *
 * Pure over the mesh interface and flat typed arrays, so it can move into a
 * Worker / WASM alongside advect/elevation.
 */
export const applyCrustLifecycle = (
  mesh: CellMesh,
  data: CellData,
  dtMyr: number,
  field: AdvectionField,
): void => {
  const { riftCell } = field;
  let createdThisStep = 0;

  for (let i = 0; i < mesh.cellCount; i++) {
    if (riftCell[i]) {
      // Fresh young oceanic crust at the spreading ridge. Ownership was set
      // provisionally by advect (inherits the spreading plate); keep it.
      data.crustType[i] = OCEANIC;
      data.age[i] = 0;
      data.density[i] = OCEANIC_DENSITY_YOUNG;
      data.thickness[i] = OCEANIC_THICKNESS;
      data.elevation[i] = baselineElevation(OCEANIC, 0, OCEANIC_THICKNESS);
      createdThisStep += mesh.area(i) * OCEANIC_THICKNESS;
      continue;
    }

    // Age every surviving cell; recompute density / differentiate oceanic crust.
    data.age[i] += dtMyr;
    if (data.crustType[i] === OCEANIC) {
      if (data.thickness[i] >= CONTINENTAL_THICKNESS_THRESHOLD) {
        data.crustType[i] = CONTINENTAL;
        data.density[i] = CONTINENTAL_DENSITY;
      } else {
        data.density[i] = oceanicDensityFromAge(data.age[i]);
      }
    }
  }

  field.createdThisStep = createdThisStep;
  field.createdTotal += createdThisStep;
  // Recompute live mass now that rift crust has been minted: advect computes it
  // before this pass fills the rifts, so its value omits the new crust.
  field.liveMass = totalCrustMass(mesh, data);
};
