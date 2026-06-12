import * as THREE from "three";
import type { CellMesh } from "../mesh/CellMesh";
import type { CellData } from "../data/CellData";

/**
 * Builds a single static Three.js mesh for the whole cell grid and exposes a
 * cheap colour-only update path.
 *
 * Each cell is a triangle fan from its centre to its ordered corners, using its
 * OWN copy of every vertex (no sharing across cells) so faces stay flat and
 * colours never bleed across cell borders. Geometry is built once; recolouring
 * only rewrites the colour buffer.
 */
export class CellRenderer {
    readonly mesh: THREE.Mesh;

    private readonly geometry: THREE.BufferGeometry;
    private readonly material: THREE.MeshBasicMaterial;
    private readonly colours: Float32Array;

    /** First vertex index for each cell (CSR-style, length cellCount + 1). */
    private readonly cellVertexStart: Int32Array;

    private readonly cellMesh: CellMesh;

    constructor(cellMesh: CellMesh) {
        this.cellMesh = cellMesh;
        const { cellCount } = cellMesh;

        // Count vertices/triangles: per cell = 1 centre + C corners, C triangles.
        this.cellVertexStart = new Int32Array(cellCount + 1);
        let triangleCount = 0;
        for (let i = 0; i < cellCount; i++) {
            const corners = cellMesh.polygon(i).length;
            this.cellVertexStart[i + 1] =
                this.cellVertexStart[i] + corners + 1;
            triangleCount += corners;
        }
        const vertexCount = this.cellVertexStart[cellCount];

        const positions = new Float32Array(vertexCount * 3);
        this.colours = new Float32Array(vertexCount * 3);
        const indices = new Uint32Array(triangleCount * 3);

        let indexWrite = 0;
        for (let i = 0; i < cellCount; i++) {
            const centre = cellMesh.position(i);
            const corners = cellMesh.polygon(i);
            const base = this.cellVertexStart[i];

            // Vertex 0 of the cell is the centre; 1..C are the corners.
            positions[base * 3] = centre[0];
            positions[base * 3 + 1] = centre[1];
            positions[base * 3 + 2] = centre[2];
            for (let k = 0; k < corners.length; k++) {
                const v = base + 1 + k;
                positions[v * 3] = corners[k][0];
                positions[v * 3 + 1] = corners[k][1];
                positions[v * 3 + 2] = corners[k][2];
            }

            // Fan triangles: (centre, corner k, corner k+1).
            for (let k = 0; k < corners.length; k++) {
                const next = (k + 1) % corners.length;
                indices[indexWrite++] = base;
                indices[indexWrite++] = base + 1 + k;
                indices[indexWrite++] = base + 1 + next;
            }
        }

        this.geometry = new THREE.BufferGeometry();
        this.geometry.setAttribute(
            "position",
            new THREE.BufferAttribute(positions, 3),
        );
        this.geometry.setAttribute(
            "color",
            new THREE.BufferAttribute(this.colours, 3),
        );
        this.geometry.setIndex(new THREE.BufferAttribute(indices, 1));
        this.geometry.computeBoundingSphere();

        this.material = new THREE.MeshBasicMaterial({ vertexColors: true });
        this.mesh = new THREE.Mesh(this.geometry, this.material);
    }

    /**
     * Rewrite the colour buffer from current elevation. Cheap: no geometry
     * rebuild, just one colour attribute upload.
     */
    updateColours(data: CellData): void {
        const { cellCount } = this.cellMesh;
        for (let i = 0; i < cellCount; i++) {
            const [r, g, b] = colourForElevation(data.elevation[i]);
            const start = this.cellVertexStart[i];
            const end = this.cellVertexStart[i + 1];
            for (let v = start; v < end; v++) {
                this.colours[v * 3] = r;
                this.colours[v * 3 + 1] = g;
                this.colours[v * 3 + 2] = b;
            }
        }
        const attr = this.geometry.getAttribute(
            "color",
        ) as THREE.BufferAttribute;
        attr.needsUpdate = true;
    }

    dispose(): void {
        this.geometry.dispose();
        this.material.dispose();
    }
}

/** Map an elevation value (~-10..+10) to an RGB triple in [0, 1]. */
const colourForElevation = (elevation: number): [number, number, number] => {
    if (elevation < 0) {
        // Ocean: deep navy to shallow cyan.
        const t = clamp01(1 + elevation / 10);
        return mix([0.02, 0.05, 0.2], [0.1, 0.45, 0.6], t);
    }
    // Land: beach -> green -> brown -> snow.
    const t = clamp01(elevation / 10);
    if (t < 0.15) return mix([0.78, 0.72, 0.5], [0.25, 0.5, 0.2], t / 0.15);
    if (t < 0.6)
        return mix([0.25, 0.5, 0.2], [0.45, 0.35, 0.22], (t - 0.15) / 0.45);
    return mix([0.45, 0.35, 0.22], [0.95, 0.95, 0.98], (t - 0.6) / 0.4);
};

const clamp01 = (x: number): number => Math.max(0, Math.min(1, x));

const mix = (
    a: [number, number, number],
    b: [number, number, number],
    t: number,
): [number, number, number] => [
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
];
