import type { CellData } from "../data/CellData";
import {
  dot,
  length,
  normalise,
  rotateAboutAxis,
  type Vec3,
} from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";
import type { AdvectionField } from "./massBudget";
import { totalCrustMass } from "./massBudget";

/**
 * Fraction of a subducting (denser) cell's crust mass that accretes onto the
 * surviving (more buoyant) cell at a convergent claim; the remainder is consumed
 * into the mantle and recorded in the subduction ledger. 1 would make live mass
 * exactly conserved; less than 1 lets convergence destroy crust while keeping the
 * budget honest (`liveMass + subductedTotal` constant). Tunable for 06.
 */
const ACCRETION_FRACTION = 0.5;

/** Density (g/cm3) stamped on an empty rift placeholder until 07 fills it. */
const RIFT_DENSITY = 2.9;

/**
 * Cell whose centre is closest to `target` on the unit sphere, found by a greedy
 * hill-climb from `guess`: repeatedly hop to whichever neighbour increases
 * `dot(position, target)` (the spherical proximity measure) until no neighbour
 * improves. Per-step plate motion is tiny, so starting from the cell itself the
 * walk terminates in O(1) hops; the worst case is bounded by the mesh diameter.
 */
export const nearestCell = (
  mesh: CellMesh,
  target: Vec3,
  guess: number,
): number => {
  let current = guess;
  let bestDot = dot(mesh.position(current), target);
  for (;;) {
    let moved = false;
    const nbrs = mesh.neighbors(current);
    for (let k = 0; k < nbrs.length; k++) {
      const n = nbrs[k];
      const d = dot(mesh.position(n), target);
      if (d > bestDot) {
        bestDot = d;
        current = n;
        moved = true;
      }
    }
    if (!moved) return current;
  }
};

/** Copy every per-cell crust field from `source` to `dest`. */
const copyCell = (
  src: CellData,
  dst: CellData,
  source: number,
  dest: number,
): void => {
  dst.elevation[dest] = src.elevation[source];
  dst.plateId[dest] = src.plateId[source];
  dst.crustType[dest] = src.crustType[source];
  dst.age[dest] = src.age[source];
  dst.density[dest] = src.density[source];
  dst.thickness[dest] = src.thickness[source];
};

/**
 * Advance one tick of rigid plate motion by conservative forward-scatter
 * advection.
 *
 * Every cell carries crust that belongs to a plate. A plate moves by rotating
 * about its Euler pole, so each source cell *scatters* its crust forward into
 * the cell its rotation carries it to (`dest`). Counting how many sources land
 * in each destination turns the two boundary artefacts of a naive gather into
 * explicit, conserved events:
 *
 * - **1 claim**  -> ordinary transport: the crust simply moves.
 * - **2+ claims** -> convergence: the more buoyant (lower-density) cell survives
 *   on top; the denser one subducts. A fraction of the subducting mass accretes
 *   onto the survivor (thickening it) and the rest is recorded as destroyed, so
 *   `liveMass + subductedTotal` is conserved and no crust is duplicated.
 * - **0 claims** -> divergence: a gap. At a divergent boundary it is left as an
 *   empty rift placeholder and flagged for 07 to fill; elsewhere (discretisation
 *   noise inside a plate) it falls back to a self-copy so no spurious hole opens.
 *
 * Boundary classification (`boundaries`, from ticket 04) only gates the rift /
 * subduction *labelling*; occupancy alone drives transport and conservation.
 *
 * Pure over the mesh interface and flat typed arrays, so it can later move into
 * a Web Worker / WASM without restructuring upstream.
 */
export const advect = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  plateData: PlateData,
  dtMyr: number,
  boundaries: BoundaryField,
  out: AdvectionField,
): void => {
  const { cellCount } = mesh;
  const { dest, winner, accreted, riftCell, subducted } = out;

  winner.fill(-1);
  accreted.fill(0);
  riftCell.fill(0);
  subducted.fill(0);
  let subductedThisStep = 0;

  // Pass 0: where does each cell's crust scatter to under its plate's rotation?
  for (let i = 0; i < cellCount; i++) {
    const plate = src.plateId[i];
    const props = plate >= 0 ? plateData.byId.get(plate) : undefined;
    const angle = props ? length(props.omega) * dtMyr : 0;
    if (props && angle !== 0) {
      const axis = normalise(props.omega);
      // Forward rotation: where does the crust now at `i` end up?
      const to = rotateAboutAxis(mesh.position(i), axis, angle);
      dest[i] = nearestCell(mesh, to, i);
    } else {
      dest[i] = i; // stationary or unowned crust stays put
    }
  }

  // Pass 1: resolve each destination's winner, accreting and ledgering losers.
  for (let i = 0; i < cellCount; i++) {
    const d = dest[i];
    const w = winner[d];
    if (w < 0) {
      winner[d] = i;
      continue;
    }

    // The more buoyant (lower-density) crust survives on top; ties by index.
    const iWins =
      src.density[i] < src.density[w] ||
      (src.density[i] === src.density[w] && i < w);
    const survivor = iWins ? i : w;
    const loser = iWins ? w : i;
    winner[d] = survivor;

    const loserMass = mesh.area(loser) * src.thickness[loser];
    const accretedMass = ACCRETION_FRACTION * loserMass;
    subductedThisStep += loserMass - accretedMass;
    accreted[d] += accretedMass / mesh.area(d);
    subducted[d] = 1;
  }

  // Pass 2: write the surviving crust into each destination (or open a rift).
  for (let d = 0; d < cellCount; d++) {
    const w = winner[d];
    if (w >= 0) {
      copyCell(src, dst, w, d);
      dst.thickness[d] += accreted[d];
    } else if (boundaries.cellKind[d] === BoundaryKind.DIVERGENT) {
      // Empty rift gap: marked now, filled with fresh crust in 07.
      dst.elevation[d] = 0;
      dst.plateId[d] = src.plateId[d]; // provisional ownership, finalised in 07
      dst.crustType[d] = 0; // oceanic
      dst.age[d] = 0;
      dst.density[d] = RIFT_DENSITY;
      dst.thickness[d] = 0;
      riftCell[d] = 1;
    } else {
      // Discretisation noise inside a plate: keep the cell's own crust.
      copyCell(src, dst, d, d);
    }
  }

  out.subductedThisStep = subductedThisStep;
  out.subductedTotal += subductedThisStep;
  out.liveMass = totalCrustMass(mesh, dst);
};
