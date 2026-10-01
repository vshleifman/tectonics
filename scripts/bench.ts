/**
 * Headless benchmark runner: `npm run bench -- <spec> [out]`, e.g.
 * `npm run bench -- level:8,seeds:12,ticks:200 bench-results/wasm-8.json`.
 * Runs the same scenario as `?bench=` in the browser, against the WASM build in
 * `src/sim-wasm/pkg`, and optionally writes the full result as JSON for
 * `npm run bench:compare`.
 */
import { readFileSync, writeFileSync } from "node:fs";
import {
  describeBench,
  parseBenchSpec,
  runBench,
} from "../src/bench/benchScenario";
import { initSync } from "../src/sim-wasm/pkg/tectonics_sim.js";
import { WasmWorld } from "../src/sim-wasm/WasmWorld";

const [specText = "", out] = process.argv.slice(2);
const spec = parseBenchSpec(specText);

const bytes = readFileSync(
  new URL("../src/sim-wasm/pkg/tectonics_sim_bg.wasm", import.meta.url),
);
const { memory } = initSync({ module: bytes });

const result = runBench("wasm", (level) => new WasmWorld(level, memory), spec);
console.log(describeBench(result));
if (out) writeFileSync(out, JSON.stringify(result, null, 1));
