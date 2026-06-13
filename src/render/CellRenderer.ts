import * as THREE from "three";
import type { CellMesh } from "../mesh/CellMesh";
import type { CellData } from "../data/CellData";
import { DENSITY_MAX, DENSITY_MIN, type PlateData } from "../plates/PlateData";
import { longitudeOf, projectPoint, type Projection } from "./projection";

/**
 * Builds a single static Three.js mesh for the whole cell grid and exposes a
 * cheap colour-only update path.
 *
 * Each cell is a triangle fan from its centre to its ordered corners, using its
 * OWN copy of every vertex (no sharing across cells) so faces stay flat and
 * colours never bleed across cell borders. Geometry is built once; recolouring
 * only rewrites the colour buffer.
 */
/**
 * How far each overlay floats above the faces. On the sphere this scales the
 * radius (`1 + lift`); on the Mercator map it becomes a small +z offset toward
 * the camera. Edges sit just above the faces, faults above the wireframe, and
 * velocity arrows above everything else so they stay readable.
 */
const EDGE_LIFT = 0.003;
const FAULT_LIFT = 0.005;
const VELOCITY_LIFT = 0.01;

/** Arrow length per unit surface speed (rad/Myr) — tuned for visibility. */
const VELOCITY_SCALE = 15;

/** Arrowhead size as a fraction of the arrow shaft length. */
const HEAD_FRACTION = 0.3;

/**
 * How cells are coloured: by elevation, flat (data view), by plate id, by crust
 * type (oceanic vs continental), or by density.
 */
export type ColourMode = "elevation" | "data" | "plate" | "crust" | "density";

export class CellRenderer {
    /** Group holding the filled faces, the faint wireframe, faults and seeds. */
    readonly object3D = new THREE.Group();

    private readonly geometry: THREE.BufferGeometry;
    private readonly material: THREE.MeshBasicMaterial;
    private readonly colours: Float32Array;

    /** Face vertex positions precomputed for both projections; swapped on toggle. */
    private readonly spherePositions: Float32Array;
    private readonly mercatorPositions: Float32Array;

    private readonly edgeGeometry: THREE.BufferGeometry;
    private readonly edgeMaterial: THREE.LineBasicMaterial;
    private readonly edges: THREE.LineSegments;

    /** Edge segment positions precomputed for both projections; swapped on toggle. */
    private readonly sphereEdgePositions: Float32Array;
    private readonly mercatorEdgePositions: Float32Array;

    /** Active projection. Overlays are re-derived through it on every update. */
    private projection: Projection = "sphere";

    /** Last overlay inputs, cached so a projection switch can re-place them. */
    private crackedCache: Uint8Array | null = null;
    private seedsCache: readonly number[] | null = null;
    private velocityPlateData: PlateData | null = null;
    private velocityData: CellData | null = null;

    /** Bright overlay drawing only the cracked arcs (the fault network). */
    private readonly faultGeometry: THREE.BufferGeometry;
    private readonly faultMaterial: THREE.LineBasicMaterial;
    private readonly faultLines: THREE.LineSegments;
    private readonly faultPositions: Float32Array;

    /** Markers at the seed junctions the player placed. */
    private readonly seedGeometry: THREE.BufferGeometry;
    private readonly seedMaterial: THREE.PointsMaterial;
    private readonly seedPoints: THREE.Points;

    /** One arrow per plate showing its linear velocity at its centroid. */
    private readonly velocityGeometry: THREE.BufferGeometry;
    private readonly velocityMaterial: THREE.LineBasicMaterial;
    private readonly velocityLines: THREE.LineSegments;

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
        const mercatorPositions = new Float32Array(vertexCount * 3);
        this.colours = new Float32Array(vertexCount * 3);
        const indices = new Uint32Array(triangleCount * 3);

        // Edge line segments: one per polygon side, both endpoints per segment.
        const edgePositions = new Float32Array(cornerTotal * 2 * 3);
        const mercatorEdgePositions = new Float32Array(cornerTotal * 2 * 3);
        let edgeVertex = 0;

        const writeTriple = (
            target: Float32Array,
            index: number,
            p: readonly [number, number, number],
        ): void => {
            target[index * 3] = p[0];
            target[index * 3 + 1] = p[1];
            target[index * 3 + 2] = p[2];
        };

