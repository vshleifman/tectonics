import * as THREE from "three";
import type { BoundaryGraph } from "../mesh/CellMesh";
import { type Projection, unprojectMercator } from "../render/projection";

/**
 * Resolves a pointer event to the nearest fault junction on the globe.
 *
 * On the sphere we intersect the camera ray with the unit sphere analytically
 * (no scene geometry needed). On the Mercator map we unproject the pointer to
 * the map plane and invert the projection back to a sphere direction. Either
 * way we then pick the junction with the smallest angular distance — a
 * max-dot-product scan over the boundary graph, linear and sub-millisecond even
 * at 100k+ cells.
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

  /** Junction id under the pointer, or null if the pointer misses the world. */
  pickJunction(event: PointerEvent, graph: BoundaryGraph): number | null {
    const rect = this.domElement.getBoundingClientRect();
    this.ndc.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -(((event.clientY - rect.top) / rect.height) * 2 - 1),
    );

    let x: number;
    let y: number;
    let z: number;
    if (this.projection === "mercator") {
      // Unproject to the map plane; x/y are the map coords regardless of
      // depth under an axis-aligned orthographic camera.
      this.point.set(this.ndc.x, this.ndc.y, 0).unproject(this.camera);
      [x, y, z] = unprojectMercator(this.point.x, this.point.y);
    } else {
      this.raycaster.setFromCamera(this.ndc, this.camera);
      if (!this.raycaster.ray.intersectSphere(this.sphere, this.hit)) {
        return null;
      }
      ({ x, y, z } = this.hit);
    }

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
