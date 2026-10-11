/**
 * The shapes and Earth reference values of the diagnostics. The metrics are
 * measured by the Rust simulation (`sim-rs/src/metrics.rs`), which serialises
 * them into exactly these shapes.
 */

/**
 * What Earth actually looks like, for the diagnostics panel to sit its numbers
 * next to. The point of this module is that "closer to real geology" is a claim
 * that can be checked rather than eyeballed, and these are the checks:
 *
 * - **Continental area.** Continental crust covers ~40% of the globe once the
 *   submerged margins are counted (dry land alone is ~29%).
 * - **Sea-floor age.** Ocean floor is young and none of it is old: the mean is
 *   ~60 Myr and the oldest surviving crust is ~180 Myr, because everything
 *   older has been subducted. The age-area distribution is close to triangular
 *   — most of the sea floor is young — which is the signature of steady
 *   creation at ridges balanced by consumption at trenches.
 * - **Plate count.** ~7 major plates plus a similar number of minor ones, and
 *   roughly steady over time: rifting mints new plates about as fast as
 *   subduction and boundary death retire them.
 * - **Speed against trench length.** The Forsyth & Uyeda (1975) result: a
 *   plate's speed tracks the fraction of its perimeter that is a subduction
 *   zone consuming *its own* crust, and barely correlates with its area. Slab
 *   pull is the dominant driving force, so this correlation is the sharpest
 *   single test of whether the force balance is behaving like Earth's.
 */
export const EARTH = {
  continentalArea: 0.4,
  meanOceanAge: 60,
  maxOceanAge: 180,
  majorPlates: 7,
  /**
   * Plates of every size. Earth's seven majors are joined by about as many
   * minor plates again, plus a long tail of microplates whose count depends
   * entirely on where the catalogue draws the line.
   */
  totalPlates: 15,
  speedTrenchCorrelation: 0.8,
  /**
   * How long the ocean floor takes to replace itself. Earth turns over its
   * entire sea floor in about the age of its oldest surviving crust, which is
   * the same statement twice: nothing older than ~180 Myr is left *because* the
   * turnover takes that long. A world whose recycling time is far longer is not
   * recycling, and its sea floor will age without limit however many ridges it
   * has.
   */
  oceanRecyclingMyr: 180,
  surfaceHeatTw: 46,
  radiogenicHeatTw: 20,
} as const;

/**
 * Width (Myr) of each sea-floor age bin, and how many bins are kept. Exported
 * because the chart that draws the histogram has to bin identically.
 */
export const AGE_BIN_MYR = 20;
export const AGE_BINS = 10;

/** What drives one plate: how fast it goes, and how much slab it has to pull it. */
export interface PlateDrive {
  id: number;
  /** Surface speed at the plate's centroid (cm/yr). */
  speed: number;
  /** Share of the plate's boundary along which its OWN crust is subducting. */
  trenchFraction: number;
  /** Share of the globe's surface this plate covers. */
  areaShare: number;
}

