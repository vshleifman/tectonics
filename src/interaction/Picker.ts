import * as THREE from "three";
import type { BoundaryGraph } from "../mesh/CellMesh";

/**
 * Resolves a pointer event to the nearest fault junction on the globe.
 *
 * Cells live on the unit sphere, so we intersect the camera ray with the unit
 * sphere analytically (no scene geometry needed) and then pick the junction
 * with the smallest angular distance to the hit — a max-dot-product scan over
 * the boundary graph, which is linear and sub-millisecond even at 100k+ cells.
 */
export class Picker {
    private readonly raycaster = new THREE.Raycaster();
    private readonly sphere = new THREE.Sphere(new THREE.Vector3(0, 0, 0), 1);
    private readonly ndc = new THREE.Vector2();
    private readonly hit = new THREE.Vector3();

    constructor(
        private readonly camera: THREE.Camera,
        private readonly domElement: HTMLElement,
    ) {}

    /** Junction id under the pointer, or null if the ray misses the globe. */
    pickJunction(event: PointerEvent, graph: BoundaryGraph): number | null {
        const rect = this.domElement.getBoundingClientRect();
        this.ndc.set(
            ((event.clientX - rect.left) / rect.width) * 2 - 1,
            -(((event.clientY - rect.top) / rect.height) * 2 - 1),
        );
        this.raycaster.setFromCamera(this.ndc, this.camera);
        if (!this.raycaster.ray.intersectSphere(this.sphere, this.hit)) {
            return null;
        }

        const pos = graph.junctionPos;
        const { x, y, z } = this.hit;
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
