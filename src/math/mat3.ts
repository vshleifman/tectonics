import type { Vec3 } from "./vec3";

/**
 * A symmetric 3x3 matrix, stored as its six distinct entries in the order
 * `xx, yy, zz, xy, xz, yz`.
 *
 * The only matrix this simulation needs is the plate drag tensor: every force
 * that resists a plate's motion is linear in its angular velocity, so the whole
 * resistance of a plate collapses into one symmetric matrix `M` with the balance
 * `M omega = torque`. Symmetric because each contribution is either an outer
 * product `v v^T` or `I - v v^T`, and both are.
 */
export type Sym3 = [number, number, number, number, number, number];

export const zeroSym3 = (): Sym3 => [0, 0, 0, 0, 0, 0];

/**
 * Add `k * (I - v v^T)` to `m` in place, for a unit `v`.
 *
 * This is the torque a drag force `-k * velocity` at position `v` exerts:
 * `v x (omega x v)` expands to `omega - v (v . omega)`, which is exactly this
 * matrix applied to omega. It is the resistance of a patch of plate being
 * dragged through the mantle.
 */
export const addDragAt = (m: Sym3, v: Vec3, k: number): void => {
  m[0] += k * (1 - v[0] * v[0]);
  m[1] += k * (1 - v[1] * v[1]);
  m[2] += k * (1 - v[2] * v[2]);
  m[3] -= k * v[0] * v[1];
  m[4] -= k * v[0] * v[2];
  m[5] -= k * v[1] * v[2];
};

/**
 * Add `k * g g^T` to `m` in place.
 *
 * This is a resistance that acts along one direction only: a force opposing
 * motion along `g` and indifferent to everything else, which is what a locked
 * plate margin is — it stops the plates closing across it and lets them slide
 * along it. Being an outer product it is positive semi-definite, so it can only
 * ever remove energy: unlike a restoring force it cannot overshoot and set the
 * plate oscillating.
 */
export const addAxisResistance = (m: Sym3, g: Vec3, k: number): void => {
  m[0] += k * g[0] * g[0];
  m[1] += k * g[1] * g[1];
  m[2] += k * g[2] * g[2];
  m[3] += k * g[0] * g[1];
  m[4] += k * g[0] * g[2];
  m[5] += k * g[1] * g[2];
};

/** Add `k * v v^T` to `m` in place. See {@link dominantAxis} for what it is for. */
export const addOuterProduct = (m: Sym3, v: Vec3, k: number): void =>
  addAxisResistance(m, v, k);

/**
 * Solve `m x = b` by Cramer's rule, returning the zero vector where `m` is too
 * near singular to trust — which happens only for a plate small enough that its
 * cells are effectively one point, and which should not be moving anyway.
 */
export const solveSym3 = (m: Sym3, b: Vec3): Vec3 => {
  const [xx, yy, zz, xy, xz, yz] = m;
  // Cofactors of the symmetric matrix.
  const c00 = yy * zz - yz * yz;
  const c01 = xz * yz - xy * zz;
  const c02 = xy * yz - xz * yy;
  const det = xx * c00 + xy * c01 + xz * c02;
  // Scale-free singularity test: compare against the matrix's own magnitude, so
  // it holds whatever units the drag coefficient is expressed in.
  const scale = Math.abs(xx) + Math.abs(yy) + Math.abs(zz);
  if (
    !Number.isFinite(det) ||
    Math.abs(det) < 1e-12 * (scale * scale * scale + 1)
  ) {
    return [0, 0, 0];
  }
  const c11 = xx * zz - xz * xz;
  const c12 = xy * xz - xx * yz;
  const c22 = xx * yy - xy * xy;
  return [
    (c00 * b[0] + c01 * b[1] + c02 * b[2]) / det,
    (c01 * b[0] + c11 * b[1] + c12 * b[2]) / det,
    (c02 * b[0] + c12 * b[1] + c22 * b[2]) / det,
  ];
};

/**
 * The axis along which `m` is largest, by power iteration.
 *
 * Used to read an extension direction out of the forces pulling on a plate:
 * summing the outer products `F F^T` of the pulls builds a matrix whose dominant
 * axis is the line along which the plate is being stretched hardest, regardless
 * of which end each pull acts from. Ten iterations is ample — the matrix is 3x3
 * and only its direction is wanted.
 */
export const dominantAxis = (m: Sym3, seed: Vec3): Vec3 => {
  const [xx, yy, zz, xy, xz, yz] = m;
  let v: Vec3 = seed;
  for (let i = 0; i < 10; i++) {
    const x = xx * v[0] + xy * v[1] + xz * v[2];
    const y = xy * v[0] + yy * v[1] + yz * v[2];
    const z = xz * v[0] + yz * v[1] + zz * v[2];
    const len = Math.hypot(x, y, z);
    if (len < 1e-12) return v;
    v = [x / len, y / len, z / len];
  }
  return v;
};
