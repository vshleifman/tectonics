import type { BoundaryGraph, CellMesh, Vec3 } from "./CellMesh";

/** Mutable 3-component vector used during the build (number[] for speed). */
type V3 = [number, number, number];

const normalise = (v: V3): V3 => {
    const len = Math.hypot(v[0], v[1], v[2]) || 1;
    return [v[0] / len, v[1] / len, v[2] / len];
};

const add = (a: V3, b: V3): V3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const scale = (a: V3, s: number): V3 => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: V3, b: V3): V3 => [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
];

/** The 12 base icosahedron vertices (golden-ratio construction, unit sphere). */
const buildBaseIcosahedron = (): { verts: V3[]; faces: V3[] } => {
    const t = (1 + Math.sqrt(5)) / 2;
    const raw: V3[] = [
        [-1, t, 0],
        [1, t, 0],
        [-1, -t, 0],
        [1, -t, 0],
        [0, -1, t],
        [0, 1, t],
        [0, -1, -t],
        [0, 1, -t],
        [t, 0, -1],
        [t, 0, 1],
        [-t, 0, -1],
        [-t, 0, 1],
    ];
    const verts = raw.map(normalise);
    const faces: V3[] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    return { verts, faces };
};

/**
 * Goldberg-polyhedron cell mesh: subdivide an icosahedron `level` times, treat
 * the triangle vertices as cell centres, and build the dual (hexagons + exactly
 * 12 pentagons) plus a flat neighbour graph. Everything is precomputed in the
 * constructor and exposed only through the {@link CellMesh} interface.
 */
export class IcosphereMesh implements CellMesh {
    readonly cellCount: number;

    /** Cell centres, flat xyz triples (length = 3 * cellCount). */
    private readonly centres: Float32Array;

    /** Neighbour graph in CSR form: list sliced by [offsets[i], offsets[i+1]). */
    private readonly neighbourOffsets: Int32Array;
    private readonly neighbourList: Int32Array;

    /** Arc index for each neighbour-list entry (same CSR layout as above). */
    private readonly neighbourArcList: Int32Array;

    /** Dual edge skeleton used to define faults; built once, retained by ref. */
    private readonly boundary: BoundaryGraph;

    /** Polygon corners in CSR form: corner xyz triples sliced by polyOffsets. */
    private readonly polyOffsets: Int32Array;
    private readonly polyCorners: Float32Array;

    private readonly areas: Float32Array;

