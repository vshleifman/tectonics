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
import { type PlateData, sutureKey } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";
import { tuning } from "./tuning";

/**
 * The force/motion tuning constants (reference density, term weights, speed gain
 * and cap, and the relaxation factor) now live in {@link tuning} so the UI can
 * tweak them live; they are read inside {@link updatePlateMotion} below.
 */

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

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
 * - **Suture coupling** — plates welded at a continental suture are pulled onto
 *   a shared angular velocity. Welded crust cannot move two ways at once, so a
 *   fused pair travels as one and the seam between them cannot reopen.
 * - **Collision resistance** — a convergent boundary pushes back on both plates,
 *   in proportion to how fast they are closing. Where a slab can sink the
 *   resistance is slight, but two continents cannot get out of each other's way
 *   and the margin locks ({@link Tuning.collisionLockGain}). This is the only
 *   term that opposes motion: without it the balance is all drive and no
 *   reaction, and colliding plates plough on at undiminished speed forever.
 *   Because it acts at the boundary rather than through the plate's centre it
 *   does more than brake -- it swings the Euler pole, so a plate that runs into
 *   a continent tends to turn and slide along the collision instead of through
 *   it.
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
  dtMyr: number = tuning.dtMyr,
): void => {
  const torque = new Map<number, [number, number, number]>();
  const centroidSum = new Map<number, [number, number, number]>();
  const densePosSum = new Map<number, [number, number, number]>();
  /** Direction in omega-space that drives a plate further into a locked margin. */
  const blocked = new Map<number, [number, number, number]>();
  /** Per plate: lock-weighted and plain arc length of its closing margins. */
  const lockSum = new Map<number, number>();
  const arcSum = new Map<number, number>();
  for (const id of plateData.byId.keys()) {
    torque.set(id, [0, 0, 0]);
    centroidSum.set(id, [0, 0, 0]);
    densePosSum.set(id, [0, 0, 0]);
    blocked.set(id, [0, 0, 0]);
    lockSum.set(id, 0);
    arcSum.set(id, 0);
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
    const excess = data.density[i] - tuning.densityRef;
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
    tau[0] += tuning.intrinsicWeight * t[0];
    tau[1] += tuning.intrinsicWeight * t[1];
    tau[2] += tuning.intrinsicWeight * t[2];
  }

  // Pass 2: boundary refinements (slab pull / ridge push) per active arc.
  const { arcCells, arcCount, arcEnds, junctionPos } = mesh.boundaryGraph();
  for (let a = 0; a < arcCount; a++) {
    const kind = boundaries.arcKind[a];
    // Every inter-plate arc is examined, not only the ones currently reading as
    // convergent or divergent. Slab pull below is a buoyancy force, not a
    // consequence of motion, and gating it on the boundary's kind made the whole
    // force balance velocity-dependent: stall the plates and the boundaries all
    // classify as NONE, every driving term vanishes, and nothing can start them
    // again. That is a deadlock a world can never leave.
    if (kind === BoundaryKind.TRANSFORM) continue;
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

    {
      // Slab pull: the denser side of the boundary hangs into the mantle and
      // drags its plate toward the lighter one. Weighted by the density
      // *contrast* across the boundary rather than by absolute density, so a
      // ridge -- young crust of equal density on both sides -- pulls on nothing,
      // while old cold sea floor meeting a continent pulls hard. That contrast
      // is there whether or not the plates happen to be moving, which is what
      // lets a stalled world start itself again.
      const aDenser = data.density[cellA] >= data.density[cellB];
      const subPlate = aDenser ? plateA : plateB;
      const subPos = aDenser ? posA : posB;
      const otherPos = aDenser ? posB : posA;
      const contrast = Math.abs(data.density[cellA] - data.density[cellB]);
      if (contrast > 0) {
        const dir = normalise(tangent(sub(otherPos, subPos), mid));
        addForce(
          torque,
          subPlate,
          mid,
          dir,
          tuning.slabWeight * contrast * arcLen,
        );
      }
    }

    if (kind === BoundaryKind.CONVERGENT) {
      // Collision resistance. Not a force: a force strong enough to stop a
      // collision is also strong enough to overshoot and fling the plate back,
      // and the pole then thrashes from tick to tick. A locked margin is a
      // constraint -- it removes the plate's freedom to move into it, and
      // nothing more. Recorded here as the direction in omega-space that would
      // increase convergence across this arc, so it can be damped out below.
      const closing = boundaries.arcClosing[a];
      // A sutured pair is already held to a common motion, so there is no
      // convergence left to cancel. Constraining it as well was double-counting,
      // and it took the plate's motion with it.
      if (closing > 0 && !plateData.sutures.has(sutureKey(plateA, plateB))) {
        const contA = data.crustType[cellA] === CONTINENTAL;
        const contB = data.crustType[cellB] === CONTINENTAL;
        // Two continents cannot get out of each other's way. A trench, by
        // contrast, is not a wall at all -- the slab sinks and the plates keep
        // closing -- so it resists only slightly. Weighting them alike is what
        // stopped subduction: the constraint removed the convergent part of
        // every plate's motion, leaving only the part running along its
        // boundaries, and the whole world settled into strike-slip.
        const lock =
          contA && contB
            ? 1
            : contA || contB
              ? 1 / Math.sqrt(tuning.collisionLockGain)
              : 1 / tuning.collisionLockGain;
        const toB = normalise(tangent(sub(posB, posA), mid));
        // d(closing)/d(omega) for plate A is mid x n, and the exact opposite
        // for B, since closing counts both plates' motion along the same normal.
        const g = cross(mid, toB);
        const w = lock * arcLen;
        for (const side of [plateA, plateB]) {
          lockSum.set(side, (lockSum.get(side) ?? 0) + w);
          arcSum.set(side, (arcSum.get(side) ?? 0) + arcLen);
        }
        const bA = blocked.get(plateA);
        if (bA) {
          bA[0] += g[0] * w;
          bA[1] += g[1] * w;
          bA[2] += g[2] * w;
        }
        const bB = blocked.get(plateB);
        if (bB) {
          bB[0] -= g[0] * w;
          bB[1] -= g[1] * w;
          bB[2] -= g[2] * w;
        }
      }
    } else {
      // Ridge push: shove each plate away from the ridge axis at the midpoint.
      const dirA = normalise(tangent(sub(posA, mid), mid));
      addForce(torque, plateA, mid, dirA, tuning.ridgeWeight * arcLen);
      const dirB = normalise(tangent(sub(posB, mid), mid));
      addForce(torque, plateB, mid, dirB, tuning.ridgeWeight * arcLen);
    }
  }

  // Map each plate's net torque to a capped speed and relax `omega` toward it.
  for (const [id, props] of plateData.byId) {
    const tau = torque.get(id) as [number, number, number];
    const mag = length(tau);
    let target: Vec3 = [0, 0, 0];
    if (mag >= EPS) {
      const speed = Math.min(tuning.speedGain * mag, tuning.maxSpeed);
      const axis = normalise(tau);
      target = [axis[0] * speed, axis[1] * speed, axis[2] * speed];
    }
    props.omega[0] += tuning.motionRelax * (target[0] - props.omega[0]);
    props.omega[1] += tuning.motionRelax * (target[1] - props.omega[1]);
    props.omega[2] += tuning.motionRelax * (target[2] - props.omega[2]);

    // Apply the collision constraint: strip out the part of the motion that
    // drives the plate further into the margins it is locked against, leaving
    // the rest untouched. A plate that runs into a continent therefore slows,
    // and what survives is the component along the collision -- so it also
    // turns, and ends up sliding past rather than ploughing through.
    const block = blocked.get(id) as [number, number, number];
    const blockMag = length(block);
    if (blockMag < EPS) continue;
    const axis: Vec3 = [
      block[0] / blockMag,
      block[1] / blockMag,
      block[2] / blockMag,
    ];
    const into = dot(props.omega, axis);
    if (into <= 0) continue;
    // What share of this plate's closing margin is actually locked: 1 where it
    // is all continent against continent, near 0 where it is all trench. Only
    // that share of the plate's convergence is held back, so a plate colliding
    // along one edge keeps subducting along the others.
    const arcs = arcSum.get(id) ?? 0;
    if (arcs <= 0) continue;
    const lockedShare = Math.min(1, Math.max(0, (lockSum.get(id) ?? 0) / arcs));
    const damp =
      Math.min(1, Math.max(0, tuning.collisionWeight)) * lockedShare * into;
    if (damp <= 0) continue;
    props.omega[0] -= damp * axis[0];
    props.omega[1] -= damp * axis[1];
    props.omega[2] -= damp * axis[2];
  }

  // Suture coupling: welded plates converge onto a common motion. Applied after
  // the force balance so it is the last word -- crust joined across a suture is
  // one sheet, whatever the two halves' own force balances would prefer.
  const blend = Math.min(1, Math.max(0, tuning.sutureCoupling * dtMyr));
  if (blend > 0) {
    for (const key of plateData.sutures.keys()) {
      const a = plateData.byId.get(Math.floor(key / 1e6));
      const b = plateData.byId.get(key % 1e6);
      if (!a || !b) continue;
      for (let k = 0; k < 3; k++) {
        const mean = 0.5 * (a.omega[k] + b.omega[k]);
        a.omega[k] += blend * (mean - a.omega[k]);
        b.omega[k] += blend * (mean - b.omega[k]);
      }
    }
  }
};
