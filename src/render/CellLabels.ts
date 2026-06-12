import * as THREE from "three";
import {
    CSS2DObject,
    CSS2DRenderer,
} from "three/examples/jsm/renderers/CSS2DRenderer.js";
import type { CellMesh } from "../mesh/CellMesh";
import type { CellData } from "../data/CellData";

/**
 * Above this cell count we skip per-cell text labels: the DOM cost is too high
 * and the text would be unreadable anyway. Greyscale shading still applies.
 */
export const LABEL_LIMIT = 3000;

interface Label {
    object: CSS2DObject;
    normal: THREE.Vector3; // cell centre direction (unit), for front-face culling
}

/**
 * Overlays crisp HTML labels (cell id + elevation) on each cell using a
 * CSS2DRenderer. Labels are only built when the cell count is small enough,
 * and only front-facing labels are shown each frame so the back of the sphere
 * stays uncluttered.
 */
export class CellLabels {
    readonly object3D = new THREE.Group();
    private readonly css = new CSS2DRenderer();
    private readonly labels: Label[] = [];
    private visible = false;
    private suppressed = false; // true when cellCount exceeded LABEL_LIMIT

    constructor(container: HTMLElement) {
        this.css.setSize(window.innerWidth, window.innerHeight);
        Object.assign(this.css.domElement.style, {
            position: "fixed",
            top: "0",
            left: "0",
            pointerEvents: "none",
            display: "none",
        } satisfies Partial<CSSStyleDeclaration>);
        container.appendChild(this.css.domElement);
        this.object3D.visible = false;
    }

    /** True when the current mesh has too many cells to label. */
    get isSuppressed(): boolean {
        return this.suppressed;
    }

    /** Rebuild labels for a new mesh + data. Clears any previous labels. */
    build(mesh: CellMesh, data: CellData): void {
        this.clear();
        this.suppressed = mesh.cellCount > LABEL_LIMIT;
        if (this.suppressed) return;

        for (let i = 0; i < mesh.cellCount; i++) {
            const [x, y, z] = mesh.position(i);
            const element = document.createElement("div");
            Object.assign(element.style, {
                color: "#ffffff",
                font: "9px/1.1 ui-monospace, monospace",
                textAlign: "center",
                whiteSpace: "pre",
                textShadow: "0 0 2px #000, 0 0 2px #000, 0 0 3px #000",
                pointerEvents: "none",
            } satisfies Partial<CSSStyleDeclaration>);
            element.textContent = `${i}\n${data.elevation[i].toFixed(1)}`;

            const object = new CSS2DObject(element);
            object.position.set(x, y, z);
            this.object3D.add(object);
            this.labels.push({
                object,
                normal: new THREE.Vector3(x, y, z),
            });
        }
    }

    setVisible(visible: boolean): void {
        this.visible = visible;
        this.object3D.visible = visible;
        this.css.domElement.style.display = visible ? "block" : "none";
    }

    /** Per-frame: show only labels on the camera-facing hemisphere. */
    update(camera: THREE.Camera): void {
        if (!this.visible) return;
        const cam = camera.position;
        for (const label of this.labels) {
            // Front-facing when the camera lies beyond the cell's tangent plane:
            // cam . n > n . n (= 1 for a unit-sphere normal). Toggle the object's
            // visibility (CSS2DRenderer overwrites element.style.display itself).
            label.object.visible =
                label.normal.dot(cam) > label.normal.lengthSq();
        }
    }

    render(scene: THREE.Scene, camera: THREE.Camera): void {
        this.css.render(scene, camera);
    }

    resize(): void {
        this.css.setSize(window.innerWidth, window.innerHeight);
    }

    private clear(): void {
        for (const label of this.labels) {
            label.object.element.remove();
        }
        this.labels.length = 0;
        this.object3D.clear();
    }
}