    constructor(level: number) {
        const base = buildBaseIcosahedron();
        let verts = base.verts;
        let faces = base.faces;

        for (let s = 0; s < level; s++) {
            const result = subdivideOnce(verts, faces);
            verts = result.verts;
            faces = result.faces;
        }

        this.cellCount = verts.length;
        this.centres = new Float32Array(verts.length * 3);
        for (let i = 0; i < verts.length; i++) {
            this.centres[i * 3] = verts[i][0];
            this.centres[i * 3 + 1] = verts[i][1];
            this.centres[i * 3 + 2] = verts[i][2];
        }

        // Per-vertex adjacency (neighbour vertices) and incident faces.
        const neighbourSets: Set<number>[] = [];
        const incidentFaces: number[][] = [];
        for (let i = 0; i < verts.length; i++) {
            neighbourSets.push(new Set<number>());
            incidentFaces.push([]);
        }
        for (let f = 0; f < faces.length; f++) {
            const [a, b, c] = faces[f];
            incidentFaces[a].push(f);
            incidentFaces[b].push(f);
            incidentFaces[c].push(f);
            neighbourSets[a].add(b);
            neighbourSets[a].add(c);
            neighbourSets[b].add(a);
            neighbourSets[b].add(c);
            neighbourSets[c].add(a);
            neighbourSets[c].add(b);
        }

        // Face centroids on the unit sphere (the dual cell corners = junctions).
        const faceCentroids: V3[] = faces.map(face => {
            const centroid = scale(
                add(add(verts[face[0]], verts[face[1]]), verts[face[2]]),
                1 / 3,
            );
            return normalise(centroid);
        });

        // --- Dual edge skeleton (boundary graph) -----------------------------
        // Junctions are faces; arcs are primal edges. Each primal edge (a,b) is
        // shared by exactly two faces (its junction endpoints) and separates the
        // two cells a, b. `edgeToArc` also lets the neighbour list record which
        // arc severs each link.
        const vertCount = verts.length;
        const edgeKey = (a: number, b: number): number =>
            a < b ? a * vertCount + b : b * vertCount + a;
        const edgeFaces = new Map<number, [number, number]>();
        const edgeCells = new Map<number, [number, number]>();
        for (let f = 0; f < faces.length; f++) {
            const [a, b, c] = faces[f];
            const tri: [number, number][] = [
                [a, b],
                [b, c],
                [c, a],
            ];
            for (const [u, v] of tri) {
                const key = edgeKey(u, v);
                const existing = edgeFaces.get(key);
                if (existing === undefined) {
                    edgeFaces.set(key, [f, -1]);
                    edgeCells.set(key, u < v ? [u, v] : [v, u]);
                } else {
                    existing[1] = f;
                }
            }
        }

        const arcCount = edgeFaces.size;
        const arcEnds = new Int32Array(arcCount * 2);
        const arcCells = new Int32Array(arcCount * 2);
        const edgeToArc = new Map<number, number>();
        // Junction (face) degrees, for the CSR adjacency below.
        const junctionArcOffsets = new Int32Array(faces.length + 1);
        let arcIndex = 0;
        for (const [key, [f0, f1]] of edgeFaces) {
            const cells = edgeCells.get(key) as [number, number];
            arcEnds[arcIndex * 2] = f0;
            arcEnds[arcIndex * 2 + 1] = f1;
            arcCells[arcIndex * 2] = cells[0];
            arcCells[arcIndex * 2 + 1] = cells[1];
            edgeToArc.set(key, arcIndex);
            junctionArcOffsets[f0 + 1]++;
            if (f1 >= 0) junctionArcOffsets[f1 + 1]++;
            arcIndex++;
        }
        for (let j = 0; j < faces.length; j++) {
            junctionArcOffsets[j + 1] += junctionArcOffsets[j];
        }
        const junctionArcs = new Int32Array(junctionArcOffsets[faces.length]);
        const junctionWrite = junctionArcOffsets.slice(0, faces.length);
        for (let a = 0; a < arcCount; a++) {
            const f0 = arcEnds[a * 2];
            const f1 = arcEnds[a * 2 + 1];
            junctionArcs[junctionWrite[f0]++] = a;
            if (f1 >= 0) junctionArcs[junctionWrite[f1]++] = a;
        }

        const junctionPos = new Float32Array(faces.length * 3);
        for (let f = 0; f < faces.length; f++) {
            junctionPos[f * 3] = faceCentroids[f][0];
            junctionPos[f * 3 + 1] = faceCentroids[f][1];
            junctionPos[f * 3 + 2] = faceCentroids[f][2];
        }

        this.boundary = {
            junctionCount: faces.length,
            junctionPos,
            arcCount,
            arcEnds,
            arcCells,
            junctionArcOffsets,
            junctionArcs,
        };

        // Flatten neighbour graph (CSR), recording the arc that severs each link.
        this.neighbourOffsets = new Int32Array(verts.length + 1);
        for (let i = 0; i < verts.length; i++) {
            this.neighbourOffsets[i + 1] =
                this.neighbourOffsets[i] + neighbourSets[i].size;
        }
        this.neighbourList = new Int32Array(
            this.neighbourOffsets[verts.length],
        );
        this.neighbourArcList = new Int32Array(
            this.neighbourOffsets[verts.length],
        );
        for (let i = 0; i < verts.length; i++) {
            let w = this.neighbourOffsets[i];
            for (const n of neighbourSets[i]) {
                this.neighbourList[w] = n;
                this.neighbourArcList[w] = edgeToArc.get(edgeKey(i, n)) ?? -1;
                w++;
            }
        }

        // Build ordered polygon corners + areas for each cell.
        this.polyOffsets = new Int32Array(verts.length + 1);
        for (let i = 0; i < verts.length; i++) {
            this.polyOffsets[i + 1] =
                this.polyOffsets[i] + incidentFaces[i].length;
        }
        this.polyCorners = new Float32Array(this.polyOffsets[verts.length] * 3);
        this.areas = new Float32Array(verts.length);

        for (let i = 0; i < verts.length; i++) {
            const centre = verts[i];
            const ordered = orderCornersAround(
                centre,
                incidentFaces[i].map(f => faceCentroids[f]),
            );
            let w = this.polyOffsets[i] * 3;
            for (const corner of ordered) {
                this.polyCorners[w++] = corner[0];
                this.polyCorners[w++] = corner[1];
                this.polyCorners[w++] = corner[2];
            }
            this.areas[i] = sphericalPolygonArea(centre, ordered);
        }
    }

