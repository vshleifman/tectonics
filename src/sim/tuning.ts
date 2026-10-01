/**
 * Central, live-editable home for every tunable simulation constant.
 *
 * The UI edits this singleton and the main thread forwards every change to the
 * simulation worker, whose Rust world (`sim-rs/src/tuning.rs`, same field
 * names) reads it on its next tick, so tweaks take effect without a world
 * rebuild. It stays mirrored on the main thread because the colour ramps read
 * it too. Pure numerical-stability epsilons are intentionally left in the
 * simulation — they are not creative knobs.
 *
 * All elevations are in km, rates per Myr, densities in g/cm3.
 */
export interface Tuning {
  // --- Elevation / relief (see sim-rs/src/elevation.rs) ---
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
  /** Hard clamp on elevation (km). */
  elevationMin: number;
  elevationMax: number;

  // --- Plate forces / motion (see sim-rs/src/plate_forces.rs) ---
  /**
   * Pull per unit length of trench, per unit of slab negative buoyancy. The
   * dominant driving force, so this is effectively the world's overall pace.
   */
  slabWeight: number;
  /**
   * Age (Myr) at which cooling oceanic lithosphere becomes denser than the
   * mantle and starts to pull. Below it a slab has no weight to give.
   */
  slabPullOnsetAge: number;
  /**
   * Age (Myr) by which the plate has cooled through and its slab stops getting
   * heavier. Beyond this, older crust pulls no harder.
   */
  slabPullSaturationAge: number;
  /** Push per unit length of ridge, per km of flank relief (0 disables). */
  ridgeWeight: number;
  /**
   * Resistance of the mantle under a plate, per unit area. With slab pull this
   * sets the whole scale of plate speeds: a plate moves at the rate that makes
   * its drag match its pull. There is no speed cap -- this is what replaces one.
   */
  mantleDrag: number;
  /**
   * How much harder continental crust drags than oceanic, standing for the deep
   * lithospheric keel under a craton. This is why the continent-heavy plates are
   * the slow ones.
   */
  continentalDrag: number;
  /**
   * Resistance per unit length of convergent margin, acting only across the
   * boundary. A trench gets a fraction of this ({@link collisionLockGain}); a
   * continent-continent margin gets all of it.
   */
  collisionDrag: number;
  /**
   * How much stiffer a continent-continent collision is than an ocean-ocean
   * subduction zone, where the slab simply sinks out of the way. A margin with
   * continent on one side only sits between the two.
   */
  collisionLockGain: number;
  /**
   * How strongly a suture pulls the two plates onto a common angular velocity,
   * per Myr. Welded crust cannot move two ways at once, so this is what stops
   * the pair pulling apart again and what lets them eventually merge.
   */
  sutureCoupling: number;
  /** Myr a suture must hold before the two plates become a single plate. */
  sutureMergeTime: number;
  /**
   * Myr a boundary must stay quiet -- no appreciable relative motion across it,
   * welded or not -- before the two plates are recognised as one. The other way
   * a plate stops existing, and the only one that needs no collision at all.
   */
  boundaryDeathTime: number;
  /**
   * Angular speed (rad/Myr) below which two plates' motions count as the same
   * motion, so that a pair welded for long enough can be merged into one plate.
   */
  mergeQuietSpeed: number;

  // --- Crust lifecycle & densities (see sim-rs/src/plates.rs, crust_lifecycle.rs) ---
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

  // --- Advection (see sim-rs/src/advect.rs) ---
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

  // --- Rifting (see sim-rs/src/rifting.rs) ---
  /**
   * Tension a plate must be under before it tears, per unit area. A plate with
   * slabs pulling it in opposing directions is in extension; when that exceeds
   * the strength of the weakest line across it, it rifts and becomes two plates.
   * This is the counterweight to merging: without it a world only ever loses
   * plates, and ends up as two hemispheres with nowhere left to make new crust.
   */
  riftStrength: number;
  /** Weakness stamped on the seam where two continents weld into a suture. */
  sutureWeakness: number;
  /** Weakness stamped on a margin left behind where a plate rifted apart. */
  riftMarginWeakness: number;
  /**
   * Myr over which lithospheric weakness decays back toward intact. Long: old
   * sutures stay the preferred place to break for billions of years, which is
   * what makes an ocean reopen roughly where the last one closed.
   */
  weaknessDecayTime: number;
  /** How much of a plate's strength the weakest line across it can remove. */
  weaknessRelief: number;
  /** Fraction of the globe below which a plate is too small to rift at all. */
  minRiftArea: number;

  // --- Supercontinent insulation (optional; see sim-rs/src/rifting.rs) ---
  /** Rate per Myr at which mantle heat builds under continental crust. */
  insulationRate: number;
  /** Rate per Myr at which mantle heat escapes under oceanic crust. */
  insulationCooling: number;
  /** How much of a plate's strength fully-built-up mantle heat removes. */
  insulationWeakening: number;
  /**
   * Extension that trapped mantle heat drives on its own, with no slab pulling.
   * A dome over an upwelling stretches the lithosphere above it, which is how a
   * supercontinent with no opposed subduction margins still tears itself apart --
   * the case Pangaea was, and the reason this term exists at all.
   */
  insulationDrive: number;

  // --- Plate identity (see sim-rs/src/plate_lifecycle.rs) ---
  /**
   * Smallest share of the globe a fragment must cover to become a plate of its
   * own when a plate is pinched in two. Below it the fragment is absorbed into a
   * neighbour: a handful of cells is an artefact of the grid, not a microplate,
   * and handing every one an Euler pole fills the world with noise. Earth's
   * smallest recognised plates are a few hundredths of a percent of the surface.
   */
  minPlateArea: number;

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

  slabWeight: 1,
  slabPullOnsetAge: 25,
  slabPullSaturationAge: 80,
  ridgeWeight: 0.05,
  mantleDrag: 30,
  continentalDrag: 3,
  collisionDrag: 400,
  collisionLockGain: 10,
  sutureCoupling: 0.05,
  sutureMergeTime: 80,
  boundaryDeathTime: 120,
  mergeQuietSpeed: 0.002,

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

  riftStrength: 1.1,
  sutureWeakness: 1,
  riftMarginWeakness: 0.6,
  weaknessDecayTime: 2000,
  weaknessRelief: 0.7,
  minRiftArea: 0.03,

  insulationRate: 0.004,
  insulationCooling: 0.01,
  insulationWeakening: 0.6,
  insulationDrive: 1.2,

  minPlateArea: 0.0005,

  activeSpeedEps: 1e-5,
  dtMyr: 2,
};

/**
 * Modelling choices that are on/off rather than a number to dial, kept beside
 * {@link tuning} and read the same way.
 */
export interface SimOptions {
  /**
   * Whether mantle heat builds under continental crust and weakens the plate
   * above it. On, supercontinents assemble and then tear themselves apart, which
   * is what Earth does; off, rifting is driven by inherited weakness and slab
   * tension alone and the per-cell heat field is never touched.
   */
  supercontinentInsulation: boolean;
}

export const defaultSimOptions: SimOptions = {
  supercontinentInsulation: true,
};

/** Live, mutable modelling options the simulation reads each tick. */
export const simOptions: SimOptions = { ...defaultSimOptions };

/** Apply a partial patch to the live modelling options (used by the UI). */
export const setSimOptions = (patch: Partial<SimOptions>): void => {
  Object.assign(simOptions, patch);
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
