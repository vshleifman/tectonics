import type { CellData } from "../data/CellData";
import { clamp, clamp01, lerp } from "../math/scalar";
import type { CellMesh } from "../mesh/CellMesh";
import { type BoundaryField, BoundaryKind } from "./boundaries";
import { tuning } from "./tuning";

/**
 * Crust type codes mirrored from {@link CellData.crustType} (0 oceanic, 1
 * continental). Continental crust does not yet exist — it is minted by accretion
 * in ticket 08 — but the convergent rules below already discriminate on it so
 * collision ranges appear automatically once it does.
 */
const CONTINENTAL = 1;

/**
 * The elevation tuning constants now live in {@link tuning} so the UI can tweak
 * them live. They are read inside the functions below (not captured at module
 * load) so each tick reflects the current values.
 */

/**
 * Isostatic baseline elevation for a cell from its crust alone (no boundary
 * stress). Oceanic crust starts at the ridge crest and deepens monotonically as
 * it ages and cools; continental crust floats high and rises with thickness.
 * This is the "couple elevation to crust age/density" requirement — the term the
 * relaxation pass pulls toward.
 */
export const baselineElevation = (
    crustType: number,
    age: number,
    thickness: number,
): number => {
    if (crustType === CONTINENTAL) {
        const extra = Math.max(0, thickness - tuning.continentThicknessRef);
        return tuning.continentBase + extra * tuning.continentThicknessGain;
    }
    const t = clamp01(age / tuning.maxOceanicAge);
    return lerp(tuning.ridgeElevation, tuning.abyssElevation, t);
};

/**
 * Build relief from plate interaction: the visible payoff of the simulation.
 *
 * Runs after {@link advect} on the freshly advected `data`, in two passes:
 *
 * - **Boundary forces** — for each active boundary arc, nudge the elevation of
 *   the two cells it separates by an increment scaled by the closing speed and
 *   `dtMyr`, per the geology of the boundary:
 *   - Convergent, both continental -> collision range: both sides rise.
 *   - Convergent, otherwise -> the denser cell subducts (a trench deepens) while
 *     the buoyant overriding cell rises into a volcanic arc. This one branch
 *     covers oceanic-oceanic (island arc) and oceanic-continental (Andean arc).
 *   - Divergent -> both cells are pulled toward the shallow ridge crest.
 *   - Transform -> no vertical change (sliding).
 * - **Isostatic relaxation** — every cell drifts toward its crust baseline, so
 *   relief accumulates while a boundary stays active, then erodes once it goes
 *   quiet, and never runs away.
 *
 * The boundary field is the same `src`-classified one {@link advect} consumes,
 * so a boundary's location is approximately stable across the step (the same
 * approximation advect makes when gating subduction). Pure over the mesh
 * interface and flat typed arrays, so it can move into a Worker / WASM.
 */
export const applyElevation = (
    mesh: CellMesh,
    data: CellData,
    dtMyr: number,
    boundaries: BoundaryField,
): void => {
    const { arcKind, arcClosing } = boundaries;
    const { arcCells, arcCount } = mesh.boundaryGraph();

    // Pass A: boundary forces.
    for (let a = 0; a < arcCount; a++) {
        const kind = arcKind[a];
        if (kind === BoundaryKind.NONE) continue;

        const cellA = arcCells[a * 2];
        const cellB = arcCells[a * 2 + 1];
        if (cellA < 0 || cellB < 0) continue;

        // `arcClosing` is the signed normal speed (+ closing); its magnitude is the
        // interaction intensity that scales the per-tick relief increment.
        const drive = Math.abs(arcClosing[a]) * dtMyr;

        if (kind === BoundaryKind.CONVERGENT) {
            const bothContinental =
                data.crustType[cellA] === CONTINENTAL &&
                data.crustType[cellB] === CONTINENTAL;
            if (bothContinental) {
                // Collision range: neither subducts, so both sides pile up.
                const up = tuning.collisionUplift * drive;
                data.elevation[cellA] += up;
                data.elevation[cellB] += up;
            } else {
                // Subduction: the denser cell sinks into a trench (matching advect's
                // density-driven rule), the buoyant one rises into a volcanic arc.
                const aDenser = data.density[cellA] >= data.density[cellB];
                const trenchCell = aDenser ? cellA : cellB;
                const arcCell = aDenser ? cellB : cellA;
                data.elevation[trenchCell] -= tuning.trenchDeepen * drive;
                data.elevation[arcCell] += tuning.arcUplift * drive;
            }
        } else if (kind === BoundaryKind.DIVERGENT) {
            // Mid-ocean ridge: shoal both sides toward the crest height.
            const pull = tuning.ridgePull * dtMyr;
            data.elevation[cellA] +=
                (tuning.ridgeElevation - data.elevation[cellA]) * pull;
            data.elevation[cellB] +=
                (tuning.ridgeElevation - data.elevation[cellB]) * pull;
        }
        // Transform: minimal vertical change, so no force is applied.
    }

    // Pass B: isostatic relaxation toward each cell's crust baseline.
    const relax = clamp01(tuning.relaxRate * dtMyr);
    for (let i = 0; i < mesh.cellCount; i++) {
        const base = baselineElevation(
            data.crustType[i],
            data.age[i],
            data.thickness[i],
        );
        let e = data.elevation[i] + (base - data.elevation[i]) * relax;
        e = clamp(e, tuning.elevationMin, tuning.elevationMax);
        data.elevation[i] = e;
    }
};
