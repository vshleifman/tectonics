import { clamp } from "../math/scalar";
import {
  CM_PER_YR_PER_RAD_MYR,
  densityMax,
  densityMin,
  type PlateSummary,
} from "../plates/PlateData";
import {
  colourForPlate,
  type ElevationStyle,
  legendForView,
  toCssRgb,
} from "../render/colour";
import type { Projection } from "../render/projection";
import type { WorldMetrics } from "../sim/metrics";
import {
  resetTuning,
  setSimOptions,
  setTuning,
  simOptions,
  type Tuning,
  tuning,
} from "../sim/tuning";
import { createDiagnostics } from "./diagnostics";
import type { PlateOverlay, SettingsStore, ViewMode } from "./settingsStore";

const ACCENT = "#4f8ff0";

/** Imperative handle for pushing live sim readouts back into the panels. */
export interface Controls {
  setCellCount: (count: number) => void;
  setPlateCount: (count: number) => void;
  setFps: (fps: number) => void;
  /** Worker ms per tick, and main-thread ms to apply each snapshot. */
  setTimings: (simMs: number, uiMs: number) => void;
  setMassReadout: (liveMass: number, drift: number) => void;
  setNote: (text: string) => void;
  /** Replace the plate list with the live set of plates. */
  setPlates: (plates: readonly PlateSummary[]) => void;
  /** Update the diagnostics panel with a fresh reading of the world. */
  setMetrics: (metrics: WorldMetrics) => void;
}

/** Legend for the boundary velocity arrows, matching the renderer's tints. */
const ARROW_LEGEND: ReadonlyArray<{ colour: string; label: string }> = [
  { colour: "rgb(255, 107, 87)", label: "Convergent" },
  { colour: "rgb(107, 184, 255)", label: "Divergent" },
  { colour: "rgb(255, 219, 92)", label: "Transform" },
];

const MIN_LEVEL = 2;
const MAX_LEVEL = 8;
const MIN_SEA_LEVEL = -8;
const MAX_SEA_LEVEL = 8;
const MIN_BRANCHES = 1;
const MAX_BRANCHES = 5;
const MIN_TICK_RATE = 1;
const MAX_TICK_RATE = 30;

/** One tunable simulation constant exposed as a slider. */
interface TuningSliderSpec {
  key: keyof Tuning;
  label: string;
  /** Short explanation shown beside the label and slider on hover. */
  tooltip: string;
  min: number;
  max: number;
  step: number;
}

/** A collapsible group of related tuning sliders. */
interface TuningGroupSpec {
  title: string;
  sliders: TuningSliderSpec[];
}

/**
 * One line per tuning constant, paraphrased from the notes in `tuning.ts`.
 * Exhaustive over {@link Tuning}, so a new constant cannot ship without a
 * tooltip even if it is not yet on a slider.
 */
const TUNING_TOOLTIPS: Record<keyof Tuning, string> = {
  ridgeElevation: "Elevation of young oceanic crust at a mid-ocean ridge.",
  abyssElevation:
    "Elevation of fully cooled oceanic crust on the abyssal plain.",
  seafloorFlatteningAge:
    "Age by which the sea floor has sunk to the abyssal plain, sinking as the square root of age until then.",
  mantleDensity:
    "Density of the mantle crust floats on; sets how high a given thickness of crust stands.",
  continentThermalShare:
    "How much higher young continent stands than old, as a share of the ocean floor's cooling relief.",
  collisionUplift:
    "Uplift where two continents collide and both sides pile up.",
  arcUplift: "Uplift of the buoyant plate overriding a subduction zone.",
  trenchDeepen: "Deepening of the denser plate as it bends into the trench.",
  ridgePull: "How fast a divergent boundary is pulled toward the ridge crest.",
  relaxRate: "How fast each cell drifts back toward its isostatic baseline.",
  elevationMin: "Lowest elevation the model will allow.",
  elevationMax: "Highest elevation the model will allow.",
  slabWeight: "Pull per unit of trench, and so the world's overall pace.",
  slabPullOnsetAge: "Age at which cooling ocean becomes dense enough to pull.",
  slabPullSaturationAge:
    "Age by which a slab has cooled through and pulls no harder.",
  slabFullLength:
    "How much slab a trench needs before it pulls at full strength.",
  slabDecayTime:
    "How long a slab keeps pulling after its trench stops converging.",
  slabInitiationTime:
    "How long old ocean at a quiet margin takes to founder into a new trench.",
  slabInitiationAge:
    "Youngest ocean that can founder into a trench with nothing pushing it.",
  motionResponseTime:
    "How long a plate takes to settle onto the motion its forces call for.",
  trenchSuction: "Pull on the overriding plate toward a rolling-back slab.",
  maxTurnRate: "Fastest a plate's heading may swing; zero means no limit.",
  ridgeWeight:
    "Push on ocean floor sliding off the ridge as it cools and sinks; zero turns ridge push off.",
  mantleDrag:
    "Drag of the mantle under a plate; with slab pull this sets plate speeds.",
  continentalDrag: "How much harder continental crust drags than oceanic.",
  collisionDrag:
    "Resistance across a convergent margin, strongest for continent on continent.",
  collisionLockGain:
    "How much stiffer a continent-continent collision is than ocean subduction.",
  sutureCoupling: "How strongly a suture pulls two plates onto one motion.",
  sutureMergeTime:
    "How long a suture must hold before the two plates become one.",
  boundaryDeathTime:
    "How long a quiet boundary must last before the two plates are one.",
  mergeQuietSpeed:
    "Relative speed below which two plates count as moving together.",
  mantleFlowCoupling:
    "How strongly mantle flow toward trenches carries continents along.",
  downwellingMemory:
    "How long a mantle downwelling outlives the trench that fed it.",
  maxOceanicAge: "Age at which oceanic crust is treated as fully cooled.",
  continentalDensity: "Density of buoyant continental crust.",
  oceanicDensityYoung: "Density of young crust at a ridge.",
  oceanicDensityOld: "Density of fully cooled oceanic crust.",
  oceanicThickness: "Starting thickness of oceanic crust.",
  continentalThicknessThreshold:
    "Thickness at which accreted ocean counts as continent.",
  continentalRevertThickness:
    "Thickness below which continent reverts to ocean.",
  continentalNucleationThickness:
    "Thickness at which ocean floor away from any continent turns continental.",
  maxCrustThickness:
    "Thickness a crustal root can carry before it starts to founder.",
  delaminationRate: "How fast crust above that ceiling peels into the mantle.",
  accretionFraction:
    "Share of a subducting cell that sticks onto the plate above.",
  maxThickeningPerMyr: "Fastest a cell may thicken by accretion.",
  crustSpreadStart:
    "Thickness above which crust spreads sideways under its own weight.",
  crustSpreadRate: "How fast the excess above that thickness flows outward.",
  subductionErosion: "Share of consumed continent actually lost to the mantle.",
  marginErosion:
    "Share of a subducting ocean cell stripped from a continental margin.",
  riftDensity: "Density of an empty rift until the lifecycle fills it.",
  riftThicknessFloor:
    "Thickness below which stretched crust counts as rifted ocean.",
  basinInfillRate:
    "How fast a small ocean ringed by continent fills with crust.",
  basinInfillMaxArea:
    "Largest enclosed sea, as a share of the globe, that fills in.",
  minContinentArea:
    "Landmasses smaller than this share of the globe rejoin a nearby continent.",
  cohesionReach:
    "Rings of cells a stray crumb of continent searches for a landmass to rejoin.",
  continentalAreaTarget:
    "Share of the globe at which continental growth levels off.",
  continentalRecyclingRate:
    "Share of continental area recycled per Myr when no target is set.",
  cratonMinAge: "Age continental crust must reach before it can grow a keel.",
  cratonInland: "How far inland crust must lie from the ocean to grow a keel.",
  cratonGrowthTime: "How long a qualifying interior takes to grow a full keel.",
  cratonDecayTime:
    "How long a keel takes to be lost where its crust is rifted, thinned or crushed.",
  cratonStrength: "Extra strength a full keel gives against rifting.",
  cratonKeelGain: "Extra coupling a full keel gives its crust to the mantle.",
  cratonDurability:
    "How hard cratons are to erode or rift; above ~0.85 they can lock the plates up.",
  cratonMergeTime:
    "How long two plates' cratons must stay welded before the plates merge; 0 to never force it.",
  riftStrength:
    "Tension continental lithosphere must be under before it tears.",
  oceanicStrengthYoung:
    "Strength of young oceanic lithosphere, relative to continental.",
  oceanicStrengthOld:
    "Strength of fully cooled ocean; old ocean almost never tears.",
  riftDuration:
    "How long a continental rift stretches before the plate breaks.",
  riftStretch:
    "How many times over rift crust is thinned by the time it breaks.",
  sutureWeakness: "Weakness left on the seam where two continents weld.",
  riftMarginWeakness: "Weakness left where a plate has rifted apart.",
  weaknessDecayTime: "How long lithospheric weakness takes to heal.",
  weaknessRelief: "How much of a plate's strength its weakest line can remove.",
  minRiftArea: "Smallest share of the globe each piece of a rift must cover.",
  maxRiftAspect: "Longest a rift piece may be, as a multiple of its width.",
  insulationRate: "How fast mantle heat builds under continental crust.",
  insulationReach:
    "How far inland a continent must reach to trap heat at full strength.",
  insulationCooling: "How fast mantle heat escapes under oceanic crust.",
  insulationWeakening:
    "How much fully built-up mantle heat removes from a plate's strength.",
  insulationDrive: "Extension that trapped heat drives with no slab pulling.",
  domePush: "Push that slides lithosphere off a hot dome after a rift.",
  domePushDuration: "How long that push keeps the rifted pieces apart.",
  riftHeatRelease: "Share of mantle heat vented when a plate rifts.",
  minPlateArea: "Smallest fragment that becomes a plate of its own.",
  primordialHeat:
    "Excess heat of the young mantle; above 0, early continent forms as plume cratons.",
  primordialCoolingTime: "How long the young mantle takes to cool off.",
  plumeRate: "Plumes rising per Myr while the mantle is at full heat.",
  plumeRadius: "Radius of the plateau each plume builds.",
  plumeDuration: "How long a plume takes to build its plateau.",
  plumeThickness: "Crust thickness a plume builds at the plateau's centre.",
  plumeKeel: "Cratonic keel plume crust is born with.",
  hotArcSuppression: "How much a hot mantle holds back arc accretion.",
  hotDrip:
    "How much thickened ocean floor drips into a hot mantle instead of becoming continent.",
  activeSpeedEps: "Relative speed below which a boundary counts as inactive.",
  dtMyr: "Million years of plate motion advanced per simulation tick.",
};

