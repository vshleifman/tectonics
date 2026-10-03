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
  /**
   * Length of slab (radians of arc subducted) a trench needs before it pulls at
   * full strength. A slab only exists where crust has actually gone down, so a
   * new subduction zone has to converge this far before it can drive a plate,
   * and a boundary that merely touches pulls nothing.
   */
  slabFullLength: number;
  /**
   * Myr over which a slab sinks away once its trench stops converging. Until
   * then it keeps pulling, and it keeps its polarity: the side that was
   * overriding stays overriding rather than the two plates trading places.
   */
  slabDecayTime: number;
  /**
   * Myr for old, fully cooled ocean at a margin that is not spreading to
   * founder into a full slab of its own, with no convergence to push it down.
   * This is how new subduction zones start; without it the trenches a world
   * begins with are the only ones it ever gets.
   */
  slabInitiationTime: number;
  /**
   * Youngest ocean (Myr) that can founder into a new trench unforced. Real
   * subduction rarely starts on its own: the Atlantic's 180 Myr-old margins
   * have yet to. Raising it makes new trenches rarer and plate motions steadier,
   * but the sea floor then ages far beyond Earth's (~250 Myr mean at 100) and
   * the continents shrink, short of active margins to recycle at. 0 lets any
   * ocean old enough to pull founder.
   */
  slabInitiationAge: number;
  /**
   * Myr a plate takes to settle onto the motion its forces call for. Plates
   * have no inertia, but the slabs that drive them cannot swing round: a
   * trench and the mantle flow around it take tens of Myr to reorganise, which
   * is why plates hold their heading for tens of Myr. At 40 a large plate's
   * heading turns ~19° over 50 Myr; at 15 it wanders ~28°.
   */
  motionResponseTime: number;
  /**
   * Pull on the overriding plate toward a trench, as a share of the slab pull
   * on the plate going down: a rolling-back slab draws the plate above after
   * it. Real, but here it makes plates track their slabs less, so off by
   * default.
   */
  trenchSuction: number;
  /**
   * Fastest a plate's Euler pole may swing (degrees per Myr); 0 for no limit.
   * Not physics, a knob: low values (<1) make headings very steady, but plates
   * then cannot follow their slabs, trenches stall and continents dwindle.
   */
  maxTurnRate: number;
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
  /**
   * How strongly the mantle flowing in toward downwellings carries continents
   * along, as a multiple of the basal drag on their keels (see
   * sim-rs/src/mantle.rs). Slabs sinking at trenches drag the mantle down and
   * it flows in toward them from all around; deep continental keels are
   * carried with it, while ocean floor rides on a soft asthenosphere and goes
   * where its slabs pull it. So continents drift toward where the mantle goes
   * down and gather there, which is what assembles supercontinents. Higher
   * gathers them into fewer, larger landmasses more of the time, but plate
   * speeds track their trenches less (the Forsyth & Uyeda signature), and
   * above ~50 faster continents feed the downwellings that pull them and the
   * motion runs away. 0 turns it off.
   */
  mantleFlowCoupling: number;
  /**
   * Myr a downwelling outlives the subduction that feeds it: slab piles keep
   * sinking long after the trench above them has gone quiet, and continents
   * keep converging on where an ocean closed. Longer makes steadier targets
   * for continents to gather over.
   */
  downwellingMemory: number;

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
   * Thickness (km) below which continental crust reverts to oceanic. Kept below
   * {@link continentalThicknessThreshold} so a cell on the cusp cannot flicker
   * between types. Stretched or eroded continent thinned this far is no longer
   * counted as continental area.
   */
  continentalRevertThickness: number;
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
  /**
   * Fraction of a subducting cell's mass that accretes onto the survivor. This
   * sets how fast arcs build continent, and so how fast continents turn over
   * once {@link continentalAreaTarget} has levelled them off. Earth's arcs keep
   * roughly a tenth of what they consume; at 0.2 continents reach their target
   * in ~3 Gyr and their crust lasts ~1.5 Gyr before it is recycled (Earth's
   * averages ~2 Gyr). Higher grows them faster and turns them over faster.
   */
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
   * orogens collapse and flow outwards, which is how a collision widens into a
   * plateau rather than a one-cell wall that is destroyed at the cap. Spreading
   * only fills neighbouring continental cells, so it cannot flood the ocean and
   * mint new continent.
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
   * survives a collision. Keep it small: the steady recycling at active
   * margins is {@link continentalAreaTarget}'s job, and at a high value
   * continents stop growing.
   */
  subductionErosion: number;
  /**
   * Fraction of a subducting oceanic cell's mass stripped from the continental
   * margin it dives under. Intra-oceanic arcs are untouched. Combined with
   * {@link accretionFraction}, the net on an Andean margin is
   * accretion minus this; when this is larger the margin shrinks.
   */
  marginErosion: number;
  /** Density stamped on an empty rift placeholder until the lifecycle fills it. */
  riftDensity: number;
  /**
   * Thickness (km) below which crust stretched across advancing cells counts as
   * rifted apart, and fresh ocean is minted there instead of a paper-thin sliver.
   */
  riftThicknessFloor: number;
  /**
   * Rate (km/Myr) at which a small body of ocean ringed by continent fills with
   * crust shed from the land around it, until it is thick enough to count as
   * continent. Arcs coalescing around a patch of sea floor trap it inside the
   * continent they build, where no trench can ever reach it; on Earth such
   * basins are buried under sediment eroded off their rims (the South Caspian
   * holds ~20 km of it) and end up as continental basement. Without this they
   * ride inside the continents for ever and pock-mark them.
   */
  basinInfillRate: number;
  /**
   * Largest enclosed body of ocean, as a share of the globe, that fills in.
   * Larger trapped seas, and any touching a plate boundary, are left alone.
   */
  basinInfillMaxArea: number;
  /**
   * Share of the globe at which continental growth levels off, in [0, 1];
   * Earth's is ~0.4. Continental crust goes back into the mantle at active
   * margins, as sediment carried down trenches and as forearc scraped off the
   * overriding plate, at a rate proportional to how much continent there is.
   * That rate is matched to the growth everything else produces (averaged over
   * 200 Myr), so the two balance at this area: growth is fast while continents
   * are small and slows as they near it, as Earth's did. 0 uses the fixed
   * {@link continentalRecyclingRate} instead. Arcs cannot build past a natural
   * ceiling of roughly half the globe at the default accretion, as the ocean
   * that feeds them runs out; a target above it leaves growth uncapped.
   */
  continentalAreaTarget: number;
  /**
   * Share of the continental area recycled per Myr when
   * {@link continentalAreaTarget} is 0. Earth returns ~3 km³/yr of its ~7×10⁹
   * km³ of continental crust, ~0.0004 per Myr; where growth then levels off is
   * up to the rest of the model.
   */
  continentalRecyclingRate: number;

  // --- Cratons (see sim-rs/src/craton.rs) ---
  /**
   * Age (Myr) continental crust must reach before it can grow a cratonic keel:
   * the cold, buoyant, stiff mantle root of an ancient continental core.
   */
  cratonMinAge: number;
  /**
   * How far inland (radians; 0.02 ≈ 130 km) crust must lie from the ocean to
   * grow a keel: continental margins are too busy to cratonise. A plate
   * boundary through a continent does not count; a craton may lie against a
   * suture.
   */
  cratonInland: number;
  /** Myr for a qualifying interior to grow a full keel. */
  cratonGrowthTime: number;
  /**
   * Myr for a keel to be lost where its crust is rifted, weakened or thinned,
   * as North China's was.
   */
  cratonDecayTime: number;
  /**
   * Extra strength a full keel gives continental lithosphere against rifting,
   * on top of its own 1: rifts go around cratons and break continents along
   * the younger belts between them, so the pieces keep their cores.
   */
  cratonStrength: number;
  /**
   * Extra coupling a full keel gives its crust to the mantle beneath, on top
   * of {@link continentalDrag}: craton-bearing plates resist changes of motion
   * and are carried harder by {@link mantleFlowCoupling}.
   */
  cratonKeelGain: number;

  // --- Rifting (see sim-rs/src/rifting.rs) ---
  /**
   * Tension intact continental lithosphere must be under before it tears;
   * oceanic lithosphere is scaled from it by age (see
   * {@link oceanicStrengthYoung}, {@link oceanicStrengthOld}). A plate with
   * slabs pulling it in opposing directions is in extension: the pull that
   * cancels, spread over the plate's mean width across it, so a large plate is
   * stressed as much as a small one pulled as hard per unit of margin. When that
   * exceeds the strength of the line it would tear along, it rifts and becomes
   * two plates. This is the counterweight to merging: without it a world only
   * ever loses plates, and ends up as two hemispheres with nowhere left to make
   * new crust.
   */
  riftStrength: number;
  /**
   * Strength of oceanic lithosphere fresh from a ridge, as a multiple of
   * continental. Young, hot, thin lithosphere is the weakest there is, which is
   * why oceanic plates break up near ridges if they break at all.
   */
  oceanicStrengthYoung: number;
  /**
   * Strength of fully cooled oceanic lithosphere, as a multiple of continental.
   * Lithosphere thickens as the square root of its age, so old ocean is the
   * strongest there is and almost never tears.
   */
  oceanicStrengthOld: number;
  /**
   * Myr a continental rift stretches before the plate breaks in two. Over that
   * time the crust along it thins into a basin between raised shoulders, and if
   * the tension behind it dies away first it stalls as a failed rift: thinned,
   * left weak, but whole. A rift through ocean takes this times the share of
   * continent along it, so a purely oceanic one opens at once.
   */
  riftDuration: number;
  /**
   * How many times over the crust along a rift is thinned by the time it breaks
   * (the stretching factor). The crust it loses is piled onto the shoulders.
   */
  riftStretch: number;
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
  /**
   * Fraction of the globe each piece must cover when a rift starts, so a plate
   * smaller than twice this cannot rift at all. Once it has stretched, a rift
   * breaks as long as both pieces are still big enough to be plates
   * ({@link minPlateArea}), even if one has since lost crust down a trench.
   */
  minRiftArea: number;
  /**
   * Longest a piece left by a rift may be, as a multiple of its width. A rift
   * that would leave a longer sliver is not taken (unless the plate was already
   * more elongated than that), so a plate breaks across its length instead of
   * being sliced into ever narrower strips.
   */
  maxRiftAspect: number;

  // --- Supercontinent insulation (optional; see sim-rs/src/rifting.rs) ---
  /** Rate per Myr at which mantle heat builds deep under continental crust. */
  insulationRate: number;
  /**
   * Distance inland (radians) at which a continent insulates the mantle at the
   * full {@link insulationRate}; nearer the coast heat escapes sideways and it
   * builds proportionally slower. So it is large continents, not every
   * continent, that trap heat and tear themselves apart.
   */
  insulationReach: number;
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
  /**
   * Push per unit length of plate margin, per unit of mantle heat beneath it
   * (see sim-rs/src/plate_forces.rs). Lithosphere raised over a hot dome slides
   * off it, so the two halves of a plate that rifted over heat move apart even
   * with no slab to pull them. Welded margins are exempt.
   */
  domePush: number;
  /**
   * Myr after a breakup that {@link domePush} goes on separating the pieces,
   * until breakup melting has vented the dome. A push lasting for good between
   * every pair of hot neighbours would hold all continents apart.
   */
  domePushDuration: number;
  /**
   * Share of the mantle heat under a plate that is vented when it rifts, as
   * the melting of breakup does. Too little and the pieces, still sitting on
   * hot mantle, break again within a few ticks, so a supercontinent shatters
   * at once instead of coming apart in stages.
   */
  riftHeatRelease: number;

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
  slabFullLength: 0.1,
  slabDecayTime: 30,
  slabInitiationTime: 20,
  slabInitiationAge: 0,
  motionResponseTime: 40,
  trenchSuction: 0,
  maxTurnRate: 0,
  ridgeWeight: 0.05,
  mantleDrag: 30,
  continentalDrag: 3,
  collisionDrag: 400,
  collisionLockGain: 10,
  sutureCoupling: 0.05,
  sutureMergeTime: 80,
  boundaryDeathTime: 120,
  mergeQuietSpeed: 0.002,
  mantleFlowCoupling: 30,
  downwellingMemory: 800,

  maxOceanicAge: 200,
  continentalDensity: 2.7,
  oceanicDensityYoung: 2.9,
  oceanicDensityOld: 3.0,
  oceanicThickness: 7,
  continentalThicknessThreshold: 15,
  continentalRevertThickness: 10,
  maxCrustThickness: 60,
  delaminationRate: 0.3,

  accretionFraction: 0.2,
  maxThickeningPerMyr: 0.5,
  subductionErosion: 0.05,
  marginErosion: 0.12,
  crustSpreadStart: 35,
  crustSpreadRate: 0.05,
  riftDensity: 2.9,
  riftThicknessFloor: 2,
  basinInfillRate: 1,
  basinInfillMaxArea: 0.0005,
  continentalAreaTarget: 0.4,
  continentalRecyclingRate: 0.0004,
  cratonMinAge: 500,
  cratonInland: 0.02,
  cratonGrowthTime: 400,
  cratonDecayTime: 100,
  cratonStrength: 2,
  cratonKeelGain: 1,

  riftStrength: 0.65,
  oceanicStrengthYoung: 0.6,
  oceanicStrengthOld: 1.5,
  riftDuration: 20,
  riftStretch: 3,
  sutureWeakness: 1,
  riftMarginWeakness: 0.6,
  weaknessDecayTime: 2000,
  weaknessRelief: 0.7,
  minRiftArea: 0.015,
  maxRiftAspect: 3,

  insulationRate: 0.004,
  insulationReach: 0.1,
  insulationCooling: 0.01,
  insulationWeakening: 0.6,
  insulationDrive: 1.2,
  domePush: 0.5,
  domePushDuration: 60,
  riftHeatRelease: 0.5,

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
