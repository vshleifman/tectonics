/**
 * A point or direction on (or near) the unit sphere, stored as a plain tuple so
 * it ports cleanly to flat typed arrays / WASM without object overhead.
 *
 * Operations return fresh tuples and never mutate their inputs, so a `Vec3` can
 * be treated as an immutable value.
 */
export type Vec3 = readonly [number, number, number];

export const length = (v: Vec3): number => Math.hypot(v[0], v[1], v[2]);

/** Unit vector in the direction of `v`; the zero vector is returned unchanged. */
export const normalise = (v: Vec3): Vec3 => {
  const len = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / len, v[1] / len, v[2] / len];
};

export const add = (a: Vec3, b: Vec3): Vec3 => [
  a[0] + b[0],
  a[1] + b[1],
  a[2] + b[2],
];

export const sub = (a: Vec3, b: Vec3): Vec3 => [
  a[0] - b[0],
  a[1] - b[1],
  a[2] - b[2],
];

export const scale = (a: Vec3, s: number): Vec3 => [
  a[0] * s,
  a[1] * s,
  a[2] * s,
];

export const dot = (a: Vec3, b: Vec3): number =>
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

export const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

/**
 * Rotate `v` about a unit `axis` by `angle` radians (Rodrigues' formula). This
 * is the core of plate motion: a rigid plate moves by rotating its crust about
 * its Euler pole. `axis` is assumed to be a unit vector; a zero `angle` (or a
 * degenerate axis) returns `v` unchanged so callers can pass `omega * dt`
 * straight through without special-casing stationary plates.
 */
export const rotateAboutAxis = (v: Vec3, axis: Vec3, angle: number): Vec3 => {
  if (angle === 0) return v;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  const d = dot(axis, v);
  const c = cross(axis, v);
  // v*cos + (axis x v)*sin + axis*(axis.v)*(1 - cos)
  const k = d * (1 - cos);
  return [
    v[0] * cos + c[0] * sin + axis[0] * k,
    v[1] * cos + c[1] * sin + axis[1] * k,
    v[2] * cos + c[2] * sin + axis[2] * k,
  ];
};

/**
 * Component of `v` tangent to the unit sphere at `normal`, i.e. `v` with its
 * radial part removed. `normal` is assumed to be a unit vector.
 */
export const tangent = (v: Vec3, normal: Vec3): Vec3 => {
  const radial = dot(v, normal);
  return [
    v[0] - radial * normal[0],
    v[1] - radial * normal[1],
    v[2] - radial * normal[2],
  ];
};

/** Read the xyz triple stored at cell/vertex `index` of a flat array. */
export const readVec3 = (arr: ArrayLike<number>, index: number): Vec3 => {
  const o = index * 3;
  return [arr[o], arr[o + 1], arr[o + 2]];
};

/** Write `v` into the xyz triple at cell/vertex `index` of a flat array. */
export const writeVec3 = (
  target: { [i: number]: number },
  index: number,
  v: Vec3,
): void => {
  const o = index * 3;
  target[o] = v[0];
  target[o + 1] = v[1];
  target[o + 2] = v[2];
};