/**
 * Every tuning constant, grouped by the part of the simulation it governs. The
 * ranges are generous bands around the defaults so the whole feel can be dialled
 * without breaking the maths; values clamp to these bounds.
 */
const TUNING_GROUP_FIELDS: {
  title: string;
  sliders: Omit<TuningSliderSpec, "tooltip">[];
}[] = [
  {
    title: "Relief / elevation",
    sliders: [
      {
        key: "ridgeElevation",
        label: "Ridge crest",
        min: -8,
        max: 0,
        step: 0.1,
      },
      {
        key: "abyssElevation",
        label: "Abyssal floor",
        min: -10,
        max: 0,
        step: 0.1,
      },
      {
        key: "seafloorFlatteningAge",
        label: "Sea floor flattening age (Myr)",
        min: 10,
        max: 300,
        step: 5,
      },
      {
        key: "mantleDensity",
        label: "Mantle density",
        min: 3.0,
        max: 3.6,
        step: 0.01,
      },
      {
        key: "continentThermalShare",
        label: "Young continent uplift",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "collisionUplift",
        label: "Collision uplift",
        min: 0,
        max: 10,
        step: 0.1,
      },
      { key: "arcUplift", label: "Arc uplift", min: 0, max: 10, step: 0.1 },
      {
        key: "trenchDeepen",
        label: "Trench deepen",
        min: 0,
        max: 10,
        step: 0.1,
      },
      { key: "ridgePull", label: "Ridge pull", min: 0, max: 1, step: 0.01 },
      {
        key: "relaxRate",
        label: "Isostatic relax rate",
        min: 0,
        max: 0.5,
        step: 0.005,
      },
      {
        key: "elevationMin",
        label: "Elevation clamp min",
        min: -20,
        max: 0,
        step: 0.5,
      },
      {
        key: "elevationMax",
        label: "Elevation clamp max",
        min: 0,
        max: 20,
        step: 0.5,
      },
    ],
  },
  {
    title: "Plate forces / motion",
    sliders: [
      { key: "slabWeight", label: "Slab pull", min: 0, max: 5, step: 0.1 },
      {
        key: "slabPullOnsetAge",
        label: "Slab pull onset age",
        min: 0,
        max: 100,
        step: 5,
      },
      {
        key: "slabPullSaturationAge",
        label: "Slab pull saturation age",
        min: 20,
        max: 300,
        step: 5,
      },
      {
        key: "slabFullLength",
        label: "Slab length for full pull (rad)",
        min: 0.01,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "slabDecayTime",
        label: "Slab sinking time (Myr)",
        min: 2,
        max: 200,
        step: 2,
      },
      {
        key: "slabInitiationTime",
        label: "Subduction initiation (Myr)",
        min: 2,
        max: 200,
        step: 2,
      },
      {
        key: "slabInitiationAge",
        label: "Subduction initiation age (Myr)",
        min: 0,
        max: 250,
        step: 5,
      },
      {
        key: "motionResponseTime",
        label: "Plate motion response (Myr)",
        min: 0,
        max: 100,
        step: 1,
      },
      {
        key: "trenchSuction",
        label: "Trench suction",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "maxTurnRate",
        label: "Max turn rate (deg/Myr, 0 = off)",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "ridgeWeight",
        label: "Ridge push",
        min: 0,
        max: 0.5,
        step: 0.005,
      },
      { key: "mantleDrag", label: "Mantle drag", min: 20, max: 2000, step: 20 },
      {
        key: "continentalDrag",
        label: "Continental keel drag",
        min: 1,
        max: 12,
        step: 0.5,
      },
      {
        key: "collisionDrag",
        label: "Collision resistance",
        min: 0,
        max: 2000,
        step: 20,
      },
      {
        key: "collisionLockGain",
        label: "Collision lock gain",
        min: 1,
        max: 40,
        step: 1,
      },
      {
        key: "sutureCoupling",
        label: "Suture coupling /Myr",
        min: 0,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "sutureMergeTime",
        label: "Suture merge time (Myr)",
        min: 0,
        max: 400,
        step: 10,
      },
      {
        key: "mergeQuietSpeed",
        label: "Merge quiet speed",
        min: 0,
        max: 0.02,
        step: 0.0005,
      },
      {
        key: "mantleFlowCoupling",
        label: "Mantle flow coupling",
        min: 0,
        max: 60,
        step: 1,
      },
      {
        key: "downwellingMemory",
        label: "Downwelling memory (Myr)",
        min: 10,
        max: 2000,
        step: 10,
      },
    ],
  },
  {
    title: "Rifting",
    sliders: [
      {
        key: "riftStrength",
        label: "Plate strength",
        min: 0,
        max: 3,
        step: 0.05,
      },
      {
        key: "oceanicStrengthYoung",
        label: "Young ocean strength",
        min: 0.1,
        max: 3,
        step: 0.05,
      },
      {
        key: "oceanicStrengthOld",
        label: "Old ocean strength",
        min: 0.1,
        max: 5,
        step: 0.05,
      },
      {
        key: "riftDuration",
        label: "Rift stretching (Myr)",
        min: 0,
        max: 60,
        step: 2,
      },
      {
        key: "riftStretch",
        label: "Rift thinning factor",
        min: 1,
        max: 6,
        step: 0.25,
      },
      {
        key: "minRiftArea",
        label: "Min riftable area",
        min: 0.005,
        max: 0.2,
        step: 0.005,
      },
      {
        key: "maxRiftAspect",
        label: "Max rift piece elongation",
        min: 1.5,
        max: 10,
        step: 0.5,
      },
      {
        key: "weaknessRelief",
        label: "Weakness relief",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "sutureWeakness",
        label: "Suture weakness",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "riftMarginWeakness",
        label: "Rift margin weakness",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "weaknessDecayTime",
        label: "Weakness memory (Myr)",
        min: 100,
        max: 5000,
        step: 100,
      },
      {
        key: "insulationRate",
        label: "Insulation heating /Myr",
        min: 0,
        max: 0.05,
        step: 0.001,
      },
      {
        key: "insulationReach",
        label: "Insulation reach inland (rad)",
        min: 0,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "insulationCooling",
        label: "Insulation cooling /Myr",
        min: 0,
        max: 0.05,
        step: 0.001,
      },
      {
        key: "insulationWeakening",
        label: "Heat weakening",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "insulationDrive",
        label: "Heat-driven extension",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "domePush",
        label: "Dome push",
        min: 0,
        max: 2,
        step: 0.05,
      },
      {
        key: "domePushDuration",
        label: "Dome push lasts (Myr)",
        min: 0,
        max: 300,
        step: 10,
      },
      {
        key: "riftHeatRelease",
        label: "Heat vented on breakup",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "minPlateArea",
        label: "Min fragment area",
        min: 0,
        max: 0.01,
        step: 0.0005,
      },
    ],
  },
  {
    title: "Crust & density",
    sliders: [
      {
        key: "maxOceanicAge",
        label: "Max oceanic age",
        min: 10,
        max: 500,
        step: 10,
      },
      {
        key: "continentalDensity",
        label: "Continental density",
        min: 2.0,
        max: 3.2,
        step: 0.01,
      },
      {
        key: "oceanicDensityYoung",
        label: "Oceanic density (young)",
        min: 2.5,
        max: 3.2,
        step: 0.01,
      },
      {
        key: "oceanicDensityOld",
        label: "Oceanic density (old)",
        min: 2.5,
        max: 3.5,
        step: 0.01,
      },
      {
        key: "oceanicThickness",
        label: "Oceanic thickness",
        min: 1,
        max: 30,
        step: 0.5,
      },
      {
        key: "continentalThicknessThreshold",
        label: "Continental threshold",
        min: 5,
        max: 50,
        step: 0.5,
      },
      {
        key: "continentalRevertThickness",
        label: "Continental revert",
        min: 2,
        max: 20,
        step: 0.5,
      },
      {
        key: "continentalNucleationThickness",
        label: "Continental nucleation",
        min: 15,
        max: 40,
        step: 0.5,
      },
      {
        key: "maxCrustThickness",
        label: "Max crust thickness",
        min: 10,
        max: 120,
        step: 1,
      },
      {
        key: "delaminationRate",
        label: "Delamination /Myr",
        min: 0,
        max: 2,
        step: 0.05,
      },
      {
        key: "crustSpreadStart",
        label: "Crust spreads above",
        min: 8,
        max: 60,
        step: 0.5,
      },
      {
        key: "crustSpreadRate",
        label: "Crust spread rate /Myr",
        min: 0,
        max: 0.3,
        step: 0.005,
      },
      {
        key: "basinInfillRate",
        label: "Basin infill km/Myr",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "basinInfillMaxArea",
        label: "Basin infill max area",
        min: 0,
        max: 0.005,
        step: 0.0001,
      },
      {
        key: "minContinentArea",
        label: "Min continent area",
        min: 0,
        max: 0.001,
        step: 0.00001,
      },
      {
        key: "cohesionReach",
        label: "Cohesion reach (rings)",
        min: 0,
        max: 6,
        step: 1,
      },
      {
        key: "continentalAreaTarget",
        label: "Continental area target",
        min: 0,
        max: 0.7,
        step: 0.01,
      },
      {
        key: "continentalRecyclingRate",
        label: "Recycling /Myr (no target)",
        min: 0,
        max: 0.005,
        step: 0.0001,
      },
      {
        key: "cratonMinAge",
        label: "Craton min age (Myr)",
        min: 0,
        max: 3000,
        step: 50,
      },
      {
        key: "cratonInland",
        label: "Craton inland distance",
        min: 0,
        max: 0.3,
        step: 0.01,
      },
      {
        key: "cratonGrowthTime",
        label: "Craton growth (Myr)",
        min: 50,
        max: 3000,
        step: 50,
      },
      {
        key: "cratonDecayTime",
        label: "Craton erosion (Myr)",
        min: 10,
        max: 1000,
        step: 10,
      },
      {
        key: "cratonStrength",
        label: "Craton strength",
        min: 0,
        max: 10,
        step: 0.25,
      },
      {
        key: "cratonKeelGain",
        label: "Craton keel coupling",
        min: 0,
        max: 10,
        step: 0.25,
      },
      {
        key: "cratonDurability",
        label: "Craton durability",
        min: 0,
        max: 0.99,
        step: 0.01,
      },
      {
        key: "cratonMergeTime",
        label: "Craton merge (Myr)",
        min: 0,
        max: 500,
        step: 10,
      },
    ],
  },
  {
    title: "Advection",
    sliders: [
      {
        key: "accretionFraction",
        label: "Accretion fraction",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "subductionErosion",
        label: "Subduction erosion",
        min: 0,
        max: 0.5,
        step: 0.01,
      },
      {
        key: "marginErosion",
        label: "Margin erosion",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "maxThickeningPerMyr",
        label: "Max thickening /Myr",
        min: 0.1,
        max: 5,
        step: 0.1,
      },
      {
        key: "riftThicknessFloor",
        label: "Rift thickness floor",
        min: 0,
        max: 5,
        step: 0.1,
      },
      {
        key: "riftDensity",
        label: "Rift density",
        min: 2.5,
        max: 3.2,
        step: 0.01,
      },
    ],
  },
  {
    title: "Primordial era",
    sliders: [
      {
        key: "primordialHeat",
        label: "Primordial heat",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "primordialCoolingTime",
        label: "Cooling time (Myr)",
        min: 100,
        max: 5000,
        step: 100,
      },
      {
        key: "plumeRate",
        label: "Plumes /Myr",
        min: 0,
        max: 1,
        step: 0.01,
      },
      {
        key: "plumeRadius",
        label: "Plume radius (rad)",
        min: 0.02,
        max: 0.3,
        step: 0.01,
      },
      {
        key: "plumeDuration",
        label: "Plume duration (Myr)",
        min: 2,
        max: 100,
        step: 2,
      },
      {
        key: "plumeThickness",
        label: "Plume crust (km)",
        min: 15,
        max: 60,
        step: 1,
      },
      {
        key: "plumeKeel",
        label: "Plume keel",
        min: 0,
        max: 1,
        step: 0.05,
      },
      {
        key: "hotArcSuppression",
        label: "Hot arc suppression",
        min: 0,
        max: 2,
        step: 0.05,
      },
      {
        key: "hotDrip",
        label: "Hot drip",
        min: 0,
        max: 2,
        step: 0.05,
      },
    ],
  },
  {
    title: "Boundaries & timing",
    sliders: [
      { key: "dtMyr", label: "Myr per tick", min: 0.5, max: 10, step: 0.5 },
      {
        key: "activeSpeedEps",
        label: "Active speed threshold",
        min: 0,
        max: 0.0005,
        step: 0.00001,
      },
    ],
  },
];

