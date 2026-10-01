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
