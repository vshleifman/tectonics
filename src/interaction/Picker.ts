import * as THREE from "three";
import type { BoundaryGraph, CellMesh } from "../mesh/CellMesh";
import {
  MAP_HALF_EXTENT,
  MAP_HALF_HEIGHT,
  type Projection,
  unprojectMercator,
} from "../render/projection";

/**
 * Resolves a pointer to the nearest fault junction or cell on the globe.
 *
 * On the sphere we intersect the camera ray with the unit sphere analytically
 * (no scene geometry needed). On the Mercator map we unproject the pointer to
 * the map plane and invert the projection back to a sphere direction. Either
 * way we then pick the junction or cell with the smallest angular distance.
 */
export class Picker {
  private readonly raycaster = new THREE.Raycaster();
  private readonly sphere = new THREE.Sphere(new THREE.Vector3(0, 0, 0), 1);
  private readonly ndc = new THREE.Vector2();
  private readonly hit = new THREE.Vector3();
  private readonly point = new THREE.Vector3();
  private projection: Projection = "sphere";

  constructor(
    private camera: THREE.Camera,
    private readonly domElement: HTMLElement,
  ) {}

  /** Switch which camera + projection subsequent picks are resolved against. */
  setProjection(projection: Projection, camera: THREE.Camera): void {
    this.projection = projection;
    this.camera = camera;
  }

  /**
   * Unit direction on the globe under the client point, or null if the point
   * misses the globe or lies off the edge of the map.
   */
  private direction(
    clientX: number,
    clientY: number,
  ): [number, number, number] | null {
    const rect = this.domElement.getBoundingClientRect();
    this.ndc.set(
      ((clientX - rect.left) / rect.width) * 2 - 1,
      -(((clientY - rect.top) / rect.height) * 2 - 1),
    );
    if (this.projection === "mercator") {
      // Unproject to the map plane; x/y are the map coords regardless of
      // depth under an axis-aligned orthographic camera.
      this.point.set(this.ndc.x, this.ndc.y, 0).unproject(this.camera);
      if (
        Math.abs(this.point.x) > MAP_HALF_EXTENT ||
        Math.abs(this.point.y) > MAP_HALF_HEIGHT
      ) {
        return null;
      }
      return unprojectMercator(this.point.x, this.point.y);
    }
    this.raycaster.setFromCamera(this.ndc, this.camera);
    if (!this.raycaster.ray.intersectSphere(this.sphere, this.hit)) {
      return null;
    }
    return [this.hit.x, this.hit.y, this.hit.z];
  }

  /**
   * Cell under the client point, or null off the world. Cells are the regions
   * nearest each centre, so the nearest centre is found by walking from
   * `start` to whichever neighbour lies closer until none does: a few steps
   * when the pointer has moved a little, as it does while hovering, where a
   * scan of every cell would cost a millisecond a frame at the finest levels.
   */
  pickCell(
    clientX: number,
    clientY: number,
    mesh: CellMesh,
    start = 0,
  ): number | null {
    const target = this.direction(clientX, clientY);
    if (!target) return null;
    const [x, y, z] = target;
    const { centres, neighbourOffsets, neighbourList } = mesh;
    const closeness = (i: number): number =>
      centres[i * 3] * x + centres[i * 3 + 1] * y + centres[i * 3 + 2] * z;
    let cell = start >= 0 && start < mesh.cellCount ? start : 0;
    let best = closeness(cell);
    for (;;) {
      let next = -1;
      for (
        let k = neighbourOffsets[cell];
        k < neighbourOffsets[cell + 1];
        k++
      ) {
        const m = neighbourList[k];
        const dot = closeness(m);
        if (dot > best) {
          best = dot;
          next = m;
        }
      }
      if (next < 0) return cell;
      cell = next;
    }
  }

  /** Junction id under the pointer, or null if the pointer misses the world. */
  pickJunction(event: PointerEvent, graph: BoundaryGraph): number | null {
    const target = this.direction(event.clientX, event.clientY);
    if (!target) return null;
    const [x, y, z] = target;

    const pos = graph.junctionPos;
    let best = -1;
    let bestDot = -Infinity;
    for (let j = 0; j < graph.junctionCount; j++) {
      const o = j * 3;
      const dot = pos[o] * x + pos[o + 1] * y + pos[o + 2] * z;
      if (dot > bestDot) {
        bestDot = dot;
        best = j;
      }
    }
    return best;
  }
}
