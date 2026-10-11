import { makeRng } from "../math/rng";
import { Plates } from "../plates/Plates";
import { PASS_NAMES, type SimWorld } from "../sim/SimWorld";
import { defaultSimOptions, defaultTuning } from "../sim/tuning";

/**
 * A fixed, reproducible scenario: build a world at `level`, drop `seeds` seeds
 * at junctions drawn from a fixed RNG, and run `ticks` ticks. Doubles as the
 * regression check: the same scenario must keep producing metric trajectories
 * inside the tolerance bands (see `scripts/compare.ts`) as the simulation
 * changes.
 */
export interface BenchSpec {
  level: number;
  seeds: number;
  ticks: number;
}

export const DEFAULT_BENCH: BenchSpec = { level: 6, seeds: 12, ticks: 200 };

/** Parse `level:8,seeds:12,ticks:200`; missing keys keep their defaults. */
export const parseBenchSpec = (text: string): BenchSpec => {
  const spec = { ...DEFAULT_BENCH };
  for (const part of text.split(",")) {
    const [key, value] = part.split(":");
    const n = Number(value);
    if (!Number.isFinite(n)) continue;
    if (key === "level" || key === "seeds" || key === "ticks") {
      spec[key] = Math.round(n);
    }
  }
  return spec;
};

/** The metrics recorded every tick: what the parity check compares. */
export interface TrajectoryPoint {
  elapsedMyr: number;
  plateCount: number;
  majorPlateCount: number;
  continentalArea: number;
  continentalMassShare: number;
  meanContinentalThickness: number;
  thinContinentalArea: number;
  continentalisedByAccretion: number;
  continentalisedBySpreading: number;
  continentalisedByInfill: number;
  continentalRecycled: number;
  continentalGrowth: number;
  continentalEroded: number;
  delaminated: number;
  foundered: number;
  decontinentalised: number;
  interiorHoles: number;
  interiorHolesUnfilled: number;
  oceanPocketCount: number;
  oceanPocketArea: number;
  continentCount: number;
  largestContinentShare: number;
  cratonShare: number;
  mantleExcess: number;
  surfaceHeatTw: number;
  radiogenicTw: number;
  coreHeatTw: number;
  mantleTemp: number;
  coreTemp: number;
  mantleCooling: number;
  mantleAnomalyMin: number;
  mantleAnomalyMax: number;
  pileMass: number;
  plumeWeightPeak: number;
  dynTopoLow: number;
  dynTopoHigh: number;
  landArea: number;
  activePlumes: number;
  meanOceanAge: number;
  maxOceanAge: number;
  oceanRecyclingMyr: number | null;
  convergentShare: number;
  divergentShare: number;
  speedTrenchCorrelation: number | null;
  liveMass: number;
  drift: number;
}

export interface BenchResult {
  engine: string;
  spec: BenchSpec;
  cellCount: number;
  /** Time to build the mesh and world. */
  buildMs: number;
  /** Time to drop every seed, crack walks and plate reassignment included. */
  seedMs: number;
  /** Mean ms per tick, over the whole run. */
  msPerTick: number;
  /** Total ms spent in each pass across the run. */
  passMs: Record<string, number>;
  trajectory: TrajectoryPoint[];
}

export const runBench = (
  engine: string,
  createWorld: (level: number) => SimWorld,
  spec: BenchSpec,
): BenchResult => {
  const t0 = performance.now();
  const world = createWorld(spec.level);
  world.setTuning({ ...defaultTuning });
  world.setSimOptions({ ...defaultSimOptions });
  const plates = new Plates(world.mesh);
  world.reassignPlates(plates.cracked, true);
  const t1 = performance.now();

  const { junctionCount } = world.mesh.boundaryGraph();
  const rng = makeRng("bench");
  for (let s = 0; s < spec.seeds; s++) {
    plates.addSeed(Math.floor(rng() * junctionCount));
    world.reassignPlates(plates.cracked, false);
  }
  const t2 = performance.now();

  const passMs: Record<string, number> = {};
  for (const name of PASS_NAMES) passMs[name] = 0;
  const trajectory: TrajectoryPoint[] = [];
  let tickMs = 0;
  for (let t = 0; t < spec.ticks; t++) {
    const start = performance.now();
    world.tick();
    tickMs += performance.now() - start;
    const timings = world.passTimings();
    PASS_NAMES.forEach((name, i) => {
      passMs[name] += timings[i];
    });

    const m = world.metrics();
    const mass = world.mass();
    trajectory.push({
      elapsedMyr: m.elapsedMyr,
      plateCount: m.plateCount,
      majorPlateCount: m.majorPlateCount,
      continentalArea: m.continentalArea,
      continentalMassShare: m.continentalMassShare,
      meanContinentalThickness: m.meanContinentalThickness,
      thinContinentalArea: m.thinContinentalArea,
      continentalisedByAccretion: m.continentalisedByAccretion,
      continentalisedBySpreading: m.continentalisedBySpreading,
      continentalisedByInfill: m.continentalisedByInfill,
      continentalRecycled: m.continentalRecycled,
      continentalGrowth: m.continentalGrowth,
      continentalEroded: m.continentalEroded,
      delaminated: m.delaminated,
      foundered: m.foundered,
      decontinentalised: m.decontinentalised,
      interiorHoles: m.interiorHoles,
      interiorHolesUnfilled: m.interiorHolesUnfilled,
      oceanPocketCount: m.oceanPocketCount,
      oceanPocketArea: m.oceanPocketArea,
      continentCount: m.continentCount,
      largestContinentShare: m.largestContinentShare,
      cratonShare: m.cratonShare,
      mantleExcess: m.mantleExcess,
      surfaceHeatTw: m.surfaceHeatTw,
      radiogenicTw: m.radiogenicTw,
      coreHeatTw: m.coreHeatTw,
      mantleTemp: m.mantleTemp,
      coreTemp: m.coreTemp,
      mantleCooling: m.mantleCooling,
      mantleAnomalyMin: m.mantleAnomalyMin,
      mantleAnomalyMax: m.mantleAnomalyMax,
      pileMass: m.pileMass,
      plumeWeightPeak: m.plumeWeightPeak,
      dynTopoLow: m.dynTopoLow,
      dynTopoHigh: m.dynTopoHigh,
      landArea: m.landArea,
      activePlumes: m.activePlumes,
      meanOceanAge: m.meanOceanAge,
      maxOceanAge: m.maxOceanAge,
      oceanRecyclingMyr: m.oceanRecyclingMyr,
      convergentShare: m.boundaryShare[0],
      divergentShare: m.boundaryShare[1],
      speedTrenchCorrelation: m.speedTrenchCorrelation,
      liveMass: mass.liveMass,
      drift: mass.drift,
    });
  }
  const cellCount = world.mesh.cellCount;
  world.dispose();

  return {
    engine,
    spec,
    cellCount,
    buildMs: t1 - t0,
    seedMs: t2 - t1,
    msPerTick: spec.ticks > 0 ? tickMs / spec.ticks : 0,
    passMs,
    trajectory,
  };
};

