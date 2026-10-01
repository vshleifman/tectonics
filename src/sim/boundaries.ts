/**
 * What kind of plate boundary an edge (or boundary cell) is, from the relative
 * velocity of the two plates across it. `NONE` covers both plate interiors and
 * inactive boundaries where the plates barely move relative to one another.
 *
 * The classification itself runs in the Rust simulation
 * (`sim-rs/src/boundaries.rs`); these codes are what its per-arc and per-cell
 * boundary fields contain.
 */
export const BoundaryKind = {
  NONE: 0,
  CONVERGENT: 1,
  DIVERGENT: 2,
  TRANSFORM: 3,
} as const;

export type BoundaryKindValue =
  (typeof BoundaryKind)[keyof typeof BoundaryKind];
