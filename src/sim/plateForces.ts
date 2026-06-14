import type { CellData } from "../data/CellData";
import {
    cross,
    dot,
    length,
    normalise,
    readVec3,
    scale,
    sub,
    tangent,
    type Vec3,
} from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";

/**
 * Reference crust density (g/cm3). Only crust denser than this — cooled,
 * subduction-prone oceanic crust — contributes a pull; continental crust
 * (~2.7) sits at the reference and so drives no intrinsic motion.
 */
const DENSITY_REF = 2.7;

/** Relative weight of the intrinsic density-distribution drive (the bootstrap). */
const INTRINSIC_WEIGHT = 1.0;

/** Relative weight of slab pull at convergent boundaries. */
const SLAB_WEIGHT = 0.5;

/** Relative weight of ridge push at divergent boundaries (set 0 to disable). */
const RIDGE_WEIGHT = 0.1;

/** Maps net torque magnitude to angular speed (rad/Myr per unit torque). */
const SPEED_GAIN = 0.03;

/** Hard cap on a plate's angular speed (rad/Myr), keeping the old MAX_SPEED feel. */
const MAX_SPEED = 0.02;

/**
 * Per-step relaxation factor: how far `omega` moves toward its force target each
 * tick. Below 1 it smooths motion so plates do not jitter as boundaries flicker.
 */
const RELAX = 0.2;

/** Torque magnitudes below this leave a plate effectively stationary (no NaN). */
const EPS = 1e-9;

/** Accumulate a tangential force at `mid` into plate `plate`'s torque. */
const addForce = (
    torque: Map<number, [number, number, number]>,
    plate: number,
    mid: Vec3,
    dir: Vec3,
    mag: number,
): void => {
    const tau = torque.get(plate);
    if (!tau) return;
    const t = cross(mid, scale(dir, mag));
    tau[0] += t[0];
    tau[1] += t[1];
    tau[2] += t[2];
};

/**
 * Derive each plate's angular velocity `omega` from a force balance over the
 * crust it carries, replacing the old random Euler poles. Three terms sum into a
 * net torque per plate (an axis through the sphere centre):
 *
 * - **Intrinsic density drive** — a plate drifts toward where its dense crust
 *   concentrates. Integrating `pos * excessDensity` over the plate gives a
 *   density-weighted position whose tangential offset from the centroid is the
 *   direction to turn toward. This needs no boundaries, so it bootstraps motion
 *   from rest with no deadlock.
 * - **Slab pull** — at convergent boundaries the denser (subducting) edge is
 *   pulled toward the overriding plate, weighted by its excess density.
 * - **Ridge push** — at divergent boundaries each plate is shoved away from the
 *   ridge axis (a weaker term).
 *
 * The net torque is mapped to a capped angular speed and the plate's `omega` is
 * relaxed toward it (smoothing). Pure over the mesh interface, flat typed arrays,
 * {@link PlateData} and {@link BoundaryField}; mutates each plate's `omega` in
 * place, so it can later move into a Worker / WASM.
 */
