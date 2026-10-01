import type { SimWorld } from "../sim/SimWorld";

/** Builds a world of the given subdivision level. */
export type WorldFactory = (level: number) => SimWorld;

/** The WASM module is instantiated once per worker and shared by every world. */
let factory: Promise<WorldFactory> | null = null;

/** Load the Rust simulation and resolve a factory for its worlds. */
export const loadWorldFactory = (): Promise<WorldFactory> => {
  factory ??= (async () => {
    const [{ default: init }, { WasmWorld }] = await Promise.all([
      import("../sim-wasm/pkg/tectonics_sim.js"),
      import("../sim-wasm/WasmWorld"),
    ]);
    const { memory } = await init();
    return (level: number) => new WasmWorld(level, memory);
  })();
  return factory;
};
