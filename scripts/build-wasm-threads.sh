#!/bin/sh
# The threaded WebAssembly build: the simulation with `--features parallel`,
# its rayon pool running on web workers over shared memory. Pages that are not
# cross-origin isolated fall back to the single-threaded build in `pkg`.
#
# Wasm threads still need nightly Rust, to rebuild the standard library with
# atomics. Pinned so the build stays reproducible; to set it up:
#   rustup toolchain install nightly-2026-10-01 --component rust-src --target wasm32-unknown-unknown
set -e
TOOLCHAIN=nightly-2026-10-01

# These replace the target's `rustflags` in `sim-rs/.cargo/config.toml`, so
# SIMD is asked for again here. The linker does not make the memory shared
# for atomics on its own: it has to be imported and shared, with a maximum
# (4 GiB, so a fine mesh fits), and the thread-local storage hooks exported
# for wasm-bindgen to start each worker with.
export RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128 \
-C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--max-memory=4294967296 \
-C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size \
-C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"

rustup run "$TOOLCHAIN" wasm-pack build --target web --release \
  --out-dir ../src/sim-wasm/pkg-threads sim-rs \
  --features parallel -Z build-std=panic_abort,std
