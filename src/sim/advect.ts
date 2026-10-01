import type { CellData } from "../data/CellData";
import { cross, dot, length, type Vec3 } from "../math/vec3";
import type { CellMesh } from "../mesh/CellMesh";
import type { PlateData } from "../plates/PlateData";
import { type BoundaryField, BoundaryKind } from "./boundaries";
import type { AdvectionField } from "./massBudget";
import { totalCrustMass } from "./massBudget";
import { tuning } from "./tuning";

/**
 * The accretion fraction (how much of a subducting cell's mass accretes onto the
 * survivor) and the rift placeholder density now live in {@link tuning} so the UI
 * can tweak them live; they are read inside {@link advect} below.
 */

/** Crust type codes mirrored from {@link CellData.crustType}. */
const OCEANIC = 0;
const CONTINENTAL = 1;

/**
 * How much of a cell's ring an advancing plate must already hold before it may
 * take that cell. Two neighbours is what a cell just across a straight boundary
 * has, so this admits an ordinary front while rejecting a cell touched at a
 * single point -- the diagonal nick that a finger would have to start from.
 */
const MIN_FOOTHOLD = 2;

/**
 * Mean angular width of one cell: the displacement a plate must bank before its
 * crust has moved far enough to occupy the next cell along.
 */
const cellWidth = (mesh: CellMesh): number =>
  Math.sqrt((4 * Math.PI) / mesh.cellCount);

/**
 * Move a cell's crust from `source` to `dest`, conserving its mass.
 *
 * Thickness is scaled by the ratio of the two cells' areas. The cells of a
 * Goldberg polyhedron are not all the same size -- the twelve pentagons are
 * about a fifth smaller than their hexagonal neighbours, and even the hexagons
 * vary a few percent -- so carrying a column of crust across at constant
 * thickness quietly changes how much crust it is. Over a long run that is a slow
 * one-way leak in the mass budget. A column squeezed into a smaller cell gets
 * thicker, which is what conservation means here.
 */
const copyCell = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  source: number,
  dest: number,
): void => {
  dst.elevation[dest] = src.elevation[source];
  dst.plateId[dest] = src.plateId[source];
  dst.crustType[dest] = src.crustType[source];
  dst.age[dest] = src.age[source];
  dst.density[dest] = src.density[source];
  // Weakness is a property of the lithosphere itself, so it travels with the
  // crust: a suture carried halfway round the world is still the line the next
  // rift will follow. Mantle heat is a property of the mantle *below*, which
  // does not travel with the plate, so it is deliberately not carried here.
  dst.weakness[dest] = src.weakness[source];
  dst.thickness[dest] =
    source === dest
      ? src.thickness[source]
      : (src.thickness[source] * mesh.area(source)) / mesh.area(dest);
};

/** Unit direction from cell `a` to cell `b`, projected off nothing: the chord. */
const chordDir = (mesh: CellMesh, a: number, b: number): Vec3 => {
  const p = mesh.position(a);
  const q = mesh.position(b);
  const d: Vec3 = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
  const len = length(d) || 1;
  return [d[0] / len, d[1] / len, d[2] / len];
};

