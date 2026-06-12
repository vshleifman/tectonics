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
/** Push edge lines this far above the unit sphere to avoid z-fighting with faces. */
const EDGE_RADIUS = 1.003;

export class CellRenderer {
    /** Group holding the filled faces and (optionally visible) black edges. */
    readonly object3D = new THREE.Group();

    private readonly geometry: THREE.BufferGeometry;
    private readonly material: THREE.MeshBasicMaterial;
    private readonly colours: Float32Array;

    private readonly edgeGeometry: THREE.BufferGeometry;
    private readonly edgeMaterial: THREE.LineBasicMaterial;
    private readonly edges: THREE.LineSegments;

    /** First vertex index for each cell (CSR-style, length cellCount + 1). */
    private readonly cellVertexStart: Int32Array;

    private readonly cellMesh: CellMesh;

    constructor(cellMesh: CellMesh) {
        this.cellMesh = cellMesh;
        const { cellCount } = cellMesh;

        // Count vertices/triangles: per cell = 1 centre + C corners, C triangles.
        this.cellVertexStart = new Int32Array(cellCount + 1);
        let triangleCount = 0;
        let cornerTotal = 0;
        for (let i = 0; i < cellCount; i++) {
            const corners = cellMesh.polygon(i).length;
            this.cellVertexStart[i + 1] = this.cellVertexStart[i] + corners + 1;
            triangleCount += corners;
            cornerTotal += corners;
        }
        const vertexCount = this.cellVertexStart[cellCount];

        const positions = new Float32Array(vertexCount * 3);
        this.colours = new Float32Array(vertexCount * 3);
        const indices = new Uint32Array(triangleCount * 3);

        // Edge line segments: one per polygon side, both endpoints per segment.
        const edgePositions = new Float32Array(cornerTotal * 2 * 3);
        let edgeWrite = 0;

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

                // Boundary segment corner[k] -> corner[next], pushed outward.
                const a = corners[k];
                const b = corners[next];
                edgePositions[edgeWrite++] = a[0] * EDGE_RADIUS;
                edgePositions[edgeWrite++] = a[1] * EDGE_RADIUS;
                edgePositions[edgeWrite++] = a[2] * EDGE_RADIUS;
                edgePositions[edgeWrite++] = b[0] * EDGE_RADIUS;
                edgePositions[edgeWrite++] = b[1] * EDGE_RADIUS;
                edgePositions[edgeWrite++] = b[2] * EDGE_RADIUS;
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
        const mesh = new THREE.Mesh(this.geometry, this.material);

        this.edgeGeometry = new THREE.BufferGeometry();
        this.edgeGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(edgePositions, 3),
        );
        this.edgeMaterial = new THREE.LineBasicMaterial({ color: 0x000000 });
        this.edges = new THREE.LineSegments(this.edgeGeometry, this.edgeMaterial);
        this.edges.visible = false;

        this.object3D.add(mesh, this.edges);
    }

    /**
     * Rewrite the colour buffer from current elevation. Cheap: no geometry
     * rebuild, just one colour attribute upload. `seaLevel` is the elevation
     * threshold below which a cell is rendered as ocean. When `dataMode` is
     * set, faces are rendered flat white (the black edges carry the structure).
     */
    updateColours(data: CellData, seaLevel = 0, dataMode = false): void {
        const { cellCount } = this.cellMesh;
        for (let i = 0; i < cellCount; i++) {
            const [r, g, b] = dataMode
                ? [1, 1, 1]
                : colourForElevation(data.elevation[i], seaLevel);
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

    /** Show or hide the black cell-boundary lines (used in data mode). */
    setEdgesVisible(visible: boolean): void {
        this.edges.visible = visible;
    }

    dispose(): void {
        this.geometry.dispose();
        this.material.dispose();
        this.edgeGeometry.dispose();
        this.edgeMaterial.dispose();
    }
}

const MIN_ELEVATION = -10;
const MAX_ELEVATION = 10;

/**
 * Map an elevation value (~-10..+10) to an RGB triple in [0, 1], with `seaLevel`
 * as the ocean/land boundary. Both the ocean and land ramps are normalised
 * relative to sea level so the full gradient is used at any threshold.
 */
const colourForElevation = (
    elevation: number,
    seaLevel: number,
): [number, number, number] => {
    if (elevation < seaLevel) {
        // Ocean: deep navy (deepest) to shallow cyan (near sea level).
        const span = seaLevel - MIN_ELEVATION || 1;
        const t = clamp01((elevation - MIN_ELEVATION) / span);
        return mix([0.02, 0.05, 0.2], [0.1, 0.45, 0.6], t);
    }
    // Land: beach -> green -> brown -> snow, normalised above sea level.
    const span = MAX_ELEVATION - seaLevel || 1;
    const t = clamp01((elevation - seaLevel) / span);
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
