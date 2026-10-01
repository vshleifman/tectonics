//! Native benchmark for profiling outside the browser:
//! `cargo run --release --bin bench -- <level> <ticks> [planes]`.
//!
//! Plates come from great-circle cracks rather than the TypeScript crack walk,
//! so the numbers are for profiling the passes, not for parity with the
//! browser benchmark.

use std::time::Instant;
use tectonics_sim::world::{PASS_COUNT, Sim};

const PASS_NAMES: [&str; PASS_COUNT] = [
    "motion",
    "classify",
    "sutures",
    "advect",
    "identity",
    "rifting",
    "lifecycle",
    "elevation",
    "reclassify",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let ticks: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let planes: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);

    let start = Instant::now();
    let mut sim = Sim::new(level);
    let built = start.elapsed();
    sim.crack_great_circles(planes, "bench");
    let seeded = start.elapsed() - built;

    let mut pass_ms = [0.0f64; PASS_COUNT];
    let run = Instant::now();
    for _ in 0..ticks {
        sim.tick();
        for (total, &ms) in pass_ms.iter_mut().zip(sim.timings()) {
            *total += ms;
        }
    }
    let per_tick = run.elapsed().as_secs_f64() * 1000.0 / ticks.max(1) as f64;

    let m = sim.metrics();
    println!(
        "level {level} ({} cells): build {:.0} ms, seed {:.0} ms, {per_tick:.2} ms/tick",
        sim.mesh.cell_count,
        built.as_secs_f64() * 1000.0,
        seeded.as_secs_f64() * 1000.0,
    );
    let passes: Vec<String> = PASS_NAMES
        .iter()
        .zip(pass_ms)
        .map(|(name, ms)| format!("{name} {:.2}", ms / ticks.max(1) as f64))
        .collect();
    println!("  {}", passes.join(", "));
    println!(
        "  final: plates {}, cont {:.1}%, ocean age {:.1}, drift {:.5}%",
        m.plate_count,
        m.continental_area * 100.0,
        m.mean_ocean_age,
        sim.mass_drift() * 100.0
    );
}
