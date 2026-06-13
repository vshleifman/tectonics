/** Constrain `x` to the inclusive range [`min`, `max`]. */
export const clamp = (x: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, x));

/** Constrain `x` to the inclusive range [0, 1]. */
export const clamp01 = (x: number): number => clamp(x, 0, 1);

/** Linear interpolation between `a` and `b` by `t` (unclamped). */
export const lerp = (a: number, b: number, t: number): number =>
  a + (b - a) * t;
