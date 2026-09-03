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
  /**
   * How much of a plate's motion into a locked margin is cancelled each step,
   * from 0 (no reaction at all -- plates plough through each other undiminished)
   * to 1 (the margin holds completely). Only the convergent component is
   * removed, so a blocked plate keeps whatever motion runs along the collision:
   * it slows and it turns, but it is never thrown backwards.
   */
  collisionWeight: number;
  /**
   * How much stiffer a continent-continent collision is than an ocean-ocean
   * subduction zone, where the slab simply sinks out of the way. A margin with
   * continent on one side only sits between the two.
   */
  collisionLockGain: number;
  /**
   * Closing rate, as a fraction of {@link maxSpeed}, above which a
   * continent-continent margin welds into a suture instead of merely resisting.
   * A gentle touch leaves two plates that can drift apart again; crust driven
   * together this hard is joined. Set it to 0 and every continental contact
   * welds; set it above 1 and none ever does.
   */
  sutureClosingFraction: number;
  /**
   * How strongly a suture pulls the two plates onto a common angular velocity,
   * per Myr. Welded crust cannot move two ways at once, so this is what stops
   * the pair pulling apart again and what lets them eventually merge.
   */
  sutureCoupling: number;
  /** Myr a suture must hold before the two plates become a single plate. */
  sutureMergeTime: number;
  /**
   * Largest share of the globe a merged plate may cover. A plate's drive comes
   * from the density asymmetry of the crust it carries, and that asymmetry
   * cancels out as a plate approaches covering everything -- merge without a
   * limit and the world ends up a few vast plates with no net torque, frozen
   * solid. Beyond this size two continents stay welded and travel together, but
   * remain separate plates.
   */
  sutureMaxMergedArea: number;
  /**
   * How close two sutured plates' angular velocities must be, as a fraction of
   * {@link maxSpeed}, before they count as moving as one and are merged.
   */
  sutureSyncTolerance: number;
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
  /**
   * Thickness (km) a crustal root can carry before it starts to founder. This is
   * the headroom a continent-continent collision has to pile into: set it near
   * the continental threshold and colliding continents cannot thicken, so their
   * crust is destroyed and their area shrinks instead of building mountains.
   *
   * It is no longer a hard ceiling. Crust may pile past it; what is above it
   * comes off slowly, at {@link delaminationRate}.
   */
  maxCrustThickness: number;
  /**
   * How fast (km/Myr) crust above {@link maxCrustThickness} founders into the
   * mantle. A deep root turns to eclogite, becomes denser than the mantle it
   * sits in, and peels away -- but over tens of millions of years, not instantly.
   * Deleting the excess the moment it appeared was by far the largest destroyer
   * of continental crust in the model: a collision whose neighbourhood was
   * already at the ceiling simply lost whatever it could not stack. Bleeding it
   * off slowly instead gives lateral spreading time to carry the crust outward,
   * so a collision widens into a plateau rather than evaporating.
   */
  delaminationRate: number;

  // --- Advection (see advect.ts) ---
  /** Fraction of a subducting cell's mass that accretes onto the survivor. */
  accretionFraction: number;
  /**
   * Fastest a cell may thicken by accretion (km/Myr). Several cells can subduct
   * into one destination in a single tick; without this, their mass lands as one
   * lump that vaults the cell past the continental threshold in one step. Crust
   * that cannot be taken up at this rate founders into the mantle instead.
   */
  maxThickeningPerMyr: number;
  /**
   * Thickness (km) above which crust starts to spread sideways under its own
   * weight. A crustal root has no strength to hold an arbitrary pile up: real
   * orogens collapse and flow outwards, which is how a thickened arc turns into
   * a wide continent instead of a one-cell wall that is destroyed at the cap.
   */
  crustSpreadStart: number;
  /** Fraction per Myr of the excess above {@link crustSpreadStart} that flows out. */
  crustSpreadRate: number;
  /**
   * Fraction of a consumed continental cell that is actually lost to the mantle
   * rather than welded onto the plate that took it. Continental crust is too
   * buoyant to subduct, so this stands for the two processes that do remove it
   * at a trench -- subduction erosion of the overriding margin and subducted
   * continental sediment -- both of which are minor next to how much crust
   * survives a collision. Keep it small: it is the main sink on continental
   * area, and at a high value continents stop growing.
   */
  subductionErosion: number;
  /** Density stamped on an empty rift placeholder until the lifecycle fills it. */
  riftDensity: number;
  /**
   * Thickness (km) below which crust stretched across advancing cells counts as
   * rifted apart, and fresh ocean is minted there instead of a paper-thin sliver.
   */
  riftThicknessFloor: number;

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
  collisionWeight: 0.5,
  collisionLockGain: 10,
  sutureClosingFraction: 0.15,
  sutureCoupling: 0.05,
  sutureMergeTime: 80,
  sutureMaxMergedArea: 0.9,
  sutureSyncTolerance: 0.1,
  speedGain: 0.03,
  maxSpeed: 0.02,
  motionRelax: 0.2,

  maxOceanicAge: 200,
  continentalDensity: 2.7,
  oceanicDensityYoung: 2.9,
  oceanicDensityOld: 3.0,
  oceanicThickness: 7,
  continentalThicknessThreshold: 15,
  maxCrustThickness: 60,
  delaminationRate: 0.3,

  accretionFraction: 0.2,
  maxThickeningPerMyr: 0.5,
  subductionErosion: 0.05,
  crustSpreadStart: 16,
  crustSpreadRate: 0.05,
  riftDensity: 2.9,
  riftThicknessFloor: 2,

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