const TUNING_GROUPS: TuningGroupSpec[] = TUNING_GROUP_FIELDS.map((group) => ({
  title: group.title,
  sliders: group.sliders.map((slider) => ({
    ...slider,
    tooltip: TUNING_TOOLTIPS[slider.key],
  })),
}));

/**
 * Framework-free overlay split into two panels. The left "Simulation" panel owns
 * world structure (subdivision, branches, seeds, run state) plus every tunable
 * simulation constant as live sliders; the right "View" panel owns what the user
 * sees (projection, view mode, overlays, sea level). The settings store remains
 * the single source of truth for view/structure settings; tuning sliders write
 * straight into the live `tuning` singleton and call `onTuningChange` so the
 * scene can recolour immediately. Clear-faults stays a dedicated action.
 */
export const createControls = (
  parent: HTMLElement,
  store: SettingsStore,
  onClearFaults: () => void,
  onTuningChange: () => void,
  onStepOnce: () => void,
): Controls => {
  const initial = store.getAll();
  const tooltip = makeTooltip(parent);

  // --- Simulation panel (top-left) -----------------------------------------
  const sim = makePanel("Simulation", "left");

  const levelLabel = makeLabel();
  const levelControl = makeSegmented(
    Array.from({ length: MAX_LEVEL - MIN_LEVEL + 1 }, (_, index) => {
      const level = MIN_LEVEL + index;
      return { value: level, label: String(level) };
    }),
    initial.level,
    (level) => {
      store.set({ level });
    },
    true,
  );

  const branchesLabel = makeLabel();
  const branchesSlider = makeSlider(
    MIN_BRANCHES,
    MAX_BRANCHES,
    1,
    initial.branchesPerSeed,
  );

  const seedToggle = makeToggle(
    "Place plate seeds (click globe)",
    initial.seedMode,
  );
  const runButton = makeButton("Run");
  runButton.setAttribute("aria-keyshortcuts", "Space");
  const paintRun = (running: boolean): void => {
    runButton.textContent = running ? "Pause" : "Run";
    runButton.setAttribute("aria-pressed", String(running));
    runButton.style.background = running ? ACCENT : "rgba(79, 143, 240, 0.18)";
    runButton.style.borderColor = running
      ? ACCENT
      : "rgba(120, 140, 180, 0.35)";
  };
  paintRun(initial.running);
  tooltip.attach(runButton, "Run or pause the simulation (Space).");
  const rateLabel = makeLabel();
  const rateSlider = makeSlider(
    MIN_TICK_RATE,
    MAX_TICK_RATE,
    1,
    initial.ticksPerSecond,
  );
  const renderRateLabel = (): void => {
    const rate = store.get("ticksPerSecond");
    rateLabel.textContent = `Sim rate: ${rate} ticks/s (${(
      rate * tuning.dtMyr
    ).toFixed(0)} Myr/s)`;
  };
  const stepButton = makeButton("Step once");
  const clearButton = makeButton("Clear faults");
  // A modelling choice rather than a dial: with it off, rifting is driven by
  // inherited weakness and slab tension alone and the mantle-heat field is
  // never touched, which is one fewer pass over every cell each tick.
  const insulationToggle = makeToggle(
    "Supercontinent insulation",
    simOptions.supercontinentInsulation,
  );
  insulationToggle.input.addEventListener("change", () => {
    setSimOptions({
      supercontinentInsulation: insulationToggle.input.checked,
    });
    onTuningChange();
  });
  tooltip.attach(
    insulationToggle.row,
    "Mantle heat builds under continents and can tear a supercontinent apart.",
  );

  const readout = document.createElement("div");
  Object.assign(readout.style, {
    display: "flex",
    justifyContent: "space-between",
    gap: "12px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);
  const cellEl = document.createElement("span");
  const plateEl = document.createElement("span");
  readout.append(cellEl, plateEl);

  const massEl = document.createElement("div");
  Object.assign(massEl.style, {
    marginTop: "6px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);

  const note = document.createElement("div");
  Object.assign(note.style, {
    marginTop: "6px",
    opacity: "0.7",
    fontSize: "11px",
    minHeight: "13px",
  } satisfies Partial<CSSStyleDeclaration>);

  // Tuning sliders, grouped and collapsible. Each remembers how to redraw its
  // own label so a "reset to defaults" can refresh them in place.
  const tuningHeading = document.createElement("div");
  tuningHeading.textContent = "Tuning";
  Object.assign(tuningHeading.style, {
    fontWeight: "600",
    margin: "12px 0 6px",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);

  interface SliderRef {
    spec: TuningSliderSpec;
    slider: HTMLInputElement;
    render: () => void;
  }
  const sliderRefs: SliderRef[] = [];

  const buildTuningGroup = (group: TuningGroupSpec): HTMLDetailsElement => {
    const details = document.createElement("details");
    Object.assign(details.style, {
      marginBottom: "6px",
      borderTop: "1px solid rgba(120, 140, 180, 0.18)",
      paddingTop: "6px",
    } satisfies Partial<CSSStyleDeclaration>);

    const summary = document.createElement("summary");
    summary.textContent = group.title;
    Object.assign(summary.style, {
      cursor: "pointer",
      opacity: "0.9",
      marginBottom: "6px",
    } satisfies Partial<CSSStyleDeclaration>);
    details.append(summary);

    for (const spec of group.sliders) {
      const label = makeLabel();
      const decimals = decimalsOf(spec.step);
      const slider = makeSlider(
        spec.min,
        spec.max,
        spec.step,
        tuning[spec.key],
      );
      const render = (): void => {
        label.textContent = `${spec.label}: ${tuning[spec.key].toFixed(
          decimals,
        )}`;
      };
      render();
      slider.addEventListener("input", () => {
        const value = clamp(Number(slider.value), spec.min, spec.max);
        setTuning({ [spec.key]: value } as Partial<Tuning>);
        render();
        if (spec.key === "dtMyr") renderRateLabel();
        onTuningChange();
        renderViewLegend();
      });
      sliderRefs.push({ spec, slider, render });
      const field = document.createElement("div");
      field.append(label, slider);
      tooltip.attach(field, spec.tooltip);
      details.append(field);
    }
    return details;
  };

  const tuningGroups = TUNING_GROUPS.map(buildTuningGroup);

  const resetButton = makeButton("Reset tuning to defaults");
  resetButton.addEventListener("click", () => {
    resetTuning();
    for (const { spec, slider, render } of sliderRefs) {
      slider.value = String(tuning[spec.key]);
      render();
    }
    onTuningChange();
    renderViewLegend();
  });
  tooltip.attach(
    resetButton,
    "Restore every tuning slider to its starting value.",
  );

  const diagnostics = createDiagnostics(parent);
  const diagnosticsButton = makeButton("Open diagnostics");
  diagnosticsButton.addEventListener("click", () => {
    diagnostics.open();
  });

  const renderLevelLabel = (): void => {
    levelLabel.textContent = `Subdivision level: ${store.get("level")}`;
  };
  const renderBranchesLabel = (): void => {
    branchesLabel.textContent = `Branches per seed: ${store.get(
      "branchesPerSeed",
    )}`;
  };
  renderLevelLabel();
  renderBranchesLabel();
  renderRateLabel();
  cellEl.textContent = "Cells: --";
  plateEl.textContent = "Plates: --";
  massEl.textContent = "Mass: --";

  branchesSlider.addEventListener("input", () => {
    store.set({
      branchesPerSeed: clamp(
        Math.round(Number(branchesSlider.value)),
        MIN_BRANCHES,
        MAX_BRANCHES,
      ),
    });
  });
  seedToggle.input.addEventListener("change", () => {
    store.set({ seedMode: seedToggle.input.checked });
  });
  runButton.addEventListener("click", () => {
    store.set({ running: !store.get("running") });
  });
  rateSlider.addEventListener("input", () => {
    store.set({
      ticksPerSecond: clamp(
        Math.round(Number(rateSlider.value)),
        MIN_TICK_RATE,
        MAX_TICK_RATE,
      ),
    });
  });
  stepButton.addEventListener("click", () => {
    onStepOnce();
  });
  clearButton.addEventListener("click", () => {
    onClearFaults();
  });

  sim.panel.append(
    levelLabel,
    levelControl.row,
    branchesLabel,
    branchesSlider,
    seedToggle.row,
    runButton,
    rateLabel,
    rateSlider,
    stepButton,
    clearButton,
    insulationToggle.row,
    readout,
    massEl,
    diagnostics.pinned,
    note,
    diagnosticsButton,
    tuningHeading,
    ...tuningGroups,
    resetButton,
  );

  // --- View panel (top-right) ----------------------------------------------
  const view = makePanel("View", "right");

  const projectionLabel = makeLabel();
  projectionLabel.textContent = "Projection";
  const projectionControl = makeSegmented<Projection>(
    [
      { value: "sphere", label: "Globe" },
      { value: "mercator", label: "Mercator" },
    ],
    initial.projection,
    (projection) => {
      store.set({ projection });
    },
  );

  const viewLabel = makeLabel();
  viewLabel.textContent = "View";
  const viewSelect = makeSelect(
    [
      { value: "colour", label: "Elevation colour" },
      { value: "data", label: "Data (all properties)" },
      { value: "crust", label: "Crust type" },
      { value: "density", label: "Density" },
      { value: "boundary", label: "Boundaries" },
    ],
    initial.viewMode,
  );

  const elevationLabel = makeLabel();
  elevationLabel.textContent = "Elevation scale";
  const elevationControl = makeSegmented<ElevationStyle>(
    [
      { value: "absolute", label: "Full" },
      { value: "land", label: "Land" },
    ],
    initial.elevationStyle,
    (elevationStyle) => {
      store.set({ elevationStyle });
    },
  );
  const elevationBlock = document.createElement("div");
  elevationBlock.append(elevationLabel, elevationControl.row);
  tooltip.attach(
    elevationBlock,
    "Full keeps ocean depth. Land paints sea level as the lowest colour and stretches the continental colours.",
  );

  const viewLegend = document.createElement("div");
  Object.assign(viewLegend.style, {
    margin: "-2px 0 10px",
    fontSize: "11px",
  } satisfies Partial<CSSStyleDeclaration>);

  const seaLabel = makeLabel();
  const seaSlider = makeSlider(
    MIN_SEA_LEVEL,
    MAX_SEA_LEVEL,
    0.5,
    initial.seaLevel,
  );
  const renderSeaLabel = (): void => {
    seaLabel.textContent = `Sea level: ${store.get("seaLevel").toFixed(1)}`;
  };
  renderSeaLabel();

  const rotateToggle = makeToggle("Auto-rotate", initial.autoRotate);
  const platesLabel = makeLabel();
  platesLabel.textContent = "Plates";
  const platesControl = makeSegmented<PlateOverlay>(
    [
      { value: "off", label: "Off" },
      { value: "lines", label: "Lines" },
      { value: "colours", label: "Colours" },
    ],
    initial.plateOverlay,
    (plateOverlay) => {
      store.set({ plateOverlay });
    },
  );
  const velocityToggle = makeToggle(
    "Show plate velocities",
    initial.showVelocities,
  );

  // Colour key for the boundary arrows; only meaningful while they are drawn,
  // so it follows the velocity toggle.
  const arrowLegend = document.createElement("div");
  Object.assign(arrowLegend.style, {
    display: initial.showVelocities ? "flex" : "none",
    gap: "10px",
    flexWrap: "wrap",
    margin: "-2px 0 8px",
    fontSize: "11px",
    opacity: "0.8",
  } satisfies Partial<CSSStyleDeclaration>);
  for (const entry of ARROW_LEGEND) {
    const item = document.createElement("span");
    Object.assign(item.style, {
      display: "flex",
      alignItems: "center",
      gap: "4px",
    } satisfies Partial<CSSStyleDeclaration>);
    const dash = document.createElement("span");
    Object.assign(dash.style, {
      width: "12px",
      height: "2px",
      borderRadius: "1px",
      background: entry.colour,
    } satisfies Partial<CSSStyleDeclaration>);
    const text = document.createElement("span");
    text.textContent = entry.label;
    item.append(dash, text);
    arrowLegend.append(item);
  }

  // Collapsible roster of every live plate: its overlay colour, how fast it is
  // moving and which way. Rebuilt wholesale on each update (there are tens of
  // plates at most, and the <details> wrapper keeps its open/closed state).
  const platesDetails = document.createElement("details");
  platesDetails.open = true;
  Object.assign(platesDetails.style, {
    marginBottom: "8px",
    borderTop: "1px solid rgba(120, 140, 180, 0.18)",
    paddingTop: "6px",
  } satisfies Partial<CSSStyleDeclaration>);
  const platesSummary = document.createElement("summary");
  platesSummary.textContent = "Plates";
  Object.assign(platesSummary.style, {
    cursor: "pointer",
    opacity: "0.9",
    marginBottom: "6px",
  } satisfies Partial<CSSStyleDeclaration>);
  const platesList = document.createElement("div");
  Object.assign(platesList.style, {
    maxHeight: "240px",
    overflowY: "auto",
    paddingRight: "2px",
  } satisfies Partial<CSSStyleDeclaration>);
  platesDetails.append(platesSummary, platesList);

  const fpsEl = document.createElement("div");
  Object.assign(fpsEl.style, {
    marginTop: "6px",
    fontVariantNumeric: "tabular-nums",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);
  fpsEl.textContent = "FPS: --";

  const timingEl = document.createElement("div");
  Object.assign(timingEl.style, {
    fontVariantNumeric: "tabular-nums",
    opacity: "0.75",
    fontSize: "11px",
  } satisfies Partial<CSSStyleDeclaration>);
  timingEl.textContent = "Sim: -- ms/tick · UI: -- ms";

  viewSelect.addEventListener("change", () => {
    store.set({ viewMode: viewSelect.value as ViewMode });
  });
  seaSlider.addEventListener("input", () => {
    store.set({
      seaLevel: clamp(Number(seaSlider.value), MIN_SEA_LEVEL, MAX_SEA_LEVEL),
    });
  });
  rotateToggle.input.addEventListener("change", () => {
    store.set({ autoRotate: rotateToggle.input.checked });
  });
  velocityToggle.input.addEventListener("change", () => {
    store.set({ showVelocities: velocityToggle.input.checked });
  });

  const renderViewLegend = (): void => {
    const viewMode = store.get("viewMode");
    const legend = legendForView(
      viewMode === "colour" ? "elevation" : viewMode,
      store.get("elevationStyle"),
      { min: densityMin(), max: densityMax() },
    );
    viewLegend.replaceChildren();
    if (legend.gradient) {
      const bar = document.createElement("div");
      Object.assign(bar.style, {
        height: "12px",
        borderRadius: "4px",
        background: legend.gradient,
        border: "1px solid rgba(120, 140, 180, 0.35)",
      } satisfies Partial<CSSStyleDeclaration>);
      const ticks = document.createElement("div");
      Object.assign(ticks.style, {
        position: "relative",
        height: "16px",
        marginTop: "2px",
        opacity: "0.8",
      } satisfies Partial<CSSStyleDeclaration>);
      for (const tick of legend.ticks) {
        const label = document.createElement("span");
        label.textContent = tick.label;
        const shift =
          tick.at <= 0
            ? "none"
            : tick.at >= 1
              ? "translateX(-100%)"
              : "translateX(-50%)";
        Object.assign(label.style, {
          position: "absolute",
          top: "0",
          left: `${tick.at * 100}%`,
          transform: shift,
          whiteSpace: "nowrap",
        } satisfies Partial<CSSStyleDeclaration>);
        ticks.append(label);
      }
      viewLegend.append(bar, ticks);
    }
    if (legend.swatches.length > 0) {
      const row = document.createElement("div");
      Object.assign(row.style, {
        display: "flex",
        flexWrap: "wrap",
        gap: "8px 12px",
        marginTop: legend.gradient ? "4px" : "0",
      } satisfies Partial<CSSStyleDeclaration>);
      for (const swatch of legend.swatches) {
        const item = document.createElement("span");
        Object.assign(item.style, {
          display: "flex",
          alignItems: "center",
          gap: "4px",
        } satisfies Partial<CSSStyleDeclaration>);
        const chip = document.createElement("span");
        Object.assign(chip.style, {
          width: "12px",
          height: "12px",
          borderRadius: "2px",
          background: swatch.colour,
          border: "1px solid rgba(230, 236, 245, 0.35)",
          flex: "0 0 auto",
        } satisfies Partial<CSSStyleDeclaration>);
        const text = document.createElement("span");
        text.textContent = swatch.label;
        item.append(chip, text);
        row.append(item);
      }
      viewLegend.append(row);
    }
    if (legend.note) {
      const caption = document.createElement("div");
      caption.textContent = legend.note;
      Object.assign(caption.style, {
        marginTop: "4px",
        opacity: "0.65",
        lineHeight: "1.35",
      } satisfies Partial<CSSStyleDeclaration>);
      viewLegend.append(caption);
    }
  };

  const syncElevationChrome = (): void => {
    elevationBlock.style.display =
      store.get("viewMode") === "colour" ? "" : "none";
    renderViewLegend();
  };
  syncElevationChrome();

  view.panel.append(
    projectionLabel,
    projectionControl.row,
    viewLabel,
    viewSelect,
    elevationBlock,
    viewLegend,
    seaLabel,
    seaSlider,
    rotateToggle.row,
    platesLabel,
    platesControl.row,
    velocityToggle.row,
    arrowLegend,
    platesDetails,
    fpsEl,
    timingEl,
  );

  // Reflect store changes (including programmatic ones) back into the panels.
  // Writing input.value/checked does not fire an "input"/"change" event, so this
  // cannot loop back into store.set.
  store.subscribe((changed) => {
    const state = store.getAll();
    if (changed.has("level")) {
      levelControl.setSelected(state.level);
      renderLevelLabel();
    }
    if (changed.has("seaLevel")) {
      seaSlider.value = String(state.seaLevel);
      renderSeaLabel();
    }
    if (changed.has("autoRotate")) {
      rotateToggle.input.checked = state.autoRotate;
    }
    if (changed.has("projection")) {
      projectionControl.setSelected(state.projection);
    }
    if (changed.has("viewMode")) {
      viewSelect.value = state.viewMode;
    }
    if (changed.has("elevationStyle")) {
      elevationControl.setSelected(state.elevationStyle);
    }
    if (changed.has("viewMode") || changed.has("elevationStyle")) {
      syncElevationChrome();
    }
    if (changed.has("seedMode")) {
      seedToggle.input.checked = state.seedMode;
    }
    if (changed.has("showVelocities")) {
      velocityToggle.input.checked = state.showVelocities;
      arrowLegend.style.display = state.showVelocities ? "flex" : "none";
    }
    if (changed.has("plateOverlay")) {
      platesControl.setSelected(state.plateOverlay);
    }
    if (changed.has("branchesPerSeed")) {
      branchesSlider.value = String(state.branchesPerSeed);
      renderBranchesLabel();
    }
    if (changed.has("running")) {
      paintRun(state.running);
    }
    if (changed.has("uiHidden")) applyUiHidden(state.uiHidden);
    if (changed.has("ticksPerSecond")) {
      rateSlider.value = String(state.ticksPerSecond);
      renderRateLabel();
    }
  });

  const hideButton = makeHideButton();
  tooltip.attach(hideButton, () =>
    store.get("uiHidden") ? "Show the panels (H)" : "Hide the panels (H)",
  );
  hideButton.addEventListener("click", () => {
    store.set({ uiHidden: !store.get("uiHidden") });
  });

  const applyUiHidden = (hidden: boolean): void => {
    const display = hidden ? "none" : "";
    sim.panel.style.display = display;
    view.panel.style.display = display;
    note.style.display = display;
    diagnostics.dialog.style.display = display;
    hideButton.setAttribute(
      "aria-label",
      hidden ? "Show interface" : "Hide interface",
    );
    hideButton.replaceChildren(eyeIcon(hidden));
  };
  applyUiHidden(initial.uiHidden);

  window.addEventListener("keydown", (event) => {
    if (event.repeat || event.metaKey || event.ctrlKey || event.altKey) return;
    if (isTypingTarget(event.target)) return;
    if (event.key.toLowerCase() === "h") {
      store.set({ uiHidden: !store.get("uiHidden") });
      return;
    }
    if (event.code !== "Space") return;
    // A focused button or select already uses Space. Leave that activation
    // alone so the run control is not toggled twice.
    const target = event.target;
    if (
      target instanceof HTMLButtonElement ||
      target instanceof HTMLSelectElement ||
      (target instanceof HTMLElement && target.tagName === "SUMMARY")
    ) {
      return;
    }
    event.preventDefault();
    store.set({ running: !store.get("running") });
  });

  parent.append(sim.panel, view.panel, hideButton);

  return {
    setCellCount: (count: number) => {
      cellEl.textContent = `Cells: ${count.toLocaleString()}`;
    },
    setPlateCount: (count: number) => {
      plateEl.textContent = `Plates: ${count.toLocaleString()}`;
    },
    setFps: (fps: number) => {
      fpsEl.textContent = `FPS: ${fps.toFixed(0)}`;
    },
    setTimings: (simMs: number, uiMs: number) => {
      timingEl.textContent = `Sim: ${simMs.toFixed(1)} ms/tick · UI: ${uiMs.toFixed(1)} ms`;
    },
    setMassReadout: (liveMass: number, drift: number) => {
      const driftPct = (drift * 100).toFixed(3);
      const sign = drift >= 0 ? "+" : "";
      massEl.textContent = `Crust mass: ${liveMass.toFixed(2)} (drift ${sign}${driftPct}%)`;
    },
    setNote: (text: string) => {
      note.textContent = text;
    },
    setMetrics: diagnostics.update,
    setPlates: (plates: readonly PlateSummary[]) => {
      platesSummary.textContent = `Plates (${plates.length})`;
      platesList.replaceChildren(
        ...(plates.length > 0
          ? plates.map(makePlateRow)
          : [makeEmptyPlateNote()]),
      );
    },
  };
};

/** Placeholder shown while the world has no plates to list. */
const makeEmptyPlateNote = (): HTMLDivElement => {
  const empty = document.createElement("div");
  empty.textContent = "No plates yet.";
  Object.assign(empty.style, {
    opacity: "0.6",
    fontSize: "11px",
  } satisfies Partial<CSSStyleDeclaration>);
  return empty;
};

/**
 * One plate row: a swatch in the plate's overlay colour, its stable id (the same
 * id the data view labels cells with), an arrow glyph rotated to its heading,
 * and its speed, bearing and share of the globe underneath.
 *
 * Speed is converted out of the simulation's rad/Myr into the cm/yr plate
 * motions are normally quoted in, so the numbers can be sanity-checked against
 * the real Earth (a few cm/yr).
 */
const makePlateRow = (plate: PlateSummary): HTMLDivElement => {
  const row = document.createElement("div");
  Object.assign(row.style, {
    padding: "4px 0",
    borderBottom: "1px solid rgba(120, 140, 180, 0.12)",
  } satisfies Partial<CSSStyleDeclaration>);

  const head = document.createElement("div");
  Object.assign(head.style, {
    display: "flex",
    alignItems: "center",
    gap: "7px",
  } satisfies Partial<CSSStyleDeclaration>);

  const swatch = document.createElement("span");
  Object.assign(swatch.style, {
    width: "11px",
    height: "11px",
    borderRadius: "3px",
    flex: "0 0 auto",
    background: toCssRgb(colourForPlate(plate.id)),
    border: "1px solid rgba(0, 0, 0, 0.35)",
  } satisfies Partial<CSSStyleDeclaration>);

  const name = document.createElement("span");
  name.textContent = `Plate ${plate.id}`;
  Object.assign(name.style, {
    flex: "1 1 auto",
  } satisfies Partial<CSSStyleDeclaration>);

  // The glyph points north at 0 deg and rotates clockwise, which is exactly how
  // the bearing is measured -- so the rotation is the bearing, unmodified.
  const heading = document.createElement("span");
  heading.textContent = plate.heading === null ? "·" : "↑";
  Object.assign(heading.style, {
    flex: "0 0 auto",
    opacity: plate.heading === null ? "0.4" : "0.95",
    transform:
      plate.heading === null
        ? "none"
        : `rotate(${plate.heading.toFixed(1)}deg)`,
  } satisfies Partial<CSSStyleDeclaration>);

  head.append(swatch, name, heading);

  const detail = document.createElement("div");
  const share = `${(plate.areaShare * 100).toFixed(1)}% of globe`;
  detail.textContent =
    plate.heading === null
      ? `still · ${share}`
      : `${(plate.speed * CM_PER_YR_PER_RAD_MYR).toFixed(1)} cm/yr · ` +
        `${plate.heading.toFixed(0).padStart(3, "0")}° · ${share}`;
  Object.assign(detail.style, {
    marginLeft: "18px",
    fontSize: "11px",
    opacity: "0.65",
    fontVariantNumeric: "tabular-nums",
  } satisfies Partial<CSSStyleDeclaration>);

  row.append(head, detail);
  return row;
};

interface Panel {
  panel: HTMLDivElement;
  title: HTMLDivElement;
}

/** A fixed, scrollable glass panel pinned to one side of the viewport. */
const makePanel = (titleText: string, side: "left" | "right"): Panel => {
  const panel = document.createElement("div");
  Object.assign(panel.style, {
    position: "fixed",
    top: "12px",
    padding: "12px 14px",
    background: "rgba(10, 14, 22, 0.78)",
    border: "1px solid rgba(120, 140, 180, 0.25)",
    borderRadius: "10px",
    color: "#e6ecf5",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    backdropFilter: "blur(6px)",
    userSelect: "none",
    minWidth: "240px",
    maxWidth: "300px",
    maxHeight: "calc(100vh - 24px)",
    overflowY: "auto",
  } satisfies Partial<CSSStyleDeclaration>);
  panel.style[side] = "12px";

  const title = document.createElement("div");
  title.textContent = titleText;
  Object.assign(title.style, {
    fontWeight: "600",
    marginBottom: "10px",
    letterSpacing: "0.02em",
  } satisfies Partial<CSSStyleDeclaration>);
  panel.append(title);

  return { panel, title };
};

interface SegmentedOption<T extends string | number> {
  value: T;
  label: string;
}

interface SegmentedControl<T extends string | number> {
  row: HTMLDivElement;
  setSelected: (value: T) => void;
}

/**
 * A row of buttons of which one is the current choice, drawn in the accent
 * colour. `square` is for the subdivision levels, whose labels are digits.
 */
const makeSegmented = <T extends string | number>(
  options: readonly SegmentedOption<T>[],
  selected: T,
  onPick: (value: T) => void,
  square = false,
): SegmentedControl<T> => {
  const row = document.createElement("div");
  Object.assign(row.style, {
    display: square ? "grid" : "flex",
    gridTemplateColumns: square
      ? `repeat(${options.length}, minmax(0, 1fr))`
      : "none",
    gap: "4px",
    marginBottom: "10px",
  } satisfies Partial<CSSStyleDeclaration>);

  const buttons: { value: T; button: HTMLButtonElement }[] = [];
  const setSelected = (value: T): void => {
    for (const entry of buttons) {
      const active = entry.value === value;
      entry.button.style.background = active ? ACCENT : "rgba(20, 26, 38, 0.9)";
      entry.button.style.borderColor = active
        ? ACCENT
        : "rgba(120, 140, 180, 0.35)";
      entry.button.style.fontWeight = active ? "600" : "400";
    }
  };

  for (const option of options) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = option.label;
    Object.assign(button.style, {
      flex: square ? "none" : "1 1 0",
      minWidth: "0",
      aspectRatio: square ? "1" : "auto",
      padding: square ? "0" : "5px 4px",
      background: "rgba(20, 26, 38, 0.9)",
      color: "#e6ecf5",
      border: "1px solid rgba(120, 140, 180, 0.35)",
      borderRadius: "6px",
      font: "13px/1.2 ui-sans-serif, system-ui, sans-serif",
      cursor: "pointer",
    } satisfies Partial<CSSStyleDeclaration>);
    button.addEventListener("click", () => {
      setSelected(option.value);
      onPick(option.value);
    });
    buttons.push({ value: option.value, button });
    row.append(button);
  }
  setSelected(selected);
  return { row, setSelected };
};

/** One fixed tooltip, moved next to whichever control is hovered. */
const makeTooltip = (
  parent: HTMLElement,
): {
  attach: (target: HTMLElement, text: string | (() => string)) => void;
} => {
  const tip = document.createElement("div");
  Object.assign(tip.style, {
    position: "fixed",
    zIndex: "50",
    display: "none",
    maxWidth: "240px",
    padding: "6px 8px",
    background: "rgba(10, 14, 22, 0.96)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.4)",
    borderRadius: "6px",
    font: "12px/1.4 ui-sans-serif, system-ui, sans-serif",
    pointerEvents: "none",
    boxShadow: "0 8px 24px rgba(0, 0, 0, 0.35)",
  } satisfies Partial<CSSStyleDeclaration>);
  parent.append(tip);

  const hide = (): void => {
    tip.style.display = "none";
  };
  window.addEventListener("scroll", hide, true);

  const attach = (target: HTMLElement, text: string | (() => string)): void => {
    const read = (): string => (typeof text === "function" ? text() : text);
    target.addEventListener("mouseenter", () => {
      tip.textContent = read();
      tip.style.display = "block";
      tip.style.left = "0px";
      tip.style.top = "0px";
      const tipRect = tip.getBoundingClientRect();
      const rect = target.getBoundingClientRect();
      let left = rect.right + 8;
      let top = rect.top;
      if (left + tipRect.width > window.innerWidth - 8) {
        left = rect.left - tipRect.width - 8;
      }
      if (top + tipRect.height > window.innerHeight - 8) {
        top = window.innerHeight - tipRect.height - 8;
      }
      tip.style.left = `${Math.max(8, left)}px`;
      tip.style.top = `${Math.max(8, top)}px`;
    });
    target.addEventListener("mouseleave", hide);
  };

  return { attach };
};

const isTypingTarget = (target: EventTarget | null): boolean => {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable
  );
};

const SVG_NS = "http://www.w3.org/2000/svg";

/** An eye, slashed once the panels are hidden. */
const eyeIcon = (slashed: boolean): SVGSVGElement => {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "18");
  svg.setAttribute("height", "18");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.8");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  const eye = document.createElementNS(SVG_NS, "path");
  eye.setAttribute("d", "M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12z");
  const pupil = document.createElementNS(SVG_NS, "circle");
  pupil.setAttribute("cx", "12");
  pupil.setAttribute("cy", "12");
  pupil.setAttribute("r", "2.5");
  svg.append(eye, pupil);
  if (slashed) {
    const slash = document.createElementNS(SVG_NS, "path");
    slash.setAttribute("d", "M4 20L20 4");
    svg.append(slash);
  }
  return svg;
};

const makeHideButton = (): HTMLButtonElement => {
  const button = document.createElement("button");
  button.type = "button";
  Object.assign(button.style, {
    position: "fixed",
    right: "12px",
    bottom: "12px",
    zIndex: "40",
    width: "36px",
    height: "36px",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: "0",
    background: "rgba(10, 14, 22, 0.78)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.35)",
    borderRadius: "8px",
    cursor: "pointer",
    backdropFilter: "blur(6px)",
  } satisfies Partial<CSSStyleDeclaration>);
  return button;
};

interface SelectOption {
  value: string;
  label: string;
}

const makeSelect = (
  options: SelectOption[],
  selected: string,
): HTMLSelectElement => {
  const select = document.createElement("select");
  Object.assign(select.style, {
    width: "100%",
    marginBottom: "10px",
    padding: "4px 6px",
    background: "rgba(20, 26, 38, 0.9)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.3)",
    borderRadius: "6px",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  for (const option of options) {
    const element = document.createElement("option");
    element.value = option.value;
    element.textContent = option.label;
    if (option.value === selected) element.selected = true;
    select.append(element);
  }
  return select;
};

const makeButton = (labelText: string): HTMLButtonElement => {
  const button = document.createElement("button");
  button.textContent = labelText;
  Object.assign(button.style, {
    width: "100%",
    marginBottom: "10px",
    padding: "6px 8px",
    background: "rgba(79, 143, 240, 0.18)",
    color: "#e6ecf5",
    border: "1px solid rgba(120, 140, 180, 0.35)",
    borderRadius: "6px",
    font: "13px/1.4 ui-sans-serif, system-ui, sans-serif",
    cursor: "pointer",
  } satisfies Partial<CSSStyleDeclaration>);
  return button;
};

interface Toggle {
  row: HTMLLabelElement;
  input: HTMLInputElement;
}

const makeToggle = (labelText: string, checked: boolean): Toggle => {
  const row = document.createElement("label");
  Object.assign(row.style, {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "10px",
    marginBottom: "8px",
    cursor: "pointer",
    opacity: "0.95",
  } satisfies Partial<CSSStyleDeclaration>);

  const text = document.createElement("span");
  text.textContent = labelText;

  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  Object.assign(input.style, {
    width: "32px",
    height: "18px",
    cursor: "pointer",
    accentColor: "#4f8ff0",
  } satisfies Partial<CSSStyleDeclaration>);

  row.append(text, input);
  return { row, input };
};

const makeLabel = (): HTMLLabelElement => {
  const label = document.createElement("label");
  Object.assign(label.style, {
    display: "block",
    marginBottom: "4px",
    opacity: "0.85",
  } satisfies Partial<CSSStyleDeclaration>);
  return label;
};

const makeSlider = (
  min: number,
  max: number,
  stepValue: number,
  value: number,
): HTMLInputElement => {
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = String(min);
  slider.max = String(max);
  slider.step = String(stepValue);
  slider.value = String(value);
  Object.assign(slider.style, {
    width: "100%",
    marginBottom: "10px",
    accentColor: "#4f8ff0",
  } satisfies Partial<CSSStyleDeclaration>);
  return slider;
};

/** Decimal places implied by a slider step, for tidy value labels. */
const decimalsOf = (step: number): number => {
  const s = String(step);
  const dot = s.indexOf(".");
  return dot < 0 ? 0 : s.length - dot - 1;
};