/**
 * Advance one tick of plate motion.
 *
 * The model follows what lithosphere actually does, which is the opposite of
 * letting cells change hands wherever a resampled position happens to land:
 *
 * - **Plates move in whole cells.** A plate banks the displacement it earns each
 *   tick and, once it has a full cell width, its entire sheet steps one cell
 *   along. Between those steps it is bit-for-bit unchanged. Resampling a
 *   rotation onto an unstructured mesh every tick is what produced the corduroy
 *   striping: `nearest cell` is not a bijection, so some cells were read twice
 *   (a duplicated row of crust) and others not at all (a dropped row).
 * - **Transport is an exact permutation.** Each cell of a stepping plate takes
 *   its crust from the neighbour directly behind it, and every source cell is
 *   consumed by at most one destination. Nothing is duplicated and nothing is
 *   silently lost; crust with nowhere to go is subducted through the ledger.
 * - **Ownership changes only where crust actually arrives.** A plate takes a
 *   foreign cell when the step carries one of its own cells onto it across a
 *   convergent margin it overrides. A plate that is not moving cannot annex
 *   anything.
 * - **Buoyancy decides which side goes down**, and it decides it for a whole
 *   margin at once. Subduction polarity is a property of a boundary, not of a
 *   single 30 km patch of it: sea-floor age varies enough along a margin to flip
 *   a per-cell comparison from one cell to the next, and the cells that won then
 *   poked forward as lobes. Continental crust always overrides oceanic, and
 *   between two oceanic margins the denser (older, cooler) side goes down.
 * - **A margin is a smooth arc.** Lithosphere is a stiff sheet, so the boundary
 *   is held smooth by curvature: a cell with four or more of its ring in one
 *   other plate is a spur too narrow to hold and is handed over, which planes
 *   off anything up to two cells wide every step. Protruding is not detected
 *   and repaired after the fact -- it cannot persist.
 * - **Thickened crust spreads under its own weight, and nothing is destroyed
 *   for want of room.** A crustal root has no strength to hold itself up, so an
 *   orogen flows outwards instead of piling into a one-cell wall. Anything a
 *   cell still cannot hold is handed on to its neighbours until it has all been
 *   placed -- an accretionary wedge and a collision range widen, they do not
 *   evaporate. Destroying the overflow at a thickness ceiling instead made a
 *   continental collision lose crust, which is the one thing a collision never
 *   does.
 * - **A rift inside a continent stretches it; it does not melt new sea floor.**
 *   The crust filling a gap is drawn out of the neighbouring cells, so a
 *   continent thins as it pulls apart. Fresh ocean is minted only where there is
 *   nothing left to stretch, which is what a mid-ocean ridge is.
 * - **Continental crust never subducts; it folds.** Whatever a continental cell
 *   meets, it is too buoyant to be drawn down: its crust welds onto the cell
 *   that took it, horizontal shortening traded for thickness, which is what
 *   builds a collision range. Once the pile has no headroom left the front locks
 *   and the boundary stops migrating, as a real collision does. Continental
 *   crust therefore only ever shrinks by the three routes that shrink it in
 *   reality, all of them minor next to how much survives: a continent rifting
 *   apart, so its trailing crust is replaced by sea floor at the new spreading
 *   centre; subduction erosion of an overriding margin; and continentally
 *   derived sediment carried down with a slab. The last two are the
 *   {@link Tuning.subductionErosion} fraction. Every other interaction adds to
 *   it, which is why the continental fraction grows and then settles rather
 *   than being ground away.
 * - **Trailing edges rift, unless something follows.** A cell a stepping plate
 *   has pulled off goes to an adjacent plate whose own step lands on it, and
 *   only becomes fresh ocean if nothing moves in behind. Without that a plate
 *   being consumed would keep minting ocean at its back and never shrink, and no
 *   ocean basin could ever close.
 *
 * A subducting cell gives {@link Tuning.accretionFraction} of its mass to the
 * overriding cell and the rest is ledgered, so
 * `liveMass + subductedTotal - createdTotal` stays closed.
 *
 * Pure over the mesh interface and flat typed arrays, so it can later move into
 * a Web Worker / WASM without restructuring upstream.
 */
