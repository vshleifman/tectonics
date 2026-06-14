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
 * Source cell whose crust rotates into `d` under `d`'s *own* plate's motion: the
 * inverse of the forward scatter. Doubles as the test for an interior aliasing
 * hole -- if the result is a same-plate cell, `d` is still inside its plate and
 * should keep that plate's crust rather than letting a neighbour bleed across a
 * boundary. Returns `d` itself for stationary or unowned crust.
 */
const gatherSource = (
  mesh: CellMesh,
  d: number,
  src: CellData,
  plateData: PlateData,
  dtMyr: number,
): number => {
  const plate = src.plateId[d];
  const props = plate >= 0 ? plateData.byId.get(plate) : undefined;
  const angle = props ? length(props.omega) * dtMyr : 0;
  if (!props || angle === 0) return d;
  const axis = normalise(props.omega);
  // Inverse rotation: where did the crust now arriving at `d` come from?
  const from = rotateAboutAxis(mesh.position(d), axis, -angle);
  return nearestCell(mesh, from, d);
};

/**
 * Representative source cell of the plate that dominates `d`'s already-resolved
 * neighbours, or -1 if no neighbour was claimed this step.
 *
 * Used to fill a hole with the crust actually sweeping into it: an interior
 * aliasing hole is ringed by its own plate and so refills seamlessly, while a
 * trailing-edge gap is dominated by the advancing neighbour and is taken over by
 * it -- instead of being re-seeded with the departing plate's crust, which is
 * what smears a trail behind a moving plate. Neighbour counts use the fully
 * resolved `winner` map, so iteration order in pass 2 does not matter.
 */
const dominantNeighbourSource = (
  mesh: CellMesh,
  d: number,
  src: CellData,
  winner: Int32Array,
): number => {
  const nbrs = mesh.neighbors(d);
  let bestCount = 0;
  let bestSource = -1;
  for (let k = 0; k < nbrs.length; k++) {
    const w = winner[nbrs[k]];
    if (w < 0) continue;
    const plate = src.plateId[w];
    // Tally how many neighbours carry this plate, keeping one source for it.
    let count = 0;
    for (let j = 0; j < nbrs.length; j++) {
      const wj = winner[nbrs[j]];
      if (wj >= 0 && src.plateId[wj] === plate) count++;
    }
    if (count > bestCount) {
      bestCount = count;
      bestSource = w;
    }
  }
  return bestSource;
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

  // Pass 1: resolve each destination's winner. At a genuine convergent boundary
  // the loser subducts and is ledgered; an interior collision is discretisation
  // noise, so the survivor simply occupies the cell and the loser is dropped.
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

    // Only real convergence destroys crust; interior aliasing collisions must
    // not, or they would pollute the subduction ledger and erode plate interiors.
    if (boundaries.cellKind[d] === BoundaryKind.CONVERGENT) {
      const loserMass = mesh.area(loser) * src.thickness[loser];
      const accretedMass = ACCRETION_FRACTION * loserMass;
      subductedThisStep += loserMass - accretedMass;
      accreted[d] += accretedMass / mesh.area(d);
      subducted[d] = 1;
    }
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
      // Hole: no crust scattered onto `d`. Decide what fills it.
      const ownSource = gatherSource(mesh, d, src, plateData, dtMyr);
      if (ownSource !== d && src.plateId[ownSource] === src.plateId[d]) {
        // Interior aliasing hole: `d`'s own plate still rotates into it, so keep
        // it same-plate. No foreign crust bleeds across a boundary (no speckle).
        copyCell(src, dst, ownSource, d);
      } else {
        // `d`'s plate has genuinely vacated it: hand the cell to the plate
        // sweeping in (its dominant claimed neighbour), so an advancing plate
        // takes over the trailing edge instead of the departing plate smearing a
        // trail. A wholly vacated pocket (no claimed neighbour) keeps `ownSource`.
        const neighbourSource = dominantNeighbourSource(mesh, d, src, winner);
        copyCell(
          src,
          dst,
          neighbourSource >= 0 ? neighbourSource : ownSource,
          d,
        );
      }
    }
  }

  out.subductedThisStep = subductedThisStep;
  out.subductedTotal += subductedThisStep;
  out.liveMass = totalCrustMass(mesh, dst);
};
