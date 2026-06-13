import type { CellData } from "../data/CellData";
import { dot, length, normalise, rotateAboutAxis, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";

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

/**
 * Advance one tick of rigid plate motion by semi-Lagrangian advection.
 *
 * Every cell carries crust that belongs to a plate. A plate moves by rotating
 * about its Euler pole, so to find what crust sits at cell `i` after `dtMyr` we
 * trace `i` backward along its current plate's rotation and gather the crust
 * from the source cell that lands there. Reading from `src` and writing to a
 * separate `dst` keeps the gather conflict-free, and copying `plateId` along
 * with the crust makes plate boundaries emergent (they travel with the crust;
 * no per-step flood fill).
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
): void => {
  const { cellCount } = mesh;
  for (let i = 0; i < cellCount; i++) {
    const plate = src.plateId[i];
    const props = plate >= 0 ? plateData.byId.get(plate) : undefined;
    const angle = props ? length(props.omega) * dtMyr : 0;

    // Stationary or unowned crust stays put: copy straight through.
    let source = i;
    if (props && angle !== 0) {
      const axis = normalise(props.omega);
      // Backward rotation: where did the crust now at `i` come from?
      const from = rotateAboutAxis(mesh.position(i), axis, -angle);
      source = nearestCell(mesh, from, i);
    }

    dst.elevation[i] = src.elevation[source];
    dst.plateId[i] = src.plateId[source];
    dst.crustType[i] = src.crustType[source];
    dst.age[i] = src.age[source];
    dst.density[i] = src.density[source];
  }
};
