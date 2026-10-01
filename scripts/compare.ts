/**
 * Compare two benchmark trajectories against the parity tolerance bands:
 * `node --import tsx scripts/compare.ts a.json b.json`.
 *
 * The simulation is chaotic, so two implementations are not expected to agree
 * tick for tick; what has to agree is the world they produce. Each metric is
 * averaged over the second half of the run (once the world has settled) and
 * the two means must sit inside that metric's band.
 */
import { readFileSync } from "node:fs";
import type { BenchResult, TrajectoryPoint } from "../src/bench/benchScenario";

interface Band {
  key: keyof TrajectoryPoint;
  /** Allowed absolute difference of the means. */
  abs: number;
  /** Allowed relative difference of the means, whichever is looser. */
  rel: number;
}

const BANDS: Band[] = [
  { key: "plateCount", abs: 4, rel: 0.3 },
  { key: "majorPlateCount", abs: 2, rel: 0.3 },
  { key: "continentalArea", abs: 0.04, rel: 0.2 },
  { key: "meanOceanAge", abs: 10, rel: 0.15 },
  { key: "maxOceanAge", abs: 20, rel: 0.1 },
  { key: "convergentShare", abs: 0.05, rel: 0.2 },
  { key: "divergentShare", abs: 0.05, rel: 0.2 },
  { key: "speedTrenchCorrelation", abs: 0.25, rel: 0.5 },
  { key: "drift", abs: 1e-4, rel: 0 },
];

const load = (path: string): BenchResult =>
  JSON.parse(readFileSync(path, "utf8")) as BenchResult;

const secondHalfMean = (r: BenchResult, key: keyof TrajectoryPoint): number => {
  const points = r.trajectory.slice(Math.floor(r.trajectory.length / 2));
  let sum = 0;
  let n = 0;
  for (const p of points) {
    const v = p[key];
    if (typeof v === "number" && Number.isFinite(v)) {
      sum += v;
      n++;
    }
  }
  return n > 0 ? sum / n : Number.NaN;
};

const [pathA, pathB] = process.argv.slice(2);
if (!pathA || !pathB) {
  console.error("usage: compare.ts a.json b.json");
  process.exit(2);
}
const a = load(pathA);
const b = load(pathB);
console.log(
  `${a.engine} ${a.msPerTick.toFixed(2)} ms/tick vs ${b.engine} ${b.msPerTick.toFixed(2)} ms/tick ` +
    `(x${(a.msPerTick / b.msPerTick).toFixed(2)})`,
);
let failed = 0;
for (const band of BANDS) {
  const ma = secondHalfMean(a, band.key);
  const mb = secondHalfMean(b, band.key);
  const diff = Math.abs(ma - mb);
  const allowed = Math.max(
    band.abs,
    band.rel * Math.max(Math.abs(ma), Math.abs(mb)),
  );
  const ok = Number.isNaN(ma) === Number.isNaN(mb) && !(diff > allowed);
  if (!ok) failed++;
  console.log(
    `${ok ? "ok  " : "FAIL"} ${String(band.key).padEnd(24)} ${ma.toFixed(4).padStart(10)} ${mb.toFixed(4).padStart(10)}  (diff ${diff.toFixed(4)}, allowed ${allowed.toFixed(4)})`,
  );
}
process.exit(failed > 0 ? 1 : 0);