const percent = (x: number, digits: number): string =>
  `${(x * 100).toFixed(digits)}%`;

/**
 * Where continental area came from and where continental mass went, averaged
 * over the second half of the run so a single quiet or busy tick cannot skew
 * it.
 */
const describeContinentalBudget = (r: BenchResult): string => {
  const points = r.trajectory.slice(Math.floor(r.trajectory.length / 2));
  if (points.length === 0) return "";
  const mean = (key: keyof TrajectoryPoint): number =>
    points.reduce((sum, p) => sum + Number(p[key] ?? 0), 0) / points.length;
  const last = points[points.length - 1];
  return (
    ` | continents: area ${percent(last.continentalArea, 1)}, mass ${percent(last.continentalMassShare, 1)}, ` +
    `mean ${last.meanContinentalThickness.toFixed(1)} km, thin ${percent(last.thinContinentalArea, 2)}; ` +
    `gain/Myr accretion ${percent(mean("continentalisedByAccretion"), 4)} ` +
    `spreading ${percent(mean("continentalisedBySpreading"), 4)} ` +
    `infill ${percent(mean("continentalisedByInfill"), 4)}, ` +
    `net growth ${percent(mean("continentalGrowth"), 4)}; ` +
    `recycled/Myr ${percent(mean("continentalRecycled"), 4)}; ` +
    `loss/Myr erosion ${percent(mean("continentalEroded"), 4)} ` +
    `delamination ${percent(mean("delaminated"), 4)} ` +
    `founder ${percent(mean("foundered"), 4)}; ` +
    `revert/Myr ${percent(mean("decontinentalised"), 4)}; ` +
    `interior holes/Myr ${percent(mean("interiorHoles"), 4)} ` +
    `unfilled ${percent(mean("interiorHolesUnfilled"), 4)}; ` +
    `pockets ${last.oceanPocketCount} (${percent(last.oceanPocketArea, 2)}); ` +
    `continents ${mean("continentCount").toFixed(1)}, ` +
    `largest ${percent(mean("largestContinentShare"), 0)} of continental area, ` +
    `cratons ${percent(mean("cratonShare"), 0)}`
  );
};

/** One-line human summary of a run, for the console. */
export const describeBench = (r: BenchResult): string => {
  const last = r.trajectory[r.trajectory.length - 1];
  const passes = Object.entries(r.passMs)
    .map(([k, v]) => `${k} ${(v / Math.max(1, r.spec.ticks)).toFixed(2)}`)
    .join(", ");
  return (
    `[${r.engine}] level ${r.spec.level} (${r.cellCount} cells), ` +
    `${r.spec.seeds} seeds, ${r.spec.ticks} ticks: build ${r.buildMs.toFixed(0)} ms, ` +
    `seeds ${r.seedMs.toFixed(0)} ms, ${r.msPerTick.toFixed(2)} ms/tick ` +
    `(${passes})` +
    (last
      ? ` | final: plates ${last.plateCount}, cont ${(last.continentalArea * 100).toFixed(1)}%, ` +
        `ocean age ${last.meanOceanAge.toFixed(1)}, heat ${last.surfaceHeatTw.toFixed(1)} TW, ` +
        `mantle ${last.mantleTemp.toFixed(0)} K (${last.mantleCooling.toFixed(0)} K/Gyr cooling, ` +
        `upper ${last.mantleAnomalyMin.toFixed(0)}..${last.mantleAnomalyMax.toFixed(0)} K, ` +
        `piled ${last.pileMass.toFixed(2)} globes, ${last.activePlumes} plumes), ` +
        `dynamic topography ${last.dynTopoLow.toFixed(2)}..${last.dynTopoHigh.toFixed(2)} km, ` +
        `land ${percent(last.landArea, 1)}, ` +
        `drift ${(last.drift * 100).toFixed(3)}%` +
        describeContinentalBudget(r)
      : "")
  );
};
