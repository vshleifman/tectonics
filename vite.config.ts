import { defineConfig } from "vite";

/**
 * Cross-origin isolation, without which there is no `SharedArrayBuffer` and
 * so no threaded simulation. Pages served without it, as on a host that does
 * not send these, fall back to the single-threaded build.
 */
const isolation = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig({
  server: {
    port: 5173,
    headers: isolation,
  },
  preview: {
    headers: isolation,
  },
  optimizeDeps: {
    exclude: ["./src/sim-wasm/pkg", "./src/sim-wasm/pkg-threads"],
  },
  worker: {
    format: "es",
  },
});
