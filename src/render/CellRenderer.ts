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
/** Faults sit slightly above the faint wireframe so they read on top. */
const FAULT_RADIUS = 1.005;

/** How cells are coloured: by elevation, flat (data view), or by plate id. */
export type ColourMode = "elevation" | "data" | "plate";

export class CellRenderer {
    /** Group holding the filled faces, the faint wireframe, faults and seeds. */
    readonly object3D = new THREE.Group();

    private readonly geometry: THREE.BufferGeometry;
    private readonly material: THREE.MeshBasicMaterial;
    private readonly colours: Float32Array;

    private readonly edgeGeometry: THREE.BufferGeometry;
    private readonly edgeMaterial: THREE.LineBasicMaterial;
    private readonly edges: THREE.LineSegments;

    /** Bright overlay drawing only the cracked arcs (the fault network). */
    private readonly faultGeometry: THREE.BufferGeometry;
    private readonly faultMaterial: THREE.LineBasicMaterial;
    private readonly faultLines: THREE.LineSegments;
    private readonly faultPositions: Float32Array;

    /** Markers at the seed junctions the player placed. */
    private readonly seedGeometry: THREE.BufferGeometry;
    private readonly seedMaterial: THREE.PointsMaterial;
    private readonly seedPoints: THREE.Points;

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

        // Fault overlay: preallocate room for every arc, draw only cracked ones.
        const graph = cellMesh.boundaryGraph();
        this.faultPositions = new Float32Array(graph.arcCount * 2 * 3);
        this.faultGeometry = new THREE.BufferGeometry();
        this.faultGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(this.faultPositions, 3),
        );
        this.faultGeometry.setDrawRange(0, 0);
        this.faultMaterial = new THREE.LineBasicMaterial({ color: 0xff5a3c });
        this.faultLines = new THREE.LineSegments(
            this.faultGeometry,
            this.faultMaterial,
        );
        this.faultLines.visible = false;

        this.seedGeometry = new THREE.BufferGeometry();
        this.seedGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(new Float32Array(0), 3),
        );
        this.seedMaterial = new THREE.PointsMaterial({
            color: 0xffe14d,
            size: 9,
            sizeAttenuation: false,
        });
        this.seedPoints = new THREE.Points(this.seedGeometry, this.seedMaterial);
        this.seedPoints.visible = false;

        this.object3D.add(mesh, this.edges, this.faultLines, this.seedPoints);
    }

    /**
     * Rewrite the colour buffer for the given mode. Cheap: no geometry rebuild,
     * just one colour attribute upload.
     *
     * - `"elevation"`: ocean/land ramp split at `seaLevel`.
     * - `"data"`: flat white faces (the black wireframe carries the structure).
     * - `"plate"`: a stable per-`plateId` hue so plates read as solid regions.
     */
    updateColours(
        data: CellData,
        mode: ColourMode = "elevation",
        seaLevel = 0,
    ): void {
        const { cellCount } = this.cellMesh;
        for (let i = 0; i < cellCount; i++) {
            const [r, g, b] =
                mode === "data"
                    ? [1, 1, 1]
                    : mode === "plate"
                      ? colourForPlate(data.plateId[i])
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

    /** Rebuild the bright fault overlay from the cracked-arc flags. */
    updateFaults(cracked: Uint8Array): void {
        const graph = this.cellMesh.boundaryGraph();
        const { arcEnds, junctionPos } = graph;
        let w = 0;
        for (let a = 0; a < graph.arcCount; a++) {
            if (!cracked[a]) continue;
            const j0 = arcEnds[a * 2];
            const j1 = arcEnds[a * 2 + 1];
            this.faultPositions[w++] = junctionPos[j0 * 3] * FAULT_RADIUS;
            this.faultPositions[w++] = junctionPos[j0 * 3 + 1] * FAULT_RADIUS;
            this.faultPositions[w++] = junctionPos[j0 * 3 + 2] * FAULT_RADIUS;
            this.faultPositions[w++] = junctionPos[j1 * 3] * FAULT_RADIUS;
            this.faultPositions[w++] = junctionPos[j1 * 3 + 1] * FAULT_RADIUS;
            this.faultPositions[w++] = junctionPos[j1 * 3 + 2] * FAULT_RADIUS;
        }
        this.faultGeometry.setDrawRange(0, w / 3);
        const attr = this.faultGeometry.getAttribute(
            "position",
        ) as THREE.BufferAttribute;
        attr.needsUpdate = true;
    }

    /** Rebuild the seed markers from the placed seed junctions. */
    updateSeeds(seeds: readonly number[]): void {
        const { junctionPos } = this.cellMesh.boundaryGraph();
        const positions = new Float32Array(seeds.length * 3);
        for (let s = 0; s < seeds.length; s++) {
            const j = seeds[s];
            positions[s * 3] = junctionPos[j * 3] * FAULT_RADIUS;
            positions[s * 3 + 1] = junctionPos[j * 3 + 1] * FAULT_RADIUS;
            positions[s * 3 + 2] = junctionPos[j * 3 + 2] * FAULT_RADIUS;
        }
        this.seedGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(positions, 3),
        );
        this.seedGeometry.getAttribute("position").needsUpdate = true;
    }

    /** Show or hide the black cell-boundary lines (used in data mode). */
    setEdgesVisible(visible: boolean): void {
        this.edges.visible = visible;
    }

    /** Show or hide the fault overlay and seed markers together. */
    setFaultsVisible(visible: boolean): void {
        this.faultLines.visible = visible;
        this.seedPoints.visible = visible;
    }

    dispose(): void {
        this.geometry.dispose();
        this.material.dispose();
        this.edgeGeometry.dispose();
        this.edgeMaterial.dispose();
        this.faultGeometry.dispose();
        this.faultMaterial.dispose();
        this.seedGeometry.dispose();
        this.seedMaterial.dispose();
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

/** Golden-ratio hue stepping gives every plate a distinct, stable colour. */
const colourForPlate = (plateId: number): [number, number, number] => {
    if (plateId < 0) return [0.5, 0.5, 0.5];
    const hue = (plateId * 0.6180339887498949) % 1;
    return hslToRgb(hue, 0.55, 0.55);
};

const hslToRgb = (
    h: number,
    s: number,
    l: number,
): [number, number, number] => {
    const c = (1 - Math.abs(2 * l - 1)) * s;
    const hp = h * 6;
    const x = c * (1 - Math.abs((hp % 2) - 1));
    let r = 0;
    let g = 0;
    let b = 0;
    if (hp < 1) [r, g, b] = [c, x, 0];
    else if (hp < 2) [r, g, b] = [x, c, 0];
    else if (hp < 3) [r, g, b] = [0, c, x];
    else if (hp < 4) [r, g, b] = [0, x, c];
    else if (hp < 5) [r, g, b] = [x, 0, c];
    else [r, g, b] = [c, 0, x];
    const m = l - c / 2;
    return [r + m, g + m, b + m];
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
