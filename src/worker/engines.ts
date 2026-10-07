import type { SimWorld } from "../sim/SimWorld";

/** Builds a world of the given subdivision level. */
export type WorldFactory = (level: number) => SimWorld;

/** A loaded build of the simulation. */
export interface Engine {
  create: WorldFactory;
  /** Which build it is and how many threads it runs on, for display. */
  label: string;
}

/** The WASM module is instantiated once per worker and shared by every world. */
let engine: Promise<Engine> | null = null;

/**
 * The threaded build, its rayon pool started on one web worker per core.
 * Needs shared memory, so only loads on a cross-origin isolated page.
 */
const loadThreaded = async (): Promise<Engine> => {
  const [pkg, { WasmWorld }] = await Promise.all([
    import("../sim-wasm/pkg-threads/tectonics_sim.js"),
    import("../sim-wasm/WasmWorld"),
  ]);
  const { memory } = await pkg.default();
  await pkg.initThreadPool(navigator.hardwareConcurrency);
  const threads = pkg.thread_count();
  return {
    create: (level) => new WasmWorld(level, memory, pkg.World),
    label: `WASM, ${threads} threads`,
  };
};

const loadSingle = async (): Promise<Engine> => {
  const [pkg, { WasmWorld }] = await Promise.all([
    import("../sim-wasm/pkg/tectonics_sim.js"),
    import("../sim-wasm/WasmWorld"),
  ]);
  const { memory } = await pkg.default();
  return {
    create: (level) => new WasmWorld(level, memory, pkg.World),
    label: "WASM, 1 thread",
  };
};

/**
 * Load the Rust simulation: the threaded build where the page allows it,
 * otherwise, or if it fails to start, the single-threaded one.
 */
export const loadEngine = (): Promise<Engine> => {
  engine ??= (async () => {
    if (self.crossOriginIsolated) {
      try {
        return await loadThreaded();
      } catch (error) {
        console.warn(
          "Threaded simulation failed to start; using one thread.",
          error,
        );
      }
    }
    return loadSingle();
  })();
  return engine;
};
