import type { BoundaryGraph, CellMesh } from "../mesh/CellMesh";
import { makeRng } from "../data/CellData";

/** Tunable knobs for crack propagation. */
export interface PropagationOptions {
    /** Crack branches spawned from each seed (capped by junction degree). */
    branchesPerSeed: number;
    /**
     * Crack length as a multiple of sqrt(junctionCount), so cracks span a
     * consistent fraction of the globe at any subdivision level. ~1 is roughly
     * a great-circle's worth of arcs.
     */
    lengthScale: number;
    /** Heading randomness [0..1]; 0 grows perfectly straight cracks. */
    jitter: number;
    /**
     * How strongly cracks are pushed to keep spreading away from their seed
     * (relative to the straight-ahead term). Higher = more radial, less curly.
     */
    outwardBias: number;
    /** Deterministic RNG seed so a given click sequence reproduces. */
    seed: string;
}

export const DEFAULT_PROPAGATION: PropagationOptions = {
    branchesPerSeed: 3,
    lengthScale: 1.8,
    jitter: 0.6,
    outwardBias: 0.6,
    seed: "tectonics",
};

/**
 * Reject a candidate arc whose direction points back toward the seed by more
 * than this (dot with the outward heading). Keeps cracks from turning around.
 */
const BACKWARD_MIN = -0.15;

/** A candidate place to start a fault branch radiating from a seed. */
interface Launch {
    /** Junction the branch starts walking from. */
    from: number;
    /** Arc joining `from` back to the seed (cracked to keep the star joined). */
    connector: number;
    /** First arc the branch cracks, leading outward from `from`. */
    arc: number;
    /** Outward bearing as a tangent at the seed, for even spreading. */
    dir: [number, number, number];
}

/**
 * Player-driven fault network on the dual edge skeleton.
 *
 * A fault is a set of cracked arcs. Cracking an arc severs the link between the
 * cell pair it separates; plates are then the connected components of the cell
 * graph with cracked links removed. The propagation is a pure, data-oriented
 * walk over flat typed arrays so it can later move into a Web Worker / WASM.
 */
export class Plates {
    private readonly graph: BoundaryGraph;

    /** 1 where an arc is cracked, indexed by arc id. */
    private readonly crackedArcs: Uint8Array;

    /** Seed junctions in placement order (for markers / reproducibility). */
    private readonly seedJunctions: number[] = [];

    /** Resolution-scaled crack length budget (arcs per branch). */
    private readonly maxLength: number;

    /** Mutable copy of the tuning knobs (so branch count can change live). */
    private readonly options: PropagationOptions;

    private rng: () => number;

    constructor(
        private readonly mesh: CellMesh,
        options: PropagationOptions = DEFAULT_PROPAGATION,
    ) {
        this.options = { ...options };
        this.graph = mesh.boundaryGraph();
        this.crackedArcs = new Uint8Array(this.graph.arcCount);
        this.maxLength = Math.max(
            8,
            Math.round(
                options.lengthScale * Math.sqrt(this.graph.junctionCount),
            ),
        );
        this.rng = makeRng(options.seed);
    }

    get cracked(): Uint8Array {
        return this.crackedArcs;
    }

    get seeds(): readonly number[] {
        return this.seedJunctions;
    }

    /** Change how many branches future seeds spawn. */
    setBranchesPerSeed(count: number): void {
        this.options.branchesPerSeed = Math.max(1, Math.round(count));
    }

