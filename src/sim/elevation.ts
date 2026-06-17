import type { CellData } from "../data/CellData";
import { clamp, clamp01, lerp } from "../math/scalar";
import type { CellMesh } from "../mesh/CellMesh";
import { MAX_OCEANIC_AGE } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";

/**
 * Crust type codes mirrored from {@link CellData.crustType} (0 oceanic, 1
 * continental). Continental crust does not yet exist — it is minted by accretion
 * in ticket 08 — but the convergent rules below already discriminate on it so
 * collision ranges appear automatically once it does.
 */
const CONTINENTAL = 1;

/**
 * Tuning constants for the elevation response. These are the 07 "tuning sink":
 * reasoned starting values, not empirically calibrated, kept in one block so the
 * whole terrain feel can be dialled here (mirrors the const block in
 * `plateForces.ts`). All elevations are in km, all rates per Myr.
 */

/** Shallow mid-ocean-ridge crest: where young (age 0) oceanic crust sits. */
const RIDGE_ELEVATION = -2.5;

/** Deep abyssal plain: where fully cooled (age `MAX_OCEANIC_AGE`) crust sinks. */
const ABYSS_ELEVATION = -6;

/** Baseline elevation of unstressed continental crust at reference thickness. */
const CONTINENT_BASE = 0.6;

/** Reference continental thickness (km) above which extra crust adds height. */
const CONTINENT_THICKNESS_REF = 7;

/** Height (km) gained per km of continental crust above the reference thickness. */
const CONTINENT_THICKNESS_GAIN = 0.1;

/**
 * Uplift gain (km per unit closing speed per Myr) for a continent-continent
 * collision, where both plates pile up (Himalaya-style). The strongest term.
 */
const COLLISION_UPLIFT = 4;

/**
 * Uplift gain for the buoyant overriding plate at a subduction zone: the
 * volcanic mountain / island arc (Andes / Aleutians).
 */
const ARC_UPLIFT = 3;

/** Deepening gain for the subducting (denser) plate: the oceanic trench. */
const TRENCH_DEEPEN = 3.5;

/**
 * Fraction per Myr by which a divergent boundary's cells are pulled toward the
 * ridge crest. A spreading centre shoals to a roughly fixed crest height
 * regardless of rate, so this is a pull toward `RIDGE_ELEVATION`, not an
 * intensity-scaled push.
 */
const RIDGE_PULL = 0.05;

/**
 * Isostatic relaxation: fraction per Myr by which every cell drifts back toward
 * its crust baseline. This erodes inactive relief over deep time and caps the
 * accumulated uplift/subsidence so it plateaus instead of running away.
 */
const RELAX_RATE = 0.02;

/** Hard clamp on elevation (km), matching the renderer's colour ramp range. */
const ELEVATION_MIN = -10;
const ELEVATION_MAX = 10;

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
        const extra = Math.max(0, thickness - CONTINENT_THICKNESS_REF);
        return CONTINENT_BASE + extra * CONTINENT_THICKNESS_GAIN;
    }
    const t = clamp01(age / MAX_OCEANIC_AGE);
    return lerp(RIDGE_ELEVATION, ABYSS_ELEVATION, t);
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
                const up = COLLISION_UPLIFT * drive;
                data.elevation[cellA] += up;
                data.elevation[cellB] += up;
            } else {
                // Subduction: the denser cell sinks into a trench (matching advect's
                // density-driven rule), the buoyant one rises into a volcanic arc.
                const aDenser = data.density[cellA] >= data.density[cellB];
                const trenchCell = aDenser ? cellA : cellB;
                const arcCell = aDenser ? cellB : cellA;
                data.elevation[trenchCell] -= TRENCH_DEEPEN * drive;
                data.elevation[arcCell] += ARC_UPLIFT * drive;
            }
        } else if (kind === BoundaryKind.DIVERGENT) {
            // Mid-ocean ridge: shoal both sides toward the crest height.
            const pull = RIDGE_PULL * dtMyr;
            data.elevation[cellA] +=
                (RIDGE_ELEVATION - data.elevation[cellA]) * pull;
            data.elevation[cellB] +=
                (RIDGE_ELEVATION - data.elevation[cellB]) * pull;
        }
        // Transform: minimal vertical change, so no force is applied.
    }

    // Pass B: isostatic relaxation toward each cell's crust baseline.
    const relax = clamp01(RELAX_RATE * dtMyr);
    for (let i = 0; i < mesh.cellCount; i++) {
        const base = baselineElevation(
            data.crustType[i],
            data.age[i],
            data.thickness[i],
        );
        let e = data.elevation[i] + (base - data.elevation[i]) * relax;
        e = clamp(e, ELEVATION_MIN, ELEVATION_MAX);
        data.elevation[i] = e;
    }
};