export const updatePlateMotion = (
    mesh: CellMesh,
    data: CellData,
    plateData: PlateData,
    boundaries: BoundaryField,
): void => {
    const torque = new Map<number, [number, number, number]>();
    const centroidSum = new Map<number, [number, number, number]>();
    const densePosSum = new Map<number, [number, number, number]>();
    for (const id of plateData.byId.keys()) {
        torque.set(id, [0, 0, 0]);
        centroidSum.set(id, [0, 0, 0]);
        densePosSum.set(id, [0, 0, 0]);
    }

    // Pass 1: integrate centroid and density-weighted position over each plate.
    for (let i = 0; i < mesh.cellCount; i++) {
        const id = data.plateId[i];
        const cSum = centroidSum.get(id);
        if (!cSum) continue;
        const pos = mesh.position(i);
        cSum[0] += pos[0];
        cSum[1] += pos[1];
        cSum[2] += pos[2];
        const excess = data.density[i] - DENSITY_REF;
        if (excess > 0) {
            const w = excess * mesh.area(i);
            const dSum = densePosSum.get(id) as [number, number, number];
            dSum[0] += pos[0] * w;
            dSum[1] += pos[1] * w;
            dSum[2] += pos[2] * w;
        }
    }

    // Intrinsic drive: torque that turns the plate's centroid toward its dense side.
    for (const id of plateData.byId.keys()) {
        const C = normalise(centroidSum.get(id) as [number, number, number]);
        const dSum = densePosSum.get(id) as [number, number, number];
        const t = cross(C, dSum);
        const tau = torque.get(id) as [number, number, number];
        tau[0] += INTRINSIC_WEIGHT * t[0];
        tau[1] += INTRINSIC_WEIGHT * t[1];
        tau[2] += INTRINSIC_WEIGHT * t[2];
    }

    // Pass 2: boundary refinements (slab pull / ridge push) per active arc.
    const { arcCells, arcCount, arcEnds, junctionPos } = mesh.boundaryGraph();
    for (let a = 0; a < arcCount; a++) {
        const kind = boundaries.arcKind[a];
        if (
            kind !== BoundaryKind.CONVERGENT &&
            kind !== BoundaryKind.DIVERGENT
        ) {
            continue;
        }
        const cellA = arcCells[a * 2];
        const cellB = arcCells[a * 2 + 1];
        if (cellA < 0 || cellB < 0) continue;
        const plateA = data.plateId[cellA];
        const plateB = data.plateId[cellB];
        if (plateA < 0 || plateB < 0 || plateA === plateB) continue;

        const posA = mesh.position(cellA);
        const posB = mesh.position(cellB);
        const mid = normalise([
            posA[0] + posB[0],
            posA[1] + posB[1],
            posA[2] + posB[2],
        ]);

        // Arc length on the unit sphere: the great-circle angle between its
        // junctions, so the per-arc forces sum to a resolution-independent
        // line integral along the boundary.
        const j0 = readVec3(junctionPos, arcEnds[a * 2]);
        const j1 = readVec3(junctionPos, arcEnds[a * 2 + 1]);
        const arcLen = Math.acos(Math.max(-1, Math.min(1, dot(j0, j1))));

        if (kind === BoundaryKind.CONVERGENT) {
            // The denser cell subducts and is pulled toward the overriding plate.
            const aDenser = data.density[cellA] >= data.density[cellB];
            const subPlate = aDenser ? plateA : plateB;
            const subCell = aDenser ? cellA : cellB;
            const subPos = aDenser ? posA : posB;
            const otherPos = aDenser ? posB : posA;
            const excess = data.density[subCell] - DENSITY_REF;
            if (excess > 0) {
                const dir = normalise(tangent(sub(otherPos, subPos), mid));
                addForce(
                    torque,
                    subPlate,
                    mid,
                    dir,
                    SLAB_WEIGHT * excess * arcLen,
                );
            }
        } else {
            // Ridge push: shove each plate away from the ridge axis at the midpoint.
            const dirA = normalise(tangent(sub(posA, mid), mid));
            addForce(torque, plateA, mid, dirA, RIDGE_WEIGHT * arcLen);
            const dirB = normalise(tangent(sub(posB, mid), mid));
            addForce(torque, plateB, mid, dirB, RIDGE_WEIGHT * arcLen);
        }
    }

    // Map each plate's net torque to a capped speed and relax `omega` toward it.
    for (const [id, props] of plateData.byId) {
        const tau = torque.get(id) as [number, number, number];
        const mag = length(tau);
        let target: Vec3 = [0, 0, 0];
        if (mag >= EPS) {
            const speed = Math.min(SPEED_GAIN * mag, MAX_SPEED);
            const axis = normalise(tau);
            target = [axis[0] * speed, axis[1] * speed, axis[2] * speed];
        }
        props.omega[0] += RELAX * (target[0] - props.omega[0]);
        props.omega[1] += RELAX * (target[1] - props.omega[1]);
        props.omega[2] += RELAX * (target[2] - props.omega[2]);
    }
};