/** A snapshot of how Earth-like the world currently is. See {@link EARTH}. */
export interface WorldMetrics {
  /** Simulated time since the world was built (Myr). */
  elapsedMyr: number;
  plateCount: number;
  /**
   * Plates covering at least 2% of the surface. Earth's seven majors all clear
   * that comfortably and its largest minor plates sit just under, so this is
   * the honest comparison against "seven major plates": a raw count is
   * dominated by however many slivers the grid happens to be carrying.
   */
  majorPlateCount: number;
  /** Share of the surface carrying continental crust, in [0, 1]. */
  continentalArea: number;
  /** Share of all crust mass that is continental, in [0, 1]. */
  continentalMassShare: number;
  /** Area-weighted mean thickness of continental crust (km). */
  meanContinentalThickness: number;
  /**
   * Share of the surface flagged continental but thinner than the threshold
   * that made it so: crust stretched thin that still counts towards
   * {@link continentalArea}.
   */
  thinContinentalArea: number;
  /**
   * Share of the surface turned continental per Myr this tick, split by
   * whether arc accretion or lateral spreading carried it past the threshold.
   */
  continentalisedByAccretion: number;
  continentalisedBySpreading: number;
  /** Share of the surface per Myr turned continental by trapped ocean filling in. */
  continentalisedByInfill: number;
  /** Share of the surface per Myr turned continental over primordial plumes. */
  continentalisedByPlume: number;
  /**
   * Share of the surface per Myr of ocean floor turned continental with neither
   * accretion nor spreading into it: crust piled up where the transport had
   * nowhere to put it, or spilled from a full arc cell beside it. Most of the
   * model's continental growth comes this way.
   */
  continentalisedByPiling: number;
  /**
   * Share of the oceanic mass per Myr of thickened ocean floor that foundered
   * into a hot primordial mantle instead of becoming continent.
   */
  dripped: number;
  /** Mantle temperature above today's, in roughly 300 K units. */
  mantleExcess: number;
  /** Plumes building crust right now. */
  activePlumes: number;
  /** Total heat leaving the surface (TW). */
  surfaceHeatTw: number;
  /** Heat supplied by radioactive decay (TW). */
  radiogenicTw: number;
  /** Heat supplied by the core (TW). */
  coreHeatTw: number;
  /** Mantle potential temperature (K). */
  mantleTemp: number;
  /** Temperature at the top of the core (K). */
  coreTemp: number;
  /** Current mantle cooling rate (K/Gyr); negative means warming. */
  mantleCooling: number;
  /** Coolest upper-mantle site (K below the planet's mantle temperature if negative). */
  mantleAnomalyMin: number;
  /** Hottest upper-mantle site (K above the planet's mantle temperature). */
  mantleAnomalyMax: number;
  /** Upper-mantle anomaly averaged over the globe by area (K): 0 but where sites are held at the limit. */
  mantleAnomalyMean: number;
  /** Slab piled in the lower mantle, as a multiple of the globe's area. */
  pileMass: number;
  /** Largest mantle site's plume weight over the mean: 1 when plumes rise anywhere alike. */
  plumeWeightPeak: number;
  /** Lowest dynamic topography over any cell (km): the mantle's deepest drag on the surface. */
  dynTopoLow: number;
  /** Highest dynamic topography over any cell (km): the mantle's greatest lift. */
  dynTopoHigh: number;
  /** Share of the globe standing above sea level (0 km). */
  landArea: number;
  /**
   * Share of the surface per Myr of continental crust recycled into the mantle
   * at active margins. Levelled off, it matches {@link continentalGrowth}.
   */
  continentalRecycled: number;
  /**
   * Share of the surface per Myr by which everything but recycling grows the
   * continents, averaged over 200 Myr.
   */
  continentalGrowth: number;
  /** Share of the continental mass lost per Myr to subduction erosion. */
  continentalEroded: number;
  /** Share of the continental mass shed per Myr by over-thick roots. */
  delaminated: number;
  /**
   * Share of the continental mass per Myr of accreted crust that could not be
   * taken up at the thickening cap and foundered instead of spilling.
   */
  foundered: number;
  /**
   * Share of the surface per Myr of continental crust that thinned below the
   * revert threshold and was reclassified as oceanic.
   */
  decontinentalised: number;
  /**
   * Share of the surface per Myr vacated with no other plate beside it: gaps
   * the quantised transport tears inside a plate, which on Earth do not exist.
   */
  interiorHoles: number;
  /**
   * Of {@link interiorHoles}, the share no nearby crust of the plate could
   * fill, left to open as rifts. Each one inside a continent leaves a pocket
   * of ocean behind.
   */
  interiorHolesUnfilled: number;
  /**
   * Share of the surface per Myr of continental crust the transport left with
   * nowhere to go, welded onto whatever moved into its cell.
   */
  tornContinent: number;
  /**
   * Share of the surface per Myr of stray continental crust the cohesion pass
   * put back onto the landmass it broke from. Tracking {@link tornContinent}
   * with no fall in {@link coastShare} means the same crust is being torn off
   * and put back every step.
   */
  rejoined: number;
  /**
   * Share of continental area touching ocean, in [0, 1]. Speckled continents
   * and pock-marked interiors read high; cohesive landmasses low.
   */
  coastShare: number;
  /**
   * Landmasses that are a single cell or smaller than `minContinentArea` of
   * the globe: crumbs of continent adrift on their own.
   */
  smallLandmassCount: number;
  /**
   * Bodies of ocean ringed by continent and smaller than about the Caspian.
   * Earth has a handful; a model pock-marking its continents has hundreds.
   */
  oceanPocketCount: number;
  /** Share of the surface in those pockets, in [0, 1]. */
  oceanPocketArea: number;
  /**
   * Connected landmasses covering at least 0.5% of the globe, whichever plates
   * they ride on. Earth has five (Afro-Eurasia, the two Americas, Antarctica,
   * Australia).
   */
  continentCount: number;
  /**
   * Share of all continental crust in the largest landmass, in [0, 1].
   * Afro-Eurasia holds about half of Earth's; a supercontinent nearly all.
   */
  largestContinentShare: number;
  /**
   * Share of continental crust that is craton, weighted by how far each cell
   * has grown its keel, in [0, 1]. Archean and Palaeoproterozoic cratons make
   * up very roughly a sixth to a third of Earth's continents.
   */
  cratonShare: number;
  /** Area-weighted mean age of oceanic crust (Myr). */
  meanOceanAge: number;
  /** Age of the oldest surviving oceanic crust (Myr). */
  maxOceanAge: number;
  /**
   * Myr for the present rate of subduction to consume the ocean floor once
   * over. `null` while nothing is subducting at all.
   */
  oceanRecyclingMyr: number | null;
  /**
   * Share of the ocean floor in each {@link AGE_BIN_MYR}-wide age bin, oldest
   * bin collecting everything beyond. Earth's is close to triangular.
   */
  ageHistogram: number[];
  /** Per-plate drive, largest plate first. */
  drives: PlateDrive[];
  /**
   * Share of total plate-boundary length by kind, in the order convergent,
   * divergent, transform, inactive. A closed sphere cannot diverge everywhere,
   * so a world with no convergence left in it has stopped recycling its crust —
   * which is the failure that shows up first and is hardest to see any other
   * way.
   */
  boundaryShare: [number, number, number, number];
  /**
   * Correlation between plate speed and the share of its perimeter that is a
   * trench consuming its own crust, over plates covering at least 1% of the
   * globe.
   * `null` when there are too few such plates to correlate.
   */
  speedTrenchCorrelation: number | null;
}
