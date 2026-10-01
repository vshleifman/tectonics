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
        `ocean age ${last.meanOceanAge.toFixed(1)}, drift ${(last.drift * 100).toFixed(3)}%`
      : "")
  );
};