    /**
     * Drop a seed and grow its fault branches to completion.
     *
     * A junction only has three arcs, so to fan out more than three branches we
     * also launch from the seed's 1-ring of neighbour junctions (cracking the
     * short connector back to the seed so the star stays joined). Launches are
     * picked to spread evenly around the seed by bearing.
     */
    addSeed(junction: number): void {
        this.seedJunctions.push(junction);

        const launches = this.collectLaunches(junction);
        if (launches.length === 0) return;
        const count = Math.min(
            Math.max(1, this.options.branchesPerSeed),
            launches.length,
        );

        // Tangent basis at the seed, to spread target bearings evenly.
        const pos = this.graph.junctionPos;
        const sx = pos[junction * 3];
        const sy = pos[junction * 3 + 1];
        const sz = pos[junction * 3 + 2];
        const u = launches[0].dir;
        const v = normalise3(
            sy * u[2] - sz * u[1],
            sz * u[0] - sx * u[2],
            sx * u[1] - sy * u[0],
        );

        const used = new Uint8Array(launches.length);
        for (let i = 0; i < count; i++) {
            const theta = (2 * Math.PI * i) / count;
            const cos = Math.cos(theta);
            const sin = Math.sin(theta);
            const tx = u[0] * cos + v[0] * sin;
            const ty = u[1] * cos + v[1] * sin;
            const tz = u[2] * cos + v[2] * sin;

            let best = -1;
            let bestDot = -Infinity;
            for (let j = 0; j < launches.length; j++) {
                if (used[j]) continue;
                const d = launches[j].dir;
                const dot = d[0] * tx + d[1] * ty + d[2] * tz;
                if (dot > bestDot) {
                    bestDot = dot;
                    best = j;
                }
            }
            if (best < 0) break;
            used[best] = 1;

            const launch = launches[best];
            this.crackedArcs[launch.connector] = 1;
            if (this.crackedArcs[launch.arc]) continue;
            this.crackedArcs[launch.arc] = 1;
            const next = this.otherEnd(launch.arc, launch.from);
            this.growBranch(junction, launch.from, next, launch.arc);
        }
    }

    /**
     * Outward branch launches available around a seed: each arc of the seed's
     * neighbour junctions that leads further out, tagged with the connector arc
     * joining it back to the seed and its bearing (a tangent at the seed).
     */
    private collectLaunches(seed: number): Launch[] {
        const { junctionArcOffsets, junctionArcs } = this.graph;
        const launches: Launch[] = [];
        const cStart = junctionArcOffsets[seed];
        const cEnd = junctionArcOffsets[seed + 1];
        for (let c = cStart; c < cEnd; c++) {
            const connector = junctionArcs[c];
            const n = this.otherEnd(connector, seed);
            const nStart = junctionArcOffsets[n];
            const nEnd = junctionArcOffsets[n + 1];
            for (let k = nStart; k < nEnd; k++) {
                const arc = junctionArcs[k];
                if (arc === connector) continue;
                const m = this.otherEnd(arc, n);
                if (m === seed) continue;
                launches.push({
                    from: n,
                    connector,
                    arc,
                    dir: this.tangentToward(seed, m),
                });
            }
        }
        return launches;
    }

    /** Forget every fault and seed; the sphere becomes a single plate again. */
    clear(): void {
        this.crackedArcs.fill(0);
        this.seedJunctions.length = 0;
        this.rng = makeRng(this.options.seed);
    }

    /**
     * Flood-fill the cell graph, never crossing a cracked arc, and write the
     * resulting connected-component id into `out` (a raw label per cell, in
     * [0, plateCount)). Returns the plate count. Linear in cell count.
     *
     * These labels are renumbered on every call and carry no identity across
     * calls; {@link PlateRegistry.relabel} maps them onto stable ids. Keeping
     * the fill a pure component labeller is what makes the matcher testable.
     */
    assignPlates(out: Int32Array): number {
        const { cellCount } = this.mesh;
        const plateId = out;
        plateId.fill(-1);

        const stack: number[] = [];
        let plateCount = 0;
        for (let s = 0; s < cellCount; s++) {
            if (plateId[s] !== -1) continue;
            plateId[s] = plateCount;
            stack.length = 0;
            stack.push(s);
            while (stack.length > 0) {
                const cell = stack.pop() as number;
                const nbrs = this.mesh.neighbors(cell);
                const arcs = this.mesh.neighbourArcs(cell);
                for (let k = 0; k < nbrs.length; k++) {
                    const arc = arcs[k];
                    if (arc >= 0 && this.crackedArcs[arc]) continue; // severed
                    const j = nbrs[k];
                    if (plateId[j] === -1) {
                        plateId[j] = plateCount;
                        stack.push(j);
                    }
                }
            }
            plateCount++;
        }
        return plateCount;
    }