        let indexWrite = 0;
        for (let i = 0; i < cellCount; i++) {
            const centre = cellMesh.position(i);
            const corners = cellMesh.polygon(i);
            const base = this.cellVertexStart[i];
            // Unwrap every corner of this cell to the centre longitude so the
            // polygon stays contiguous across the antimeridian seam on the map.
            const refLon = longitudeOf(centre[0], centre[1], centre[2]);

            // Vertex 0 of the cell is the centre; 1..C are the corners.
            writeTriple(positions, base, centre);
            writeTriple(
                mercatorPositions,
                base,
                projectPoint(centre[0], centre[1], centre[2], "mercator", 0, refLon),
            );
            for (let k = 0; k < corners.length; k++) {
                const v = base + 1 + k;
                writeTriple(positions, v, corners[k]);
                writeTriple(
                    mercatorPositions,
                    v,
                    projectPoint(
                        corners[k][0],
                        corners[k][1],
                        corners[k][2],
                        "mercator",
                        0,
                        refLon,
                    ),
                );
            }

            // Fan triangles: (centre, corner k, corner k+1).
            for (let k = 0; k < corners.length; k++) {
                const next = (k + 1) % corners.length;
                indices[indexWrite++] = base;
                indices[indexWrite++] = base + 1 + k;
                indices[indexWrite++] = base + 1 + next;

                // Boundary segment corner[k] -> corner[next], lifted off the faces.
                const a = corners[k];
                const b = corners[next];
                writeTriple(
                    edgePositions,
                    edgeVertex,
                    projectPoint(a[0], a[1], a[2], "sphere", EDGE_LIFT),
                );
                writeTriple(
                    mercatorEdgePositions,
                    edgeVertex,
                    projectPoint(a[0], a[1], a[2], "mercator", EDGE_LIFT, refLon),
                );
                edgeVertex++;
                writeTriple(
                    edgePositions,
                    edgeVertex,
                    projectPoint(b[0], b[1], b[2], "sphere", EDGE_LIFT),
                );
                writeTriple(
                    mercatorEdgePositions,
                    edgeVertex,
                    projectPoint(b[0], b[1], b[2], "mercator", EDGE_LIFT, refLon),
                );
                edgeVertex++;
            }
        }

        this.spherePositions = positions;
        this.mercatorPositions = mercatorPositions;
        this.sphereEdgePositions = edgePositions;
        this.mercatorEdgePositions = mercatorEdgePositions;

        this.geometry = new THREE.BufferGeometry();
        this.geometry.setAttribute(
            "position",
            new THREE.BufferAttribute(positions.slice(), 3),
        );
        this.geometry.setAttribute(
            "color",
            new THREE.BufferAttribute(this.colours, 3),
        );
        this.geometry.setIndex(new THREE.BufferAttribute(indices, 1));
        this.geometry.computeBoundingSphere();

        // Double-sided: cell winding faces outward on the sphere, which flips to
        // back-facing once flattened onto the Mercator plane under a top-down camera.
        this.material = new THREE.MeshBasicMaterial({
            vertexColors: true,
            side: THREE.DoubleSide,
        });
        const mesh = new THREE.Mesh(this.geometry, this.material);

