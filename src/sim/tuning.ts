/**
 * Central, live-editable home for every tunable simulation constant.
 *
 * These values were previously hard-coded `const`s scattered across the sim and
 * render modules (the "tuning sinks" in `elevation.ts`, `plateForces.ts`,
 * `crustLifecycle.ts`, etc.). They are gathered here into one mutable singleton
 * the simulation reads live each tick, so the UI can expose them as sliders and
 * tweaks take effect on the next step without a world rebuild. Pure
 * numerical-stability epsilons (e.g. the `EPS = 1e-9` guard in `plateForces.ts`)
 * are intentionally left in place — they are not creative knobs.
 *
 * All elevations are in km, rates per Myr, densities in g/cm3.
 */
export interface Tuning {
  // --- Elevation / relief (see elevation.ts) ---
  /** Shallow mid-ocean-ridge crest: where young (age 0) oceanic crust sits. */
  ridgeElevation: number;
  /** Deep abyssal plain: where fully cooled oceanic crust sinks. */
  abyssElevation: number;
  /** Baseline elevation of unstressed continental crust at reference thickness. */
  continentBase: number;
  /** Reference continental thickness (km) above which extra crust adds height. */
  continentThicknessRef: number;
  /** Height (km) gained per km of continental crust above the reference. */
  continentThicknessGain: number;
  /** Uplift gain for a continent-continent collision (both sides pile up). */
  collisionUplift: number;
  /** Uplift gain for the buoyant overriding plate at a subduction zone. */
  arcUplift: number;
  /** Deepening gain for the subducting (denser) plate: the oceanic trench. */
  trenchDeepen: number;
  /** Fraction per Myr a divergent boundary is pulled toward the ridge crest. */
  ridgePull: number;
  /** Isostatic relaxation: fraction per Myr each cell drifts to its baseline. */
  relaxRate: number;
  /** Hard clamp on elevation (km); also bounds the renderer's colour ramp. */
  elevationMin: number;
  elevationMax: number;

  // --- Plate forces / motion (see plateForces.ts) ---
  /** Reference crust density; only denser crust contributes a drive. */
  densityRef: number;
  /** Relative weight of the intrinsic density-distribution drive (bootstrap). */
  intrinsicWeight: number;
  /** Relative weight of slab pull at convergent boundaries. */
  slabWeight: number;
  /** Relative weight of ridge push at divergent boundaries (0 disables). */
  ridgeWeight: number;
  /** Maps net torque magnitude to angular speed (rad/Myr per unit torque). */
  speedGain: number;
  /** Hard cap on a plate's angular speed (rad/Myr). */
  maxSpeed: number;
  /** Per-step relaxation factor: how far omega moves toward its force target. */
  motionRelax: number;

  // --- Crust lifecycle & densities (see PlateData.ts, crustLifecycle.ts) ---
  /** Oceanic crust ages out to here before being treated as fully cooled (Myr). */
  maxOceanicAge: number;
  /** Continental (buoyant) crust density. */
  continentalDensity: number;
  /** Young ridge oceanic crust density. */
  oceanicDensityYoung: number;
  /** Fully cooled oceanic crust density. */
  oceanicDensityOld: number;
  /** Starting oceanic crust thickness (km). */
  oceanicThickness: number;
  /** Thickness (km) above which accreted oceanic crust becomes continental. */
  continentalThicknessThreshold: number;

  // --- Advection (see advect.ts) ---
  /** Fraction of a subducting cell's mass that accretes onto the survivor. */
  accretionFraction: number;
  /** Density stamped on an empty rift placeholder until the lifecycle fills it. */
  riftDensity: number;

  // --- Boundaries / timing ---
  /** Relative speed below which a boundary is treated as inactive (NONE). */
  activeSpeedEps: number;
  /** Myr of plate motion advanced per simulation tick. */
  dtMyr: number;
}

/** The reasoned starting values, used for "reset to defaults". */
export const defaultTuning: Tuning = {
  ridgeElevation: -2.5,
  abyssElevation: -6,
  continentBase: 0.6,
  continentThicknessRef: 7,
  continentThicknessGain: 0.1,
  collisionUplift: 4,
  arcUplift: 3,
  trenchDeepen: 3.5,
  ridgePull: 0.05,
  relaxRate: 0.02,
  elevationMin: -10,
  elevationMax: 10,

  densityRef: 2.7,
  intrinsicWeight: 1.0,
  slabWeight: 0.5,
  ridgeWeight: 0.1,
  speedGain: 0.03,
  maxSpeed: 0.02,
  motionRelax: 0.2,

  maxOceanicAge: 200,
  continentalDensity: 2.7,
  oceanicDensityYoung: 2.9,
  oceanicDensityOld: 3.0,
  oceanicThickness: 7,
  continentalThicknessThreshold: 15,

  accretionFraction: 0.5,
  riftDensity: 2.9,

  activeSpeedEps: 1e-5,
  dtMyr: 2,
};

/** Live, mutable tuning the simulation reads each tick. */
export const tuning: Tuning = { ...defaultTuning };

/** Apply a partial patch to the live tuning (used by the UI sliders). */
export const setTuning = (patch: Partial<Tuning>): void => {
  Object.assign(tuning, patch);
};

/** Restore every tuning value to its default. */
export const resetTuning = (): void => {
  Object.assign(tuning, defaultTuning);
};
