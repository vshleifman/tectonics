import type { CellData } from "../data/CellData";
import {
  addAxisResistance,
  addDragAt,
  addOuterProduct,
  dominantAxis,
  type Sym3,
  solveSym3,
  zeroSym3,
} from "../math/mat3";
import { clamp01 } from "../math/scalar";
import { cross, normalise, sub, tangent, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { arcLength, type BoundaryField, BoundaryKind } from "./boundaries";
import { tuning } from "./tuning";

/** Crust type code mirrored from {@link CellData.crustType}. */
const CONTINENTAL = 1;

/**
 * Solve each plate's angular velocity from a force balance over the crust it
 * carries and the boundaries around it.
 *
 * A plate has no useful inertia — the mantle it moves through is vastly more
 * resistant than the plate is massive — so it is not accelerated by a net force,
 * it moves at whatever speed makes the driving and resisting forces cancel.
 * Everything that resists is linear in the plate's angular velocity, so the
 * whole balance collapses to `M omega = torque` for a symmetric drag matrix `M`,
 * solved directly. There is no speed cap, no torque-to-speed gain and no
 * relaxation factor: a plate's speed is an *output* of the balance, which is the
 * whole point. If a plate comes out too fast, some force is wrong.
 *
 * **Driving** (into the torque):
 *
 * - **Slab pull.** The dominant driver, and the reason plate speeds vary as much
 *   as they do. Oceanic lithosphere cools and thickens with age until it is
 *   denser than the mantle beneath it, and the sheet hanging into the mantle at
 *   a trench pulls the rest of the plate along behind it. The pull per unit of
 *   trench therefore grows with the *age* of the crust going down — young crust
 *   at a trench pulls on nothing — which is why the plates with old, long slabs
 *   (Pacific, Nazca, Cocos) are the fast ones.
 * - **Ridge push.** A plate slides off the elevated ridge it was born at. Real,
 *   and roughly an order of magnitude weaker than slab pull. Scaled here by the
 *   relief the elevation model actually built, so it grows as the ridge flank
 *   deepens away from the axis.
 *
 * **Resisting** (into the drag matrix, never the torque — a resistance written
 * as a force can overshoot and set a plate oscillating; as drag it cannot):
 *
 * - **Mantle drag** under the whole plate, so a big plate is harder to move than
 *   a small one, and a plate with a deep continental keel harder still. Together
 *   with slab pull this reproduces the Forsyth & Uyeda result that plate speed
 *   tracks how much trench a plate has and barely tracks its area.
 * - **Collision resistance** at convergent margins, acting only across the
 *   boundary. Where a slab can sink this is slight; where two continents meet
 *   neither can get out of the way and the margin locks. Because it resists only
 *   the closing direction, a plate that runs into a continent keeps whatever
 *   motion runs *along* the collision: it slows, and it turns to slide past.
 *
 * Finally, plates welded at a suture are pulled onto a common angular velocity:
 * crust joined across a suture is one sheet whatever the two halves' own
 * balances would prefer.
 *
 * Pure over the mesh interface, flat typed arrays, {@link PlateData} and
 * {@link BoundaryField}; mutates each plate's `omega` in place.
 */
export const updatePlateMotion = (
  mesh: CellMesh,
  data: CellData,
  plateData: PlateData,
  boundaries: BoundaryField,
  dtMyr: number = tuning.dtMyr,
): void => {
  const torque = new Map<number, [number, number, number]>();
  const drag = new Map<number, Sym3>();
  /** Per plate: the vector sum of its slab pulls, and the sum of their sizes. */
  const pullSum = new Map<number, [number, number, number]>();
  const pullMagnitude = new Map<number, number>();
  /**
   * Per plate: the outer products of its pulls, whose dominant axis is the line
   * the plate is being stretched along.
   */
  const pullSpread = new Map<number, Sym3>();
  const area = new Map<number, number>();
  for (const id of plateData.byId.keys()) {
    torque.set(id, [0, 0, 0]);
    drag.set(id, zeroSym3());
    pullSum.set(id, [0, 0, 0]);
    pullMagnitude.set(id, 0);
    pullSpread.set(id, zeroSym3());
    area.set(id, 0);
  }

  // --- Resistance under the plate ------------------------------------------
  // Every cell drags on the mantle beneath it. Continental crust drags harder:
  // a craton carries a lithospheric keel a couple of hundred kilometres deep,
  // while oceanic lithosphere is thin. This is why the continent-heavy plates
  // (Eurasia, Africa, North America) are the slow ones.
  for (let i = 0; i < mesh.cellCount; i++) {
    const m = drag.get(data.plateId[i]);
    if (!m) continue;
    const keel = data.crustType[i] === CONTINENTAL ? tuning.continentalDrag : 1;
    addDragAt(m, mesh.position(i), tuning.mantleDrag * mesh.area(i) * keel);
    area.set(data.plateId[i], (area.get(data.plateId[i]) ?? 0) + mesh.area(i));
  }

  // --- Boundary forces ------------------------------------------------------
  const { arcCells, arcCount } = mesh.boundaryGraph();
  for (let a = 0; a < arcCount; a++) {
    const cellA = arcCells[a * 2];
    const cellB = arcCells[a * 2 + 1];
    if (cellA < 0 || cellB < 0) continue;
    const plateA = data.plateId[cellA];
    const plateB = data.plateId[cellB];
    if (plateA < 0 || plateB < 0 || plateA === plateB) continue;

    const kind = boundaries.arcKind[a];
    const posA = mesh.position(cellA);
    const posB = mesh.position(cellB);
    const mid = normalise([
      posA[0] + posB[0],
      posA[1] + posB[1],
      posA[2] + posB[2],
    ]);
    const arcLen = arcLength(mesh, a);
    const contA = data.crustType[cellA] === CONTINENTAL;
    const contB = data.crustType[cellB] === CONTINENTAL;

    if (kind === BoundaryKind.DIVERGENT) {
      // Ridge push: each plate slides off the ridge, driven by the relief
      // between the crest at the axis and the deeper floor behind it.
      for (const [plate, cell, pos, other] of [
        [plateA, cellA, posA, posB],
        [plateB, cellB, posB, posA],
      ] as const) {
        // How far this side has already subsided below the crest it was born
        // at: the slope the plate is sliding down.
        const relief = tuning.ridgeElevation - data.elevation[cell];
        if (relief <= 0) continue;
        const dir = normalise(tangent(sub(pos, other), mid));
        addTorque(
          torque,
          plate,
          mid,
          dir,
          tuning.ridgeWeight * relief * arcLen,
        );
      }
      continue;
    }

    // Slab pull. Deliberately not gated on the boundary reading as convergent
    // *now*: a slab's weight is there whether or not the plates happen to be
    // moving, and gating on motion makes the whole balance self-referential —
    // stall the world and every boundary classifies as inactive, every force
    // vanishes, and nothing can ever start it again.
    if (contA && contB) {
      // Two continents: nothing can sink, so there is no slab to pull.
    } else {
      // The side that goes down is the one that cannot stay up.
      let downCell = cellA;
      let downPlate = plateA;
      let upPos = posB;
      if (contA || (!contB && data.density[cellB] > data.density[cellA])) {
        downCell = cellB;
        downPlate = plateB;
        upPos = posA;
      }
      const pull = slabPull(data.age[downCell]);
      if (pull > 0) {
        const downPos = downCell === cellA ? posA : posB;
        const dir = normalise(tangent(sub(upPos, downPos), mid));
        const force = tuning.slabWeight * pull * arcLen;
        addTorque(torque, downPlate, mid, dir, force);
        recordPull(pullSum, pullMagnitude, pullSpread, downPlate, dir, force);
      }
    }

    if (kind !== BoundaryKind.CONVERGENT) continue;

    // Collision resistance, as drag across the boundary only. A trench barely
    // resists — the slab sinks and the plates keep closing. A continent-continent
    // margin resists fully: neither side can be disposed of.
    const lock =
      contA && contB
        ? 1
        : contA || contB
          ? 1 / Math.sqrt(tuning.collisionLockGain)
          : 1 / tuning.collisionLockGain;
    // `g` is the axis in omega-space that closes this boundary, so resisting
    // along it is exactly resisting convergence and nothing else.
    const normal = normalise(tangent(sub(posB, posA), mid));
    const g = cross(mid, normal);
    const k = tuning.collisionDrag * lock * arcLen;
    const mA = drag.get(plateA);
    if (mA) addAxisResistance(mA, g, k);
    const mB = drag.get(plateB);
    if (mB) addAxisResistance(mB, g, k);
  }

  // --- What is left over as tension -----------------------------------------
  // Pulls that cancel in the vector sum do not move the plate; they stretch it.
  // A plate towed by a single slab is under no tension however fast it goes,
  // while one pulled from opposite edges is being torn even if it sits still.
  plateData.stress.clear();
  for (const id of plateData.byId.keys()) {
    const total = pullMagnitude.get(id) ?? 0;
    const net = pullSum.get(id) as [number, number, number];
    const opposed = total - Math.hypot(net[0], net[1], net[2]);
    const plateArea = area.get(id) ?? 0;
    if (opposed <= 0 || plateArea <= 0) continue;
    plateData.stress.set(id, {
      tension: opposed / plateArea,
      axis: dominantAxis(pullSpread.get(id) as Sym3, [1, 0, 0]),
    });
  }

  // --- Solve the balance ----------------------------------------------------
  for (const [id, props] of plateData.byId) {
    const omega = solveSym3(
      drag.get(id) as Sym3,
      torque.get(id) as [number, number, number],
    );
    props.omega[0] = omega[0];
    props.omega[1] = omega[1];
    props.omega[2] = omega[2];
  }

  // Suture coupling: welded plates converge onto a common motion. Applied after
  // the balance so it is the last word -- crust joined across a suture is one
  // sheet, whatever the two halves' own force balances would prefer.
  const blend = clamp01(tuning.sutureCoupling * dtMyr);
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

/**
 * Pull per unit length of trench from crust of the given age, in [0, 1].
 *
 * Oceanic lithosphere is born buoyant and cools into negative buoyancy: the
 * thermal boundary layer thickens with age until the column outweighs the mantle
 * it floats on, somewhere around {@link Tuning.slabPullOnsetAge}. Past that the
 * excess mass grows with age, and stops growing once the plate has cooled
 * through ({@link Tuning.slabPullSaturationAge}) — a slab does not keep getting
 * heavier forever. Young crust arriving at a trench pulls on nothing, which is
 * why subducting a ridge kills the plate that was riding it.
 */
const slabPull = (age: number): number => {
  const onset = tuning.slabPullOnsetAge;
  const span = Math.max(1e-6, tuning.slabPullSaturationAge - onset);
  return clamp01((age - onset) / span);
};

/** Tally one slab pull into the running totals the tension is read from. */
const recordPull = (
  sum: Map<number, [number, number, number]>,
  magnitude: Map<number, number>,
  spread: Map<number, Sym3>,
  plate: number,
  dir: Vec3,
  force: number,
): void => {
  const s = sum.get(plate);
  if (!s) return;
  s[0] += dir[0] * force;
  s[1] += dir[1] * force;
  s[2] += dir[2] * force;
  magnitude.set(plate, (magnitude.get(plate) ?? 0) + force);
  addOuterProduct(spread.get(plate) as Sym3, dir, force);
};

/** Accumulate a tangential force at `mid` into plate `plate`'s torque. */
const addTorque = (
  torque: Map<number, [number, number, number]>,
  plate: number,
  mid: Vec3,
  dir: Vec3,
  mag: number,
): void => {
  const tau = torque.get(plate);
  if (!tau) return;
  const t = cross(mid, [dir[0] * mag, dir[1] * mag, dir[2] * mag]);
  tau[0] += t[0];
  tau[1] += t[1];
  tau[2] += t[2];
};