    /**
     * Walk a single crack outward from its seed `origin`. At each junction the
     * arc chosen blends "keep going straight" with a bias to keep spreading
     * away from the origin, and arcs that point back toward the origin are
     * rejected outright — so cracks fan out rather than curl back. Stops on
     * connecting to an existing fault, on a dead end, or at the length budget.
     */
    private growBranch(
        origin: number,
        prevJunction: number,
        junction: number,
        prevArc: number,
    ): void {
        const { junctionArcOffsets, junctionArcs } = this.graph;
        let cur = junction;
        let arcIn = prevArc;
        let heading = this.forwardHeading(prevJunction, cur);

        for (let step = 0; step < this.maxLength; step++) {
            // Joined an earlier fault at this junction -> stop (forms a T/Y).
            if (this.hasOtherCrackedArc(cur, arcIn)) break;

            // Direction that keeps moving away from the seed at this junction.
            const outward = this.forwardHeading(origin, cur);

            const start = junctionArcOffsets[cur];
            const end = junctionArcOffsets[cur + 1];
            let bestArc = -1;
            let bestNext = -1;
            let bestScore = -Infinity;
            for (let k = start; k < end; k++) {
                const arc = junctionArcs[k];
                if (arc === arcIn) continue;
                const next = this.otherEnd(arc, cur);
                const dir = this.tangentToward(cur, next);
                const radial = this.dotHeading(dir, outward);
                if (radial < BACKWARD_MIN) continue; // would turn back to origin
                const score =
                    this.dotHeading(dir, heading) +
                    this.options.outwardBias * radial +
                    this.options.jitter * (this.rng() - 0.5);
                if (score > bestScore) {
                    bestScore = score;
                    bestArc = arc;
                    bestNext = next;
                }
            }
            if (bestArc < 0) break; // dead end (all arcs head backward)

            // Crossing into an already-cracked arc connects the network; stop.
            if (this.crackedArcs[bestArc]) {
                this.crackedArcs[bestArc] = 1;
                break;
            }
            this.crackedArcs[bestArc] = 1;
            heading = this.forwardHeading(cur, bestNext);
            arcIn = bestArc;
            cur = bestNext;
        }
    }

    /** The endpoint of `arc` that is not `junction`. */
    private otherEnd(arc: number, junction: number): number {
        const a = this.graph.arcEnds[arc * 2];
        const b = this.graph.arcEnds[arc * 2 + 1];
        return a === junction ? b : a;
    }

    /** True if `junction` touches a cracked arc other than `exceptArc`. */
    private hasOtherCrackedArc(junction: number, exceptArc: number): boolean {
        const { junctionArcOffsets, junctionArcs } = this.graph;
        const start = junctionArcOffsets[junction];
        const end = junctionArcOffsets[junction + 1];
        for (let k = start; k < end; k++) {
            const arc = junctionArcs[k];
            if (arc !== exceptArc && this.crackedArcs[arc]) return true;
        }
        return false;
    }

    /** Unit tangent at junction `from` pointing along the geodesic toward `to`. */
    private tangentToward(from: number, to: number): [number, number, number] {
        const pos = this.graph.junctionPos;
        const fx = pos[from * 3];
        const fy = pos[from * 3 + 1];
        const fz = pos[from * 3 + 2];
        const tx = pos[to * 3];
        const ty = pos[to * 3 + 1];
        const tz = pos[to * 3 + 2];
        const d = tx * fx + ty * fy + tz * fz;
        return normalise3(tx - d * fx, ty - d * fy, tz - d * fz);
    }

    /**
     * Unit tangent at junction `to` continuing the geodesic that arrived from
     * `from` (i.e. the "keep going straight" direction at the new junction).
     */
    private forwardHeading(from: number, to: number): [number, number, number] {
        const pos = this.graph.junctionPos;
        const fx = pos[from * 3];
        const fy = pos[from * 3 + 1];
        const fz = pos[from * 3 + 2];
        const tx = pos[to * 3];
        const ty = pos[to * 3 + 1];
        const tz = pos[to * 3 + 2];
        const d = fx * tx + fy * ty + fz * tz;
        return normalise3(d * tx - fx, d * ty - fy, d * tz - fz);
    }

    private dotHeading(
        dir: readonly [number, number, number],
        heading: readonly [number, number, number],
    ): number {
        return dir[0] * heading[0] + dir[1] * heading[1] + dir[2] * heading[2];
    }
}

const normalise3 = (
    x: number,
    y: number,
    z: number,
): [number, number, number] => {
    const len = Math.hypot(x, y, z) || 1;
    return [x / len, y / len, z / len];
};