    neighbors(i: number): readonly number[] {
        const start = this.neighbourOffsets[i];
        const end = this.neighbourOffsets[i + 1];
        const out: number[] = [];
        for (let k = start; k < end; k++) out.push(this.neighbourList[k]);
        return out;
    }

    position(i: number): Vec3 {
        const o = i * 3;
        return [this.centres[o], this.centres[o + 1], this.centres[o + 2]];
    }

    polygon(i: number): readonly Vec3[] {
        const start = this.polyOffsets[i];
        const end = this.polyOffsets[i + 1];
        const out: Vec3[] = [];
        for (let k = start; k < end; k++) {
            const o = k * 3;
            out.push([
                this.polyCorners[o],
                this.polyCorners[o + 1],
                this.polyCorners[o + 2],
            ]);
        }
        return out;
    }

    area(i: number): number {
        return this.areas[i];
    }

    boundaryGraph(): BoundaryGraph {
        return this.boundary;
    }

    neighbourArcs(i: number): readonly number[] {
        const start = this.neighbourOffsets[i];
        const end = this.neighbourOffsets[i + 1];
        const out: number[] = [];
        for (let k = start; k < end; k++) out.push(this.neighbourArcList[k]);
        return out;
    }
}

/** Subdivide every triangle into 4, sharing midpoints via an edge cache. */
const subdivideOnce = (
    verts: V3[],
    faces: V3[],
): { verts: V3[]; faces: V3[] } => {
    const newVerts: V3[] = verts.slice();
    const midpointCache = new Map<string, number>();

    const midpoint = (a: number, b: number): number => {
        const key = a < b ? `${a}_${b}` : `${b}_${a}`;
        const cached = midpointCache.get(key);
        if (cached !== undefined) return cached;
        const mid = normalise(scale(add(verts[a], verts[b]), 0.5));
        const index = newVerts.length;
        newVerts.push(mid);
        midpointCache.set(key, index);
        return index;
    };

    const newFaces: V3[] = [];
    for (const [a, b, c] of faces) {
        const ab = midpoint(a, b);
        const bc = midpoint(b, c);
        const ca = midpoint(c, a);
        newFaces.push([a, ab, ca]);
        newFaces.push([b, bc, ab]);
        newFaces.push([c, ca, bc]);
        newFaces.push([ab, bc, ca]);
    }
    return { verts: newVerts, faces: newFaces };
};

/** Order dual-cell corners angularly in the tangent plane at `centre`. */
const orderCornersAround = (centre: V3, corners: V3[]): V3[] => {
    if (corners.length === 0) return corners;
    // Tangent-plane basis at the cell centre.
    const reference = sub(corners[0], scale(centre, dot(corners[0], centre)));
    const uAxis = normalise(reference);
    const vAxis = cross(centre, uAxis);
    const withAngle = corners.map(corner => {
        const d = sub(corner, scale(centre, dot(corner, centre)));
        const angle = Math.atan2(dot(d, vAxis), dot(d, uAxis));
        return { corner, angle };
    });
    withAngle.sort((p, q) => p.angle - q.angle);
    return withAngle.map(p => p.corner);
};

/** Solid angle (= unit-sphere area) of a triangle of unit vectors. */
const sphericalTriangleArea = (a: V3, b: V3, c: V3): number => {
    const numerator = Math.abs(dot(a, cross(b, c)));
    const denominator = 1 + dot(a, b) + dot(b, c) + dot(c, a);
    return 2 * Math.atan2(numerator, denominator);
};

/** Sum the fan of spherical triangles from the centre to each corner edge. */
const sphericalPolygonArea = (centre: V3, corners: V3[]): number => {
    let total = 0;
    for (let k = 0; k < corners.length; k++) {
        const next = corners[(k + 1) % corners.length];
        total += sphericalTriangleArea(centre, corners[k], next);
    }
    return total;
};