        this.edgeGeometry = new THREE.BufferGeometry();
        this.edgeGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(edgePositions.slice(), 3),
        );
        this.edgeMaterial = new THREE.LineBasicMaterial({ color: 0x000000 });
        this.edges = new THREE.LineSegments(this.edgeGeometry, this.edgeMaterial);
        this.edges.visible = false;
        this.edges.frustumCulled = false;

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
        this.faultLines.frustumCulled = false;

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
        this.seedPoints.frustumCulled = false;

        this.velocityGeometry = new THREE.BufferGeometry();
        this.velocityGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(new Float32Array(0), 3),
        );
        this.velocityMaterial = new THREE.LineBasicMaterial({ color: 0xffffff });
        this.velocityLines = new THREE.LineSegments(
            this.velocityGeometry,
            this.velocityMaterial,
        );
        this.velocityLines.visible = false;
        this.velocityLines.frustumCulled = false;

        this.object3D.add(
            mesh,
            this.edges,
            this.faultLines,
            this.seedPoints,
            this.velocityLines,
        );
    }

    /**
     * Rewrite the colour buffer for the given mode. Cheap: no geometry rebuild,
     * just one colour attribute upload.
     *
     * - `"elevation"`: ocean/land ramp split at `seaLevel`.
     * - `"data"`: flat white faces (the black wireframe carries the structure).
     * - `"plate"`: a stable per-`plateId` hue so plates read as solid regions.
     * - `"crust"`: oceanic vs continental two-tone.
     * - `"density"`: a light-to-dark ramp over the crust density range.
     */
    updateColours(
        data: CellData,
        mode: ColourMode = "elevation",
        seaLevel = 0,
    ): void {
        const { cellCount } = this.cellMesh;
        for (let i = 0; i < cellCount; i++) {
            const [r, g, b] = colourForCell(data, mode, i, seaLevel);
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
        this.crackedCache = cracked;
        const graph = this.cellMesh.boundaryGraph();
        const { arcEnds, junctionPos } = graph;
        let w = 0;
        for (let a = 0; a < graph.arcCount; a++) {
            if (!cracked[a]) continue;
            const j0 = arcEnds[a * 2];
            const j1 = arcEnds[a * 2 + 1];
            const x0 = junctionPos[j0 * 3];
            const y0 = junctionPos[j0 * 3 + 1];
            const z0 = junctionPos[j0 * 3 + 2];
            // Keep the arc contiguous: unwrap its far end to the near end's longitude.
            const refLon = longitudeOf(x0, y0, z0);
            const p0 = projectPoint(x0, y0, z0, this.projection, FAULT_LIFT, refLon);
            const p1 = projectPoint(
                junctionPos[j1 * 3],
                junctionPos[j1 * 3 + 1],
                junctionPos[j1 * 3 + 2],
                this.projection,
                FAULT_LIFT,
                refLon,
            );
            this.faultPositions[w++] = p0[0];
            this.faultPositions[w++] = p0[1];
            this.faultPositions[w++] = p0[2];
            this.faultPositions[w++] = p1[0];
            this.faultPositions[w++] = p1[1];
            this.faultPositions[w++] = p1[2];
        }
        this.faultGeometry.setDrawRange(0, w / 3);
        const attr = this.faultGeometry.getAttribute(
            "position",
        ) as THREE.BufferAttribute;
        attr.needsUpdate = true;
    }

    /** Rebuild the seed markers from the placed seed junctions. */
    updateSeeds(seeds: readonly number[]): void {
        this.seedsCache = seeds;
        const { junctionPos } = this.cellMesh.boundaryGraph();
        const positions = new Float32Array(seeds.length * 3);
        for (let s = 0; s < seeds.length; s++) {
            const j = seeds[s];
            const p = projectPoint(
                junctionPos[j * 3],
                junctionPos[j * 3 + 1],
                junctionPos[j * 3 + 2],
                this.projection,
                FAULT_LIFT,
            );
            positions[s * 3] = p[0];
            positions[s * 3 + 1] = p[1];
            positions[s * 3 + 2] = p[2];
        }
        this.seedGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(positions, 3),
        );
        this.seedGeometry.getAttribute("position").needsUpdate = true;
    }

    /**
     * Rebuild the per-plate velocity arrows. Each plate gets one arrow at its
     * centroid pointing along its linear surface velocity (`omega x centroid`),
     * with length scaled by speed. Plates that are empty or effectively still
     * are skipped. Arrows are drawn as line segments: a shaft plus two head
     * strokes, all pushed just above the surface.
     */
    updateVelocities(plateData: PlateData, data: CellData): void {
        this.velocityPlateData = plateData;
        this.velocityData = data;
        const centroids = plateData.centroids(this.cellMesh, data);
        const segments: number[] = [];

        for (const [id, centroid] of centroids) {
            const [cx, cy, cz] = centroid;
            if (cx === 0 && cy === 0 && cz === 0) continue; // empty plate

            const v = plateData.linearVelocity(id, [cx, cy, cz]);
            const speed = Math.hypot(v[0], v[1], v[2]);
            if (speed < 1e-6) continue; // effectively still

            // Unit heading along the velocity and a perpendicular in the
            // tangent plane (centroid is the surface normal) for the arrowhead.
            const dx = v[0] / speed;
            const dy = v[1] / speed;
            const dz = v[2] / speed;
            const sx = cy * dz - cz * dy;
            const sy = cz * dx - cx * dz;
            const sz = cx * dy - cy * dx;

            const length = speed * VELOCITY_SCALE;
            const head = length * HEAD_FRACTION;
            const tipX = cx + dx * length;
            const tipY = cy + dy * length;
            const tipZ = cz + dz * length;

            // Project the whole arrow against the centroid longitude so it stays
            // contiguous on the map seam.
            const refLon = longitudeOf(cx, cy, cz);
            const push = (
                ax: number,
                ay: number,
                az: number,
                bx: number,
                by: number,
                bz: number,
            ): void =>
                pushSegment(
                    segments,
                    this.projection,
                    refLon,
                    ax,
                    ay,
                    az,
                    bx,
                    by,
                    bz,
                );

            // Shaft.
            push(cx, cy, cz, tipX, tipY, tipZ);
            // Two head strokes, swept back from the tip and out to each side.
            const backX = tipX - dx * head;
            const backY = tipY - dy * head;
            const backZ = tipZ - dz * head;
            push(
                tipX,
                tipY,
                tipZ,
                backX + sx * head,
                backY + sy * head,
                backZ + sz * head,
            );
            push(
                tipX,
                tipY,
                tipZ,
                backX - sx * head,
                backY - sy * head,
                backZ - sz * head,
            );
        }

        this.velocityGeometry.setAttribute(
            "position",
            new THREE.BufferAttribute(new Float32Array(segments), 3),
        );
        this.velocityGeometry.getAttribute("position").needsUpdate = true;
    }

    /**
     * Switch the projection the scene is drawn in. Swaps the precomputed face
     * and edge buffers and re-derives the dynamic overlays (faults, seeds,
     * velocity arrows) from their cached inputs so the map shows exactly what
     * the sphere showed. Colours are untouched.
     */
    setProjection(projection: Projection): void {
        if (projection === this.projection) return;
        this.projection = projection;

        const facePositions =
            projection === "sphere" ? this.spherePositions : this.mercatorPositions;
        const faceAttr = this.geometry.getAttribute(
            "position",
        ) as THREE.BufferAttribute;
        (faceAttr.array as Float32Array).set(facePositions);
        faceAttr.needsUpdate = true;
        this.geometry.computeBoundingSphere();

        const edgePositions =
            projection === "sphere"
                ? this.sphereEdgePositions
                : this.mercatorEdgePositions;
        const edgeAttr = this.edgeGeometry.getAttribute(
            "position",
        ) as THREE.BufferAttribute;
        (edgeAttr.array as Float32Array).set(edgePositions);
        edgeAttr.needsUpdate = true;

        if (this.crackedCache) this.updateFaults(this.crackedCache);
        if (this.seedsCache) this.updateSeeds(this.seedsCache);
        if (this.velocityPlateData && this.velocityData) {
            this.updateVelocities(this.velocityPlateData, this.velocityData);
        }
    }

    /** Show or hide the plate-velocity arrows. */
    setVelocitiesVisible(visible: boolean): void {
        this.velocityLines.visible = visible;
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
        this.velocityGeometry.dispose();
        this.velocityMaterial.dispose();
    }
}

