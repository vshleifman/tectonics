# tectonics

A plate tectonics world-builder in the browser. The UI is TypeScript and
Three.js; the simulation is Rust compiled to WASM, running in a Web Worker.

## Setup

Needs Node and a Rust toolchain with the WASM target and `wasm-pack`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
npm install
```

## Scripts

- `npm run dev` builds the WASM package into `src/sim-wasm/pkg` and starts
  Vite. After changing `sim-rs/`, rerun `npm run build:wasm`.
- `npm run build` does a production build, WASM included.
- `npm run test:rs` runs the native Rust tests (mesh invariants, mass ledger,
  transport permutation).
- `npm run bench -- level:8,seeds:12,ticks:200 out.json` runs the benchmark
  scenario headlessly; `npm run bench:compare -- a.json b.json` checks two runs
  against the metric tolerance bands. In the browser, add
  `?bench=level:8,seeds:12,ticks:200` to the URL and read the console.
- `cargo run --release --manifest-path sim-rs/Cargo.toml --bin bench -- 8 50`
  profiles the passes natively.
