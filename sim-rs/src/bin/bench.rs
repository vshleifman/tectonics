//! Native benchmark for profiling outside the browser:
//! `cargo run --release --bin bench -- <level> <ticks> [planes] [name=value ...]`,
//! where each `name=value` overrides a tuning field (snake_case).
//!
//! Plates come from great-circle cracks rather than the TypeScript crack walk,
//! so the numbers are for profiling the passes, not for parity with the
//! browser benchmark.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use tectonics_sim::cell_data::CONTINENTAL;
use tectonics_sim::shape::Moments;
use tectonics_sim::world::{PASS_COUNT, Sim};

/// Aspect ratio above which a plate or continent counts as a strip.
const STRIP_ASPECT: f64 = 3.0;
/// Share of the globe below which a continent is too small to have a shape.
const MIN_CONTINENT_AREA: f64 = 0.002;

/// Area-weighted mean aspect of `groups`, and the share of their area in
/// strips.
fn shape_summary(groups: &[Moments], areas: &[f64]) -> (f64, f64) {
    let total: f64 = areas.iter().sum();
    let (mut mean, mut strips) = (0.0, 0.0);
    for (m, &a) in groups.iter().zip(areas) {
        let aspect = m.aspect().min(20.0);
        mean += aspect * a;
        if aspect > STRIP_ASPECT {
            strips += a;
        }
    }
    let total = total.max(1e-12);
    (mean / total, strips / total)
}

/// Shape of every plate, and of every connected continent of some size on
/// each plate.
fn shapes(sim: &Sim) -> ((f64, f64), (f64, f64)) {
    let mesh = &sim.mesh;
    let data = sim.front();
    let n = mesh.cell_count;
    let globe: f64 = (0..n).map(|i| mesh.area(i)).sum();

    let mut plates: HashMap<i32, (Moments, f64)> = HashMap::new();
    for i in 0..n {
        let e = plates.entry(data.plate_id[i]).or_default();
        e.0.add(mesh, i);
        e.1 += mesh.area(i);
    }
    let (pm, pa): (Vec<Moments>, Vec<f64>) = plates.into_values().unzip();

    let mut seen = vec![false; n];
    let (mut cm, mut ca) = (Vec::new(), Vec::new());
    for start in 0..n {
        if seen[start] || data.crust_type[start] != CONTINENTAL {
            continue;
        }
        let plate = data.plate_id[start];
        let mut m = Moments::default();
        let mut area = 0.0;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(cell) = stack.pop() {
            m.add(mesh, cell);
            area += mesh.area(cell);
            for k in mesh.nbrs(cell) {
                let nb = mesh.nbr[k] as usize;
                if !seen[nb] && data.crust_type[nb] == CONTINENTAL && data.plate_id[nb] == plate {
                    seen[nb] = true;
                    stack.push(nb);
                }
            }
        }
        if area >= MIN_CONTINENT_AREA * globe {
            cm.push(m);
            ca.push(area);
        }
    }
    (shape_summary(&pm, &pa), shape_summary(&cm, &ca))
}

/// Ticks back a plate's motion is compared against when counting reversals.
const REVERSAL_LAG: usize = 5;
/// Turn (degrees) over the lag that counts as a reversal.
const REVERSAL_ANGLE: f64 = 120.0;
/// Angular speed (rad/Myr) below which a plate's direction is not meaningful.
const MOVING_SPEED: f64 = 1e-3;
/// Share of the globe below which a plate counts as small when tallying
/// reversals.
const SMALL_PLATE_SHARE: f64 = 0.01;
/// Ticks over which a cluster of rifts counts as one burst.
const RIFT_BURST_TICKS: usize = 5;
/// Ticks back a plate's heading is compared against: 10 and 50 Myr at 2 Myr
/// a tick.
const HEADING_LAGS: [usize; 2] = [5, 25];
/// Share of the globe a plate must cover for its heading to be tracked.
const HEADING_MIN_SHARE: f64 = 0.01;
/// Turn (degrees) that counts as a change of direction.
const HEADING_SWING: f64 = 45.0;
/// Time (Myr) before which headings are not tallied, while the seeded world
/// finds its feet.
const HEADING_FROM_MYR: f64 = 200.0;
/// Time (Myr) after which the continents have grown and their arrangement is
/// worth summarising.
const GROWN_MYR: f64 = 2000.0;
/// Ticks between samples of the landmasses.
const LAND_SAMPLE_TICKS: usize = 10;
/// Share of the continental crust in one landmass that counts as a
/// supercontinent.
const SUPERCONTINENT_SHARE: f64 = 0.75;

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