export const advect = (
  mesh: CellMesh,
  src: CellData,
  dst: CellData,
  plateData: PlateData,
  dtMyr: number,
  boundaries: BoundaryField,
  out: AdvectionField,
): void => {
  const { cellCount } = mesh;
  const { winner, accreted, welded, riftCell, subducted } = out;

  winner.fill(-1);
  accreted.fill(0);
  welded.fill(0);
  riftCell.fill(0);
  subducted.fill(0);
  let subductedThisStep = 0;

  const width = cellWidth(mesh);

  // --- Quantised rigid motion ----------------------------------------------
  // Each plate banks the displacement it earns this tick; once it has a whole
  // cell's worth its entire sheet steps one cell along, and the remainder is
  // carried over so the long-run speed still matches |omega|.
  const stepping = new Set<number>();
  for (const [id, props] of plateData.byId) {
    props.pending = Math.min(
      props.pending + length(props.omega) * dtMyr,
      2 * width,
    );
    if (props.pending >= width) {
      props.pending -= width;
      stepping.add(id);
    }
  }

  // --- Where this step carries each cell's crust ---------------------------
  // For a stepping plate, the neighbour most closely aligned with the cell's own
  // surface velocity: one cell along, in the direction the plate is going.
  const forward = new Int32Array(cellCount).fill(-1);
  for (let s = 0; s < cellCount; s++) {
    const plate = src.plateId[s];
    if (plate < 0 || !stepping.has(plate)) continue;
    const props = plateData.byId.get(plate);
    if (!props) continue;
    const vel = cross(props.omega, mesh.position(s));
    const speed = length(vel);
    if (speed === 0) continue;
    const dir: Vec3 = [vel[0] / speed, vel[1] / speed, vel[2] / speed];
    const nbrs = mesh.neighbors(s);
    let best = -1;
    let bestDot = 0;
    for (let k = 0; k < nbrs.length; k++) {
      const a = dot(chordDir(mesh, s, nbrs[k]), dir);
      if (a > bestDot) {
        bestDot = a;
        best = nbrs[k];
      }
    }
    forward[s] = best;
  }

  /**
   * Does `a`'s crust ride over `b`'s when the two meet? Continental crust is
   * buoyant enough that it always overrides oceanic crust regardless of the
   * density numbers; between two cells of the same kind the lighter one wins,
   * with the index breaking exact ties so the result never depends on
   * iteration order.
   */
  const overrides = (a: number, b: number): boolean => {
    const aCont = src.crustType[a] === CONTINENTAL;
    const bCont = src.crustType[b] === CONTINENTAL;
    if (aCont !== bCont) return aCont;
    if (src.age[a] !== src.age[b]) return src.age[a] < src.age[b];
    return a < b;
  };

  /**
   * Crust can only pile up so far: anything beyond this founders into the
   * mantle instead of stacking without bound, which would blow up elevation.
   * It has to sit well above the continental threshold, or a collision has no
   * headroom to thicken into and simply destroys the arriving continent.
   */
  const thicknessCap = Math.max(
    tuning.maxCrustThickness,
    tuning.continentalThicknessThreshold,
  );

  // --- Pass 1: who takes which cell ----------------------------------------
  // The only way a cell changes plate, and it is decided a whole margin at a
  // time rather than cell by cell. Subduction polarity is a property of the
  // boundary, not of any one 30 km patch of it: a margin has one down-going
  // side along its whole length, so the tally below compares the mean buoyancy
  // of the two sides and the entire front then advances -- or does not -- as a
  // unit. Deciding it per cell is what let a boundary fray, because the age (and
  // so density) of adjacent sea floor varies enough to flip the winner from one
  // cell to the next, and the winning cells then poked forward as lobes.
  const newPlate = new Int32Array(cellCount);
  newPlate.set(src.plateId);
  const claimant = new Int32Array(cellCount).fill(-1);
  const eaten = new Uint8Array(cellCount);

  /**
   * How readily a cell goes down at a trench: lower rides higher.
   *
   * Ranked by *age*, not density. Oceanic crust cools toward a fixed density and
   * stops changing once it passes {@link Tuning.maxOceanicAge}, so ranking by
   * density made every piece of old sea floor identical -- and a margin whose
   * two sides tie is a margin where nothing subducts. A world left running long
   * enough aged all of its ocean past that point and then simply stopped
   * recycling: nothing sank, so nothing was young, so nothing sank. Age keeps
   * separating crust however old it gets, and it is the physical variable
   * anyway. The older plate is the colder, thicker one, and that is the one that
   * sinks -- the Pacific under the Philippine Sea, not the other way round.
   *
   * Continental crust is scored a full age-scale below the youngest ocean: it is
   * not merely the more buoyant option, it is the one that cannot go down.
   */
  const buoyancy = (c: number): number =>
    src.crustType[c] === CONTINENTAL ? -tuning.maxOceanicAge : src.age[c];

  // Every place this step drives one plate's crust onto another's, and the
  // per-margin tally of which side is the buoyant one.
  const fromCell: number[] = [];
  const ontoCell: number[] = [];
  const margins = new Map<number, { mine: number; theirs: number }>();
  for (let s = 0; s < cellCount; s++) {
    const plate = src.plateId[s];
    const d = forward[s];
    if (plate < 0 || d < 0) continue;
    const held = src.plateId[d];
    if (held < 0 || held === plate) continue;

    const nbrs = mesh.neighbors(s);
    const arcs = mesh.neighbourArcs(s);
    let arc = -1;
    for (let k = 0; k < nbrs.length; k++) {
      if (nbrs[k] === d) {
        arc = arcs[k];
        break;
      }
    }
    if (arc < 0 || boundaries.arcKind[arc] !== BoundaryKind.CONVERGENT)
      continue;

    fromCell.push(s);
    ontoCell.push(d);
    const key = plate * 65536 + held;
    let tally = margins.get(key);
    if (!tally) {
      tally = { mine: 0, theirs: 0 };
      margins.set(key, tally);
    }
    tally.mine += buoyancy(s);
    tally.theirs += buoyancy(d);
  }

  for (let i = 0; i < fromCell.length; i++) {
    const s = fromCell[i];
    const d = ontoCell[i];
    const plate = src.plateId[s];
    const tally = margins.get(plate * 65536 + src.plateId[d]);
    // The margin as a whole must be one this plate overrides...
    if (!tally || tally.mine >= tally.theirs) continue;
    // ...and continental crust is never dragged under locally, whatever the
    // margin's average says: an arriving oceanic front stops at it.
    if (src.crustType[d] === CONTINENTAL && src.crustType[s] !== CONTINENTAL) {
      continue;
    }

    // The advancing plate must already hold a stretch of this cell's ring, so a
    // front can only widen an edge it owns and never start a finger.
    const ring = mesh.neighbors(d);
    let foothold = 0;
    for (let j = 0; j < ring.length; j++) {
      if (src.plateId[ring[j]] === plate) foothold++;
    }
    if (foothold < MIN_FOOTHOLD) continue;

    // A continent-continent collision locks once the pile has no room left:
    // neither side can sink and the crust cannot thicken further, so the
    // boundary stops migrating instead of one continent eating the other.
    if (
      src.crustType[d] === CONTINENTAL &&
      src.crustType[s] === CONTINENTAL &&
      src.thickness[s] + src.thickness[d] > thicknessCap
    ) {
      continue;
    }

    const prev = claimant[d];
    if (prev < 0 || overrides(s, prev)) claimant[d] = s;
  }

  for (let d = 0; d < cellCount; d++) {
    const s = claimant[d];
    if (s < 0) continue;
    newPlate[d] = src.plateId[s];
    eaten[d] = 1;
  }

  // --- Pass 1b: a locked front stalls the crust behind it -------------------
  // A plate cannot drive its crust into a margin that will not give way. The
  // rigid whole-cell step would do it anyway, shovelling a fresh column into the
  // boundary row every tick while that row kept everything it already had, so
  // the seam grew without limit -- roots thousands of kilometres deep. Real
  // lithosphere jams instead: the front stops, and the stop propagates back up
  // the column behind it. Those cells simply keep the crust they have, so the
  // plate's interior still moves while its blocked edge does not.
  const stalled = new Uint8Array(cellCount);
  {
    const jam: number[] = [];
    for (let sCell = 0; sCell < cellCount; sCell++) {
      const plate = src.plateId[sCell];
      const ahead = forward[sCell];
      if (plate < 0 || ahead < 0) continue;
      if (src.plateId[ahead] === plate) continue;
      // The front advanced here, so nothing is blocked.
      if (newPlate[ahead] === plate) continue;
      // Crust that loses the buoyancy contest goes down the trench rather than
      // stopping; only the side that cannot sink jams.
      if (!overrides(sCell, ahead)) continue;
      stalled[sCell] = 1;
      jam.push(sCell);
    }
    // Reverse of `forward`, to walk the jam back through the column.
    const behindStart = new Int32Array(cellCount + 1);
    for (let sCell = 0; sCell < cellCount; sCell++) {
      if (forward[sCell] >= 0) behindStart[forward[sCell] + 1]++;
    }
    for (let i = 0; i < cellCount; i++) behindStart[i + 1] += behindStart[i];
    const behind = new Int32Array(
      behindStart[cellCount] > 0 ? behindStart[cellCount] : 1,
    );
    const fill = Int32Array.from(behindStart.subarray(0, cellCount));
    for (let sCell = 0; sCell < cellCount; sCell++) {
      const t = forward[sCell];
      if (t >= 0) behind[fill[t]++] = sCell;
    }
    for (let head = 0; head < jam.length; head++) {
      const t = jam[head];
      for (let k = behindStart[t]; k < behindStart[t + 1]; k++) {
        const b = behind[k];
        if (stalled[b] || src.plateId[b] !== src.plateId[t]) continue;
        stalled[b] = 1;
        jam.push(b);
      }
    }
  }

  // --- Pass 2: transport, as an exact permutation --------------------------
  // Every destination pulls from at most one source and every source feeds at
  // most one destination. Candidates are ranked by how squarely the source sits
  // behind the destination, with the cell whose own forward step lands here
  // preferred outright; ties and contention are resolved by taking the
  // best-aligned pairs first. A destination left without a source has been
  // pulled off its plate's crust: it rifts.
  const sourceOf = new Int32Array(cellCount).fill(-1);
  const claimed = new Uint8Array(cellCount);
  const MAX_NBRS = 6;
  const cand = new Int32Array(cellCount * MAX_NBRS).fill(-1);
  const candScore = new Float32Array(cellCount * MAX_NBRS);
  const candCount = new Int32Array(cellCount);
  const bestScore = new Float32Array(cellCount);
  const queue: number[] = [];

  for (let d = 0; d < cellCount; d++) {
    const plate = newPlate[d];
    if (plate < 0) continue;
    if (!stepping.has(plate)) {
      // A stationary plate keeps every cell it already held, unchanged. It can
      // have claimed nothing, so `eaten` is never set on its cells.
      if (src.plateId[d] === plate) {
        sourceOf[d] = d;
        claimed[d] = 1;
      }
      continue;
    }
    if (stalled[d] && src.plateId[d] === plate) {
      // Jammed against a locked margin: this cell keeps the crust it has.
      sourceOf[d] = d;
      claimed[d] = 1;
      continue;
    }
    const props = plateData.byId.get(plate);
    if (!props) continue;
    const vel = cross(props.omega, mesh.position(d));
    const speed = length(vel);
    const back: Vec3 =
      speed === 0
        ? [0, 0, 0]
        : [-vel[0] / speed, -vel[1] / speed, -vel[2] / speed];
    const nbrs = mesh.neighbors(d);
    let n = 0;
    for (let k = 0; k < nbrs.length; k++) {
      const s = nbrs[k];
      if (src.plateId[s] !== plate) continue;
      // +2 lifts the cell whose own step lands exactly here above every other
      // candidate, keeping transport consistent with the claims in pass 1.
      const score =
        dot(chordDir(mesh, d, s), back) + (forward[s] === d ? 2 : 0);
      let i = n;
      while (i > 0 && candScore[d * MAX_NBRS + i - 1] < score) {
        cand[d * MAX_NBRS + i] = cand[d * MAX_NBRS + i - 1];
        candScore[d * MAX_NBRS + i] = candScore[d * MAX_NBRS + i - 1];
        i--;
      }
      cand[d * MAX_NBRS + i] = s;
      candScore[d * MAX_NBRS + i] = score;
      n++;
    }
    candCount[d] = n;
    if (n === 0) continue;
    bestScore[d] = candScore[d * MAX_NBRS];
    queue.push(d);
  }

  queue.sort((a, b) => bestScore[b] - bestScore[a]);
  for (let q = 0; q < queue.length; q++) {
    const d = queue[q];
    const base = d * MAX_NBRS;
    for (let i = 0; i < candCount[d]; i++) {
      const s = cand[base + i];
      if (claimed[s]) continue;
      claimed[s] = 1;
      sourceOf[d] = s;
      break;
    }
  }

  // --- Pass 2b: a neighbour follows into a vacated cell --------------------
  // A cell its own plate has pulled off is only a rift if nothing moves in
  // behind. Where an adjacent plate's step lands exactly here, that plate takes
  // the cell and brings its own crust with it. This is how a plate being
  // consumed actually shrinks: without it every trailing edge mints new ocean,
  // the footprint never changes, and an ocean basin can never close.
  for (let d = 0; d < cellCount; d++) {
    if (sourceOf[d] >= 0 || newPlate[d] < 0) continue;
    const nbrs = mesh.neighbors(d);
    let taker = -1;
    for (let k = 0; k < nbrs.length; k++) {
      const s = nbrs[k];
      if (claimed[s] || forward[s] !== d) continue;
      const plate = src.plateId[s];
      if (plate < 0 || plate === newPlate[d]) continue;
      // Two neighbours of the cell's ring is enough to follow into a gap: the
      // cell is being vacated, so nothing has to be overridden to take it.
      let foothold = 0;
      for (let j = 0; j < nbrs.length; j++) {
        if (src.plateId[nbrs[j]] === plate) foothold++;
      }
      if (foothold < 2) continue;
      if (taker < 0 || overrides(s, taker)) taker = s;
    }
    if (taker < 0) continue;
    claimed[taker] = 1;
    sourceOf[d] = taker;
    newPlate[d] = src.plateId[taker];
  }

  /**
   * Leave `d` as an empty rift placeholder: the crust lifecycle (07) mints the
   * fresh young ocean that fills it and books the created mass.
   */
  const openRift = (d: number, plate: number): void => {
    dst.elevation[d] = 0;
    dst.plateId[d] = plate;
    dst.crustType[d] = OCEANIC;
    dst.age[d] = 0;
    dst.density[d] = tuning.riftDensity;
    dst.thickness[d] = 0;
    riftCell[d] = 1;
  };

  for (let d = 0; d < cellCount; d++) {
    const plate = newPlate[d];
    if (plate < 0) {
      copyCell(mesh, src, dst, d, d);
      continue;
    }
    const s = sourceOf[d];
    if (s < 0) {
      // Nothing arrived. That is only a gap if this cell's own crust left with
      // someone: a cell nobody drew from simply keeps what it has. Rifting a
      // cell whose crust never moved would replace it with fresh ocean, which
      // for a continent means quietly converting it to sea floor -- the one
      // mechanism that legitimately shrinks continental crust, fired where no
      // continent actually broke apart.
      if (claimed[d]) {
        openRift(d, plate);
      } else {
        copyCell(mesh, src, dst, d, d);
        dst.plateId[d] = plate;
        claimed[d] = 1;
        winner[d] = d;
      }
      continue;
    }
    copyCell(mesh, src, dst, s, d);
    dst.plateId[d] = plate;
    winner[d] = s;
  }

  // --- Pass 3: dispose of the crust that had nowhere to go -----------------
  // A source no destination took is crust that ran out of room, and which way it
  // goes is decided by buoyancy, exactly as at a real margin:
  //
  // - the cell was annexed, so its old crust is now beneath the arriving plate:
  //   it subducts, or welds on if both sides are continental;
  // - the cell is on a leading edge that another plate overrides: it goes down
  //   the trench;
  // - the cell is on a leading edge that overrides what is in front of it but
  //   could not advance onto it: it cannot sink, so it piles up where it stands.
  //   That is the forearc thickening that turns an active margin into new
  //   continental crust, and it is why nothing here is ever simply discarded.
  const subduct = (loser: number, into: number): void => {
    const loserMass = mesh.area(loser) * src.thickness[loser];
    const accretedMass = tuning.accretionFraction * loserMass;
    subductedThisStep += loserMass - accretedMass;
    accreted[into] += accretedMass / mesh.area(into);
    subducted[into] = 1;
  };

  const weld = (loser: number, into: number): void => {
    welded[into] += (mesh.area(loser) * src.thickness[loser]) / mesh.area(into);
  };

  /**
   * Consume a continental cell. Continental crust is too buoyant to be drawn
   * into the mantle, so nearly all of it is transferred to the cell that took
   * it: horizontal shortening traded for thickness. Only the small
   * {@link Tuning.subductionErosion} fraction goes down, which is the pair of
   * processes that really do remove continental material at a trench --
   * subduction erosion of the overriding margin, and continentally-derived
   * sediment riding the slab past the wedge.
   */
  const consumeContinental = (loser: number, into: number): void => {
    const mass = mesh.area(loser) * src.thickness[loser];
    const eroded = tuning.subductionErosion * mass;
    subductedThisStep += eroded;
    welded[into] += (mass - eroded) / mesh.area(into);
    subducted[into] = 1;
    // A docked terrane is continental crust wherever it ends up. Leaving the
    // cell labelled with whatever the arriving plate happened to be carrying
    // meant a scraped-off fragment read as ocean floor until its thickness
    // crossed the differentiation threshold -- and could be recycled before it
    // ever did.
    if (!riftCell[into]) {
      dst.crustType[into] = CONTINENTAL;
      dst.density[into] = tuning.continentalDensity;
    }
  };

  for (let s = 0; s < cellCount; s++) {
    if (claimed[s] || src.plateId[s] < 0) continue;
    const ahead = forward[s];
    const blockedByOther = ahead >= 0 && src.plateId[ahead] !== src.plateId[s];

    // What happens is decided by the crust that has run out of room, not by
    // whatever happens to be on top of it. Continental crust cannot sink, so it
    // crumples; oceanic crust at a margin it cannot advance across sinks, and
    // does not stand there thickening until it turns into a continent.
    if (src.crustType[s] === CONTINENTAL) {
      if (eaten[s]) {
        // Scraped off by the plate that took the cell: nearly all of it is
        // transferred, the rest is the trench's erosion of the margin.
        consumeContinental(s, s);
      } else {
        // Simply compressed where it stands -- crustal shortening.
        weld(s, s);
      }
    } else if (eaten[s]) {
      subduct(s, s);
    } else if (blockedByOther) {
      subduct(s, ahead);
    } else {
      weld(s, s);
    }
  }

  // --- Pass 4: apply this step's gains -------------------------------------
  // Everything scraped off a slab or folded off a colliding continent has to
  // end up somewhere. A cell absorbs what it can -- as fast as crust can
  // actually be underplated ({@link Tuning.maxThickeningPerMyr}) and no higher
  // than the thickness ceiling -- and hands the rest to its neighbours, over
  // several rounds, until it has all been placed. That is what an accretionary
  // wedge and an orogen do: they widen. Destroying the overflow instead is what
  // made a continental collision *lose* crust, which is the one thing a
  // collision never does.
  const maxGain = tuning.maxThickeningPerMyr * dtMyr;
  let surplus = new Float32Array(cellCount);
  for (let d = 0; d < cellCount; d++) {
    const gain = accreted[d] + welded[d];
    if (gain === 0) continue;
    const absorbable = Math.min(accreted[d], maxGain) + welded[d];
    // A rift cell is an open gap the lifecycle is about to fill with fresh
    // ocean, overwriting whatever is in it. Crust must not be parked there or
    // it is wiped out and, worse, counted as newly created; it goes to the
    // neighbours instead.
    const held = riftCell[d]
      ? 0
      : Math.min(absorbable, Math.max(0, thicknessCap - dst.thickness[d]));
    dst.thickness[d] += held;
    surplus[d] = gain - held;
  }

  // Push what would not fit out into the surrounding crust, in proportion to
  // the room each neighbour has left, until it is all placed or the whole
  // neighbourhood is at the ceiling.
  const PLACEMENT_ROUNDS = 6;
  for (let round = 0; round < PLACEMENT_ROUNDS; round++) {
    let pending = false;
    for (let d = 0; d < cellCount; d++) {
      if (surplus[d] > 0) {
        pending = true;
        break;
      }
    }
    if (!pending) break;
    const headroom = new Float32Array(cellCount);
    for (let d = 0; d < cellCount; d++) {
      headroom[d] = riftCell[d]
        ? 0
        : Math.max(0, thicknessCap - dst.thickness[d]);
    }
    const incoming = new Float32Array(cellCount);
    const next = new Float32Array(cellCount);
    for (let d = 0; d < cellCount; d++) {
      const have = surplus[d];
      if (have <= 0) continue;
      const nbrs = mesh.neighbors(d);
      let room = 0;
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (dst.plateId[n] === dst.plateId[d] && !riftCell[n]) {
          room += headroom[n];
        }
      }
      // When every neighbour is already at the ceiling the crust still has to
      // go somewhere -- it flows toward the thinnest of them rather than
      // stacking where it stands. Handing it back to its own cell instead let a
      // stalled front pile up without limit, tick after tick, into roots
      // thousands of kilometres deep.
      const crowded = room <= 0;
      let weight = 0;
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (dst.plateId[n] !== dst.plateId[d] || riftCell[n]) continue;
        weight += crowded ? 1 / (1 + dst.thickness[n]) : headroom[n];
      }
      if (weight <= 0) {
        next[d] += have;
        continue;
      }
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (dst.plateId[n] !== dst.plateId[d] || riftCell[n]) continue;
        const w = crowded ? 1 / (1 + dst.thickness[n]) : headroom[n];
        if (w <= 0) continue;
        incoming[n] += (have * (w / weight) * mesh.area(d)) / mesh.area(n);
      }
    }
    const last = round === PLACEMENT_ROUNDS - 1;
    for (let d = 0; d < cellCount; d++) {
      if (incoming[d] === 0) continue;
      // Early rounds fill the room available and pass the rest further out, so a
      // collision spreads over a wide area; the last round takes whatever is
      // left, because by then there is nowhere further to send it and a root is
      // the honest outcome -- one the delamination pass then bleeds off.
      const place = last ? incoming[d] : Math.min(incoming[d], headroom[d]);
      dst.thickness[d] += place;
      next[d] += incoming[d] - place;
    }
    surplus = next;
  }
  // Anything still homeless piles up where it stands, past the nominal ceiling.
  // Crust is not annihilated for want of room: it makes a root, and the root
  // comes off slowly further down (see the delamination pass). Ledgering the
  // remainder here instead destroyed more continental crust than every other
  // mechanism in the model put together, because a collision whose whole
  // neighbourhood was already at the ceiling had nowhere to put anything.
  for (let d = 0; d < cellCount; d++) {
    if (surplus[d] > 0) dst.thickness[d] += surplus[d];
  }

  // --- Pass 4a: thickened crust spreads under its own weight ---------------
  // Continental crust has no strength to hold a deep root: above
  // {@link Tuning.crustSpreadStart} an orogen flows outwards, thinning at the
  // crest and thickening its surroundings. This is the process that turns a
  // collision or a magmatic arc into a wide continent -- without it the crust
  // piles into a one-cell wall, hits the absolute ceiling, and the excess is
  // destroyed instead of becoming land. Mass moves, it is never created: the
  // outflow from a cell equals the sum of what its neighbours receive.
  const before = Float32Array.from(dst.thickness);
  const flow = new Float32Array(cellCount);
  const spreadRate = Math.min(0.5, Math.max(0, tuning.crustSpreadRate * dtMyr));
  if (spreadRate > 0) {
    for (let d = 0; d < cellCount; d++) {
      const excess = before[d] - tuning.crustSpreadStart;
      if (excess <= 0 || riftCell[d]) continue;
      const nbrs = mesh.neighbors(d);
      let deficit = 0;
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (dst.plateId[n] !== dst.plateId[d] || riftCell[n]) continue;
        if (before[n] < before[d]) deficit += before[d] - before[n];
      }
      if (deficit <= 0) continue;
      const give = Math.min(excess, deficit) * spreadRate;
      flow[d] -= give;
      for (let k = 0; k < nbrs.length; k++) {
        const n = nbrs[k];
        if (dst.plateId[n] !== dst.plateId[d] || riftCell[n]) continue;
        if (before[n] >= before[d]) continue;
        const share = (before[d] - before[n]) / deficit;
        flow[n] += (give * share * mesh.area(d)) / mesh.area(n);
      }
    }
    for (let d = 0; d < cellCount; d++) dst.thickness[d] += flow[d];
  }

  // --- Pass 4d: an over-thick root founders, slowly -----------------------
  // A crustal root deep enough to turn to eclogite is denser than the mantle
  // around it and peels away. It is a real sink on continental crust, and the
  // only one that acts on a collision -- but it works over tens of millions of
  // years, so it is bled off at a rate rather than clipped off in a single step.
  // Spreading (above) therefore gets many ticks to carry the crust outward
  // before any of it is lost, which is what turns a collision into a plateau.
  const shedPerStep = Math.max(0, tuning.delaminationRate * dtMyr);
  if (shedPerStep > 0) {
    for (let d = 0; d < cellCount; d++) {
      const over = dst.thickness[d] - thicknessCap;
      if (over <= 0) continue;
      const shed = Math.min(over, shedPerStep);
      dst.thickness[d] -= shed;
      subductedThisStep += shed * mesh.area(d);
    }
  }

  // --- Pass 4c: a continent rifts by stretching, not by melting ------------
  // A gap that opens inside continental crust is not a mid-ocean ridge. The
  // continent necks down into it: the crust that fills the gap is drawn out of
  // its neighbours, and no new crust is made. Minting fresh sea floor at every
  // trailing edge instead was a crust pump -- a continent whose leading edge
  // could not advance still conjured a row of new ocean behind itself each time
  // it stepped, and since continental crust is never subducted, the planet just
  // accumulated crust until it was all continent. Sea floor is now minted only
  // where there is nothing left to stretch, which is what a mid-ocean ridge
  // actually is. Stretched thin enough, the fill drops below a continental
  // column and reads as ocean floor from then on -- continental crust really
  // does turn into oceanic crust when a continent rifts apart.
  for (let d = 0; d < cellCount; d++) {
    if (!riftCell[d]) continue;
    const plate = dst.plateId[d];
    const nbrs = mesh.neighbors(d);
    let available = 0;
    for (let k = 0; k < nbrs.length; k++) {
      const n = nbrs[k];
      if (dst.plateId[n] !== plate || dst.crustType[n] !== CONTINENTAL)
        continue;
      available +=
        Math.max(0, dst.thickness[n] - tuning.riftThicknessFloor) *
        mesh.area(n);
    }
    if (available <= 0) continue;
    // Draw at most a normal crustal column out of the neighbourhood; a gap can
    // never pull in more crust than would fill it.
    const wanted = Math.min(available, tuning.oceanicThickness * mesh.area(d));
    let pulled = 0;
    for (let k = 0; k < nbrs.length; k++) {
      const n = nbrs[k];
      if (dst.plateId[n] !== plate || dst.crustType[n] !== CONTINENTAL)
        continue;
      const spare =
        Math.max(0, dst.thickness[n] - tuning.riftThicknessFloor) *
        mesh.area(n);
      if (spare <= 0) continue;
      const take = (wanted * spare) / available;
      dst.thickness[n] -= take / mesh.area(n);
      pulled += take;
    }
    riftCell[d] = 0;
    dst.thickness[d] = pulled / mesh.area(d);
    dst.age[d] = 0;
    if (dst.thickness[d] >= tuning.continentalThicknessThreshold) {
      dst.crustType[d] = CONTINENTAL;
      dst.density[d] = tuning.continentalDensity;
    } else {
      dst.crustType[d] = OCEANIC;
      dst.density[d] = tuning.oceanicDensityYoung;
    }
  }

  // --- Pass 4b: the front has flexural strength ----------------------------
  // Lithosphere is a stiff sheet: a margin is a smooth arc, never a fringe of
  // fingers. A plate boundary is therefore smoothed the way a real one is held
  // smooth -- by curvature. Any cell with four or more of its ring in one other
  // plate is a spur too narrow to hold, and is handed over; a cell on an
  // ordinary straight margin has only two or three foreign neighbours and is
  // untouched. Two sweeps dissolve anything up to two cells wide, which bounds
  // how far a lobe can push into a neighbour before it is planed off. Only the
  // label moves -- the crust stays where it is and changes hands, which is what
  // accreting a terrane amounts to.
  for (let sweep = 0; sweep < 2; sweep++) {
    const labels = new Int32Array(dst.plateId);
    for (let d = 0; d < cellCount; d++) {
      const own = labels[d];
      if (own < 0) continue;
      const nbrs = mesh.neighbors(d);
      let bestPlate = -1;
      let bestCount = 0;
      for (let k = 0; k < nbrs.length; k++) {
        const p = labels[nbrs[k]];
        if (p < 0 || p === own) continue;
        let count = 0;
        for (let j = 0; j < nbrs.length; j++)
          if (labels[nbrs[j]] === p) count++;
        if (count > bestCount) {
          bestCount = count;
          bestPlate = p;
        }
      }
      if (bestCount >= 4) dst.plateId[d] = bestPlate;
    }
  }

  // Contiguity is deliberately NOT enforced here. Consuming a narrow neck can
  // pinch a plate in two, and two disconnected pieces are two plates: nothing
  // holds them to a common motion any more. Forcing them back together was what
  // made plate fragmentation -- the Farallon's fate -- impossible. The split is
  // resolved by `updatePlateIdentity`, which promotes a genuine fragment to a
  // plate of its own and absorbs anything too small to be one.

  out.subductedThisStep = subductedThisStep;
  out.subductedTotal += subductedThisStep;
  out.liveMass = totalCrustMass(mesh, dst);
};