/** Append a single line segment, projecting both endpoints above the surface. */
const pushSegment = (
    out: number[],
    projection: Projection,
    refLon: number,
    ax: number,
    ay: number,
    az: number,
    bx: number,
    by: number,
    bz: number,
): void => {
    const a = projectPoint(ax, ay, az, projection, VELOCITY_LIFT, refLon);
    const b = projectPoint(bx, by, bz, projection, VELOCITY_LIFT, refLon);
    out.push(a[0], a[1], a[2], b[0], b[1], b[2]);
};

/** Pick a cell's colour for the active mode. */
const colourForCell = (
    data: CellData,
    mode: ColourMode,
    i: number,
    seaLevel: number,
): [number, number, number] => {
    switch (mode) {
        case "data":
            return [1, 1, 1];
        case "plate":
            return colourForPlate(data.plateId[i]);
        case "crust":
            return colourForCrust(data.crustType[i]);
        case "density":
            return colourForDensity(data.density[i]);
        default:
            return colourForElevation(data.elevation[i], seaLevel);
    }
};

/** Oceanic crust reads cool blue; continental crust reads warm tan. */
const colourForCrust = (crustType: number): [number, number, number] =>
    crustType === 1 ? [0.82, 0.71, 0.48] : [0.13, 0.32, 0.55];

/** Light (least dense) to dark (most dense) ramp over the density range. */
const colourForDensity = (density: number): [number, number, number] => {
    const t = clamp01((density - DENSITY_MIN) / (DENSITY_MAX - DENSITY_MIN));
    return mix([0.95, 0.93, 0.82], [0.35, 0.12, 0.18], t);
};

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