/// Where continental area comes from and where continental mass goes, at the
/// current tick. Gains are shares of the globe per Myr; losses are shares of
/// the continental mass per Myr.
fn print_continental_budget(sim: &Sim) {
    let m = sim.metrics();
    println!(
        "  {:>6.0} Myr  plates {:>3}  cont area {:5.1}% mass {:5.1}%  mean {:5.1} km  thin {:5.2}%  \
         gain/Myr accretion {:.4}% spreading {:.4}% infill {:.4}% net growth {:.4}% recycled {:.4}%  loss/Myr erosion {:.4}% delamination {:.4}% founder {:.4}%  revert/Myr {:.4}%  \
         holes/Myr {:.4}% unfilled {:.4}%  pockets {} ({:.2}%)",
        m.elapsed_myr,
        m.plate_count,
        m.continental_area * 100.0,
        m.continental_mass_share * 100.0,
        m.mean_continental_thickness,
        m.thin_continental_area * 100.0,
        m.continentalised_by_accretion * 100.0,
        m.continentalised_by_spreading * 100.0,
        m.continentalised_by_infill * 100.0,
        m.continental_growth * 100.0,
        m.continental_recycled * 100.0,
        m.continental_eroded * 100.0,
        m.delaminated * 100.0,
        m.foundered * 100.0,
        m.decontinentalised * 100.0,
        m.interior_holes * 100.0,
        m.interior_holes_unfilled * 100.0,
        m.ocean_pocket_count,
        m.ocean_pocket_area * 100.0,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let level: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let ticks: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(50);
    let planes: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);

    let start = Instant::now();
    let mut sim = Sim::new(level);
    // Tuning overrides as `name=value` pairs after the positional arguments.
    for arg in args.iter().skip(4) {
        let Some((name, value)) = arg.split_once('=') else {
            continue;
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        let t = &mut sim.tuning;
        match name {
            "slab_weight" => t.slab_weight = value,
            "slab_full_length" => t.slab_full_length = value,
            "slab_decay_time" => t.slab_decay_time = value,
            "slab_initiation_time" => t.slab_initiation_time = value,
            "motion_response_time" => t.motion_response_time = value,
            "ridge_weight" => t.ridge_weight = value,
            "max_rift_aspect" => t.max_rift_aspect = value,
            "rift_strength" => t.rift_strength = value,
            "dome_push" => t.dome_push = value,
            "rift_heat_release" => t.rift_heat_release = value,
            "insulation_drive" => t.insulation_drive = value,
            "insulation_rate" => t.insulation_rate = value,
            "insulation_reach" => t.insulation_reach = value,
            "min_rift_area" => t.min_rift_area = value,
            "oceanic_strength_young" => t.oceanic_strength_young = value,
            "oceanic_strength_old" => t.oceanic_strength_old = value,
            "rift_duration" => t.rift_duration = value,
            "rift_stretch" => t.rift_stretch = value,
            "weakness_decay_time" => t.weakness_decay_time = value,
            "suture_weakness" => t.suture_weakness = value,
            "weakness_relief" => t.weakness_relief = value,
            "rift_margin_weakness" => t.rift_margin_weakness = value,
            "suture_merge_time" => t.suture_merge_time = value,
            "mantle_flow_coupling" => t.mantle_flow_coupling = value,
            "downwelling_memory" => t.downwelling_memory = value,
            "slab_initiation_age" => t.slab_initiation_age = value,
            "trench_suction" => t.trench_suction = value,
            "dome_push_duration" => t.dome_push_duration = value,
            "max_turn_rate" => t.max_turn_rate = value,
            "craton_min_age" => t.craton_min_age = value,
            "craton_inland" => t.craton_inland = value,
            "craton_growth_time" => t.craton_growth_time = value,
            "craton_decay_time" => t.craton_decay_time = value,
            "craton_strength" => t.craton_strength = value,
            "craton_keel_gain" => t.craton_keel_gain = value,
            "accretion_fraction" => t.accretion_fraction = value,
            "subduction_erosion" => t.subduction_erosion = value,
            "crust_spread_start" => t.crust_spread_start = value,
            "crust_spread_rate" => t.crust_spread_rate = value,
            "continental_thickness_threshold" => t.continental_thickness_threshold = value,
            "continental_revert_thickness" => t.continental_revert_thickness = value,
            "margin_erosion" => t.margin_erosion = value,
            "basin_infill_rate" => t.basin_infill_rate = value,
            "basin_infill_max_area" => t.basin_infill_max_area = value,
            "continental_area_target" => t.continental_area_target = value,
            "continental_recycling_rate" => t.continental_recycling_rate = value,
            "max_thickening_per_myr" => t.max_thickening_per_myr = value,
            _ => eprintln!("unknown tuning override {name}"),
        }
    }
    let built = start.elapsed();
    sim.crack_great_circles(planes, "bench");
    let seeded = start.elapsed() - built;

    let mut pass_ms = [0.0f64; PASS_COUNT];
    let mut history: VecDeque<HashMap<i32, [f64; 3]>> = VecDeque::new();
    let (mut reversals, mut speed_sum, mut speed_samples) = (0usize, 0.0f64, 0usize);
    let mut small_reversals = 0usize;
    let mut rifts_at: VecDeque<u64> = VecDeque::from([sim.rift_count()]);
    let mut rift_burst = 0u64;
    let budget_every = (ticks / 10).max(1);
    // Landmass sampling, once the continents have had time to grow.
    let settled = (GROWN_MYR / sim.tuning.dt_myr) as usize;
    let (mut land_samples, mut largest_sum, mut count_sum, mut assembled) =
        (0usize, 0.0, 0.0, 0usize);
    let (mut correlation_sum, mut correlation_samples) = (0.0, 0usize);
    let mut craton_sum = 0.0;
    let mut largest_series: Vec<f64> = Vec::new();
    // Heading (degrees) and share of the globe of every plate, each tick.
    let mut headings: VecDeque<HashMap<i32, (f64, f64)>> = VecDeque::new();
    let mut turn = [(0.0f64, 0.0f64, 0.0f64); HEADING_LAGS.len()];
    let run = Instant::now();
    for tick in 0..ticks {
        sim.tick();
        rifts_at.push_back(sim.rift_count());
        if rifts_at.len() > RIFT_BURST_TICKS + 1 {
            rifts_at.pop_front();
        }
        rift_burst = rift_burst.max(rifts_at[rifts_at.len() - 1] - rifts_at[0]);
        if (tick + 1) % budget_every == 0 {
            print_continental_budget(&sim);
        }
        if tick >= settled && tick % LAND_SAMPLE_TICKS == 0 {
            let m = sim.metrics();
            land_samples += 1;
            largest_sum += m.largest_continent_share;
            count_sum += m.continent_count as f64;
            if m.largest_continent_share >= SUPERCONTINENT_SHARE {
                assembled += 1;
            }
            largest_series.push(m.largest_continent_share);
            craton_sum += m.craton_share;
            if let Some(r) = m.speed_trench_correlation {
                correlation_sum += r;
                correlation_samples += 1;
            }
        }
        for (total, &ms) in pass_ms.iter_mut().zip(sim.timings()) {
            *total += ms;
        }
        let now: HashMap<i32, [f64; 3]> = sim
            .plate_omegas()
            .chunks(4)
            .map(|c| (c[0] as i32, [c[1], c[2], c[3]]))
            .collect();
        let earlier = (history.len() == REVERSAL_LAG).then(|| &history[0]);
        let summaries = sim.summaries();
        let share: HashMap<i32, f64> = summaries.iter().map(|p| (p.id, p.area_share)).collect();
        let heading_now: HashMap<i32, (f64, f64)> = summaries
            .iter()
            .filter(|p| p.area_share >= HEADING_MIN_SHARE && p.speed >= MOVING_SPEED)
            .filter_map(|p| p.heading.map(|h| (p.id, (h, p.area_share))))
            .collect();
        if tick as f64 * sim.tuning.dt_myr >= HEADING_FROM_MYR {
            for (slot, &lag) in HEADING_LAGS.iter().enumerate() {
                let Some(then) = headings.len().checked_sub(lag).map(|i| &headings[i]) else {
                    continue;
                };
                for (id, &(h, a)) in &heading_now {
                    let Some(&(h0, _)) = then.get(id) else {
                        continue;
                    };
                    let d = (h - h0).rem_euclid(360.0);
                    let d = d.min(360.0 - d);
                    turn[slot].0 += d * a;
                    turn[slot].1 += a;
                    if d > HEADING_SWING {
                        turn[slot].2 += a;
                    }
                }
            }
        }
        headings.push_back(heading_now);
        if headings.len() > HEADING_LAGS[HEADING_LAGS.len() - 1] {
            headings.pop_front();
        }
        for (id, w) in &now {
            let speed = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            speed_sum += speed;
            speed_samples += 1;
            let Some(v) = earlier.and_then(|e| e.get(id)) else {
                continue;
            };
            let before = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if speed < MOVING_SPEED || before < MOVING_SPEED {
                continue;
            }
            let cos = (w[0] * v[0] + w[1] * v[1] + w[2] * v[2]) / (speed * before);
            if cos < REVERSAL_ANGLE.to_radians().cos() {
                reversals += 1;
                if share.get(id).copied().unwrap_or(0.0) < SMALL_PLATE_SHARE {
                    small_reversals += 1;
                }
            }
        }
        history.push_back(now);
        if history.len() > REVERSAL_LAG {
            history.pop_front();
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
    println!(
        "  motion: mean speed {:.4} rad/Myr, reversals (>{REVERSAL_ANGLE:.0} deg in {} Myr) {reversals}, {small_reversals} on plates under {:.0}% of the globe",
        speed_sum / speed_samples.max(1) as f64,
        REVERSAL_LAG as f64 * sim.tuning.dt_myr,
        SMALL_PLATE_SHARE * 100.0,
    );
    println!(
        "  rifts: {} total, most in any {} Myr {rift_burst}, {} failed",
        sim.rift_count(),
        RIFT_BURST_TICKS as f64 * sim.tuning.dt_myr,
        sim.failed_rift_count(),
    );
    if land_samples > 0 {
        let s = land_samples as f64;
        // Supercontinent cycles: how often the largest landmass crosses up
        // through the supercontinent share.
        let assemblies = largest_series
            .windows(2)
            .filter(|w| w[0] < SUPERCONTINENT_SHARE && w[1] >= SUPERCONTINENT_SHARE)
            .count();
        println!(
            "  landmasses after {GROWN_MYR:.0} Myr: {:.1} continents, largest {:.0}% of continental area, \
             one landmass with >{:.0}% {:.0}% of the time, {assemblies} assemblies, \
             speed-trench correlation {:.2}, cratons {:.0}% of continental area",
            count_sum / s,
            largest_sum / s * 100.0,
            SUPERCONTINENT_SHARE * 100.0,
            assembled as f64 / s * 100.0,
            correlation_sum / correlation_samples.max(1) as f64,
            craton_sum / s * 100.0,
        );
    }
    let turns: Vec<String> = HEADING_LAGS
        .iter()
        .zip(&turn)
        .map(|(&lag, &(sum, weight, swung))| {
            let w = weight.max(1e-12);
            format!(
                "over {:.0} Myr {:.1} deg ({:.0}% >{HEADING_SWING:.0} deg)",
                lag as f64 * sim.tuning.dt_myr,
                sum / w,
                swung / w * 100.0
            )
        })
        .collect();
    println!(
        "  heading turn, plates over {:.0}% of the globe: {}",
        HEADING_MIN_SHARE * 100.0,
        turns.join(", ")
    );
    let ((plate_aspect, plate_strips), (cont_aspect, cont_strips)) = shapes(&sim);
    println!(
        "  shape: plate aspect {plate_aspect:.2} ({:.0}% of area in strips), continent aspect {cont_aspect:.2} ({:.0}% in strips)",
        plate_strips * 100.0,
        cont_strips * 100.0,
    );
}
