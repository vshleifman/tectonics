//! Native benchmark for profiling outside the browser:
//! `cargo run --release --bin bench -- <level> <ticks> [planes] [name=value ...]`,
//! where each `name=value` overrides a tuning field (snake_case).
//!
//! Plates come from great-circle cracks rather than the TypeScript crack walk,
//! so the numbers are for profiling the passes, not for parity with the
//! browser benchmark.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use tectonics_sim::boundaries::DIVERGENT;
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

/// Rings of ocean a landmass may be separated by and still count as joined to
/// its neighbour: a gap of up to twice this many cells is bridged.
const BRIDGE_RINGS: i32 = 1;
/// Age (Myr) of ocean on a passive coast that counts as old enough to founder.
const OLD_PASSIVE_AGE: f64 = 150.0;
/// Ticks between samples of the supercontinent diagnostics: 100 Myr.
const SERIES_TICKS: usize = 50;

/// What the supercontinent hypotheses need to see, at one moment.
#[derive(Default, Clone, Copy)]
struct Assembly {
    /// Share of continental area in the largest landmass, strictly connected.
    strict: f64,
    /// The same with gaps of up to `2 * BRIDGE_RINGS` ocean cells bridged.
    bridged: f64,
    /// Length of the area-weighted mean position of continental crust: 1 all
    /// in one spot, near 0 spread evenly. A cap holding share `f` of the globe
    /// scores `1 - f`.
    clump: f64,
    /// Share of continent-ocean coastline where the ocean is on the same
    /// plate, so no trench can form there.
    passive: f64,
    /// Mean age of the ocean along passive coasts.
    passive_age: f64,
    /// Share of passive coast where the ocean is older than `OLD_PASSIVE_AGE`.
    passive_old: f64,
    /// Length of the sink-weighted mean downwelling position over the total:
    /// 1 for one downwelling, near 0 for sinks spread evenly.
    sink_dipole: f64,
    /// Area-weighted mean cosine between continental crust's motion and the
    /// mantle flow beneath it.
    flow_align: f64,
    /// Speed the mantle flow alone would carry continent at (coupling times
    /// flow), over the speed it actually moves.
    flow_ratio: f64,
    sutures: usize,
    /// Mean |delta omega| across welded pairs, over `merge_quiet_speed`.
    suture_slip: f64,
    plates: usize,
    /// Distance (km) from the most remote ocean to the nearest continent:
    /// Point Nemo's is ~2700 km.
    remote_km: f64,
    /// Share of continental area in landmasses too small to be continents.
    fragments: f64,
    /// Share of continental area within the cap, centred on its mean position,
    /// that would hold all of it: 1 for a perfectly round supercontinent.
    in_cap: f64,
    /// Clump of the largest landmass alone, over that of a round cap of the
    /// same area: 1 if round, lower the more it sprawls. Pangaea, as a block
    /// from 80 S to 80 N and 120 degrees wide, scores ~0.98.
    main_compact: f64,
    /// Farthest the largest landmass reaches from its centre (degrees).
    main_reach: f64,
    /// The same for a round cap of its area.
    main_cap_reach: f64,
}

/// Mean distance (km) between neighbouring cell centres.
fn cell_km(n: usize) -> f64 {
    (4.0 * std::f64::consts::PI / n as f64).sqrt() * EARTH_KM
}

const EARTH_KM: f64 = 6371.0;
/// Share of the globe a landmass must cover to be a continent, as in
/// `metrics.rs`.
const CONTINENT_SHARE: f64 = 0.005;

fn assembly(sim: &Sim) -> Assembly {
    let mesh = &sim.mesh;
    let d = sim.front();
    let n = mesh.cell_count;
    let cont = |i: usize| d.crust_type[i] == CONTINENTAL;
    let c = |i: usize| {
        [
            mesh.centres[i * 3] as f64,
            mesh.centres[i * 3 + 1] as f64,
            mesh.centres[i * 3 + 2] as f64,
        ]
    };
    let mut out = Assembly {
        strict: sim.metrics().largest_continent_share,
        plates: sim.plate_count(),
        ..Default::default()
    };

    // Rings of ocean out from the continents, then landmasses through cells
    // at most BRIDGE_RINGS out.
    let mut ring = vec![-1i32; n];
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| cont(i)).collect();
    for &i in &queue {
        ring[i] = 0;
    }
    while let Some(i) = queue.pop_front() {
        if ring[i] >= BRIDGE_RINGS {
            continue;
        }
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            if ring[m] < 0 {
                ring[m] = ring[i] + 1;
                queue.push_back(m);
            }
        }
    }
    // Every ring out, for the most remote ocean.
    let mut far = vec![-1i32; n];
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| cont(i)).collect();
    for &i in &queue {
        far[i] = 0;
    }
    let mut deepest = 0;
    while let Some(i) = queue.pop_front() {
        deepest = deepest.max(far[i]);
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            if far[m] < 0 {
                far[m] = far[i] + 1;
                queue.push_back(m);
            }
        }
    }
    out.remote_km = deepest as f64 * cell_km(n);

    // Strict landmasses, for the share of crust in fragments.
    let globe: f64 = (0..n).map(|i| mesh.area(i)).sum();
    let mut seen = vec![false; n];
    let mut fragment_area = 0.0;
    let mut main: Vec<usize> = Vec::new();
    let mut main_area = 0.0;
    for start in 0..n {
        if seen[start] || !cont(start) {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut cells = Vec::new();
        let mut area = 0.0;
        while let Some(i) = stack.pop() {
            cells.push(i);
            area += mesh.area(i);
            for k in mesh.nbrs(i) {
                let m = mesh.nbr[k] as usize;
                if !seen[m] && cont(m) {
                    seen[m] = true;
                    stack.push(m);
                }
            }
        }
        if area < CONTINENT_SHARE * globe {
            fragment_area += area;
        }
        if area > main_area {
            main_area = area;
            main = cells;
        }
    }
    // The largest landmass on its own: how compact against a round cap of the
    // same area, and how far it reaches from its centre.
    let mut centre = [0.0f64; 3];
    for &i in &main {
        let p = c(i);
        for k in 0..3 {
            centre[k] += mesh.area(i) * p[k];
        }
    }
    if main_area > 0.0 && len(centre) > 0.0 {
        let cap = 1.0 - main_area / globe;
        out.main_compact = len(centre) / main_area / cap;
        let unit = [centre[0] / len(centre), centre[1] / len(centre), centre[2] / len(centre)];
        out.main_reach = main
            .iter()
            .map(|&i| {
                let p = c(i);
                (p[0] * unit[0] + p[1] * unit[1] + p[2] * unit[2]).clamp(-1.0, 1.0).acos()
            })
            .fold(0.0, f64::max)
            .to_degrees();
        // Reach of a round cap of the same area, for comparison.
        out.main_cap_reach = (1.0 - 2.0 * main_area / globe).clamp(-1.0, 1.0).acos().to_degrees();
    }

    let mut seen = vec![false; n];
    let (mut total, mut largest) = (0.0f64, 0.0f64);
    for start in 0..n {
        if !cont(start) {
            continue;
        }
        total += mesh.area(start);
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut area = 0.0;
        while let Some(i) = stack.pop() {
            if cont(i) {
                area += mesh.area(i);
            }
            for k in mesh.nbrs(i) {
                let m = mesh.nbr[k] as usize;
                if !seen[m] && ring[m] >= 0 {
                    seen[m] = true;
                    stack.push(m);
                }
            }
        }
        largest = largest.max(area);
    }
    out.bridged = if total > 0.0 { largest / total } else { 0.0 };

    let mut sum = [0.0f64; 3];
    let (mut align, mut align_w, mut carried, mut moving) = (0.0, 0.0, 0.0, 0.0);
    let coupling = sim.tuning.mantle_flow_coupling;
    let plates = sim.plates();
    for i in 0..n {
        if !cont(i) {
            continue;
        }
        let a = mesh.area(i);
        let p = c(i);
        for k in 0..3 {
            sum[k] += a * p[k];
        }
        let v = plates.linear_velocity(d.plate_id[i], p);
        let u = sim.mantle().wind_at(i);
        let (sv, su) = (len(v), len(u));
        carried += a * coupling * su;
        moving += a * sv;
        if sv > 1e-9 && su > 1e-12 {
            align += a * (v[0] * u[0] + v[1] * u[1] + v[2] * u[2]) / (sv * su);
            align_w += a;
        }
    }
    out.clump = if total > 0.0 { len(sum) / total } else { 0.0 };
    out.fragments = if total > 0.0 { fragment_area / total } else { 0.0 };
    if total > 0.0 && len(sum) > 0.0 {
        let centre = [sum[0] / len(sum), sum[1] / len(sum), sum[2] / len(sum)];
        // A cap holding share f of the globe reaches cos = 1 - 2f.
        let edge = 1.0 - 2.0 * total / globe;
        let inside: f64 = (0..n)
            .filter(|&i| cont(i))
            .filter(|&i| {
                let p = c(i);
                p[0] * centre[0] + p[1] * centre[1] + p[2] * centre[2] >= edge
            })
            .map(|i| mesh.area(i))
            .sum();
        out.in_cap = inside / total;
    }
    out.flow_align = if align_w > 0.0 { align / align_w } else { 0.0 };
    out.flow_ratio = if moving > 0.0 { carried / moving } else { 0.0 };

    let (mut coast, mut passive, mut age, mut old) = (0usize, 0usize, 0.0, 0usize);
    for i in 0..n {
        if cont(i) {
            continue;
        }
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            if !cont(m) {
                continue;
            }
            coast += 1;
            if d.plate_id[m] == d.plate_id[i] {
                passive += 1;
                age += d.age[i] as f64;
                if d.age[i] as f64 >= OLD_PASSIVE_AGE {
                    old += 1;
                }
            }
        }
    }
    out.passive = passive as f64 / coast.max(1) as f64;
    out.passive_age = age / passive.max(1) as f64;
    out.passive_old = old as f64 / passive.max(1) as f64;

    let (sites, sinks) = sim.mantle().sinks();
    let (mut pole, mut weight) = ([0.0f64; 3], 0.0);
    for (s, &q) in sites.iter().zip(sinks) {
        if q > 0.0 {
            for k in 0..3 {
                pole[k] += q * s[k];
            }
            weight += q;
        }
    }
    out.sink_dipole = if weight > 0.0 { len(pole) / weight } else { 0.0 };

    let mut slip = 0.0;
    for &key in plates.sutures.keys() {
        let (a, b) = tectonics_sim::plates::split_key(key);
        if let (Some(pa), Some(pb)) = (plates.by_id.get(&a), plates.by_id.get(&b)) {
            let w = [
                pa.omega[0] - pb.omega[0],
                pa.omega[1] - pb.omega[1],
                pa.omega[2] - pb.omega[2],
            ];
            slip += len(w);
            out.sutures += 1;
        }
    }
    out.suture_slip = slip / out.sutures.max(1) as f64 / sim.tuning.merge_quiet_speed.max(1e-12);
    out
}

/// Mantle heat under a breakup's continents, after venting, above which it
/// counts as having broken over a dome.
const HOT_BREAKUP: f64 = 0.15;
/// Ticks between measurements of a breakup's gap: 10 Myr.
const GAP_TICKS: usize = 5;
/// Times (Myr) after a breakup at which its gap is summarised.
const GAP_HORIZONS: [f64; 5] = [20.0, 50.0, 100.0, 200.0, 300.0];

/// A rift that split two continents, followed as they part.
struct Breakup {
    a: i32,
    b: i32,
    start: f64,
    /// Mean mantle heat under the two continents just after the breakup
    /// (after venting): high for a breakup over a dome, low for one torn by
    /// slab pull alone.
    heat: f64,
    /// Share of the boundary between the two plates that is diverging,
    /// `GAP_TICKS` after the breakup.
    opening: Option<f64>,
    /// Myr after the breakup that the two plates were first welded together.
    welded: Option<f64>,
    /// Relative motion across the boundary at 10 and 30 Myr after.
    parting: [Option<Parting>; 2],
    /// Gap (km) between the two continents, every `GAP_TICKS`.
    gaps: Vec<(f64, f64)>,
    /// When either plate stopped existing, merged away or absorbed.
    ended: Option<f64>,
}

/// Continental area (share of the globe) on each plate.
fn continental_area_by_plate(sim: &Sim) -> HashMap<i32, f64> {
    let mesh = &sim.mesh;
    let d = sim.front();
    let globe: f64 = (0..mesh.cell_count).map(|i| mesh.area(i)).sum();
    let mut out: HashMap<i32, f64> = HashMap::new();
    for i in 0..mesh.cell_count {
        if d.crust_type[i] == CONTINENTAL {
            *out.entry(d.plate_id[i]).or_insert(0.0) += mesh.area(i) / globe;
        }
    }
    out
}

/// Shortest distance (km) from continent on plate `a` to continent on plate
/// `b`, through anything; `None` if either has none left.
fn gap_km(sim: &Sim, a: i32, b: i32) -> Option<f64> {
    let mesh = &sim.mesh;
    let d = sim.front();
    let n = mesh.cell_count;
    let on = |i: usize, p: i32| d.crust_type[i] == CONTINENTAL && d.plate_id[i] == p;
    let mut hops = vec![-1i32; n];
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| on(i, a)).collect();
    if queue.is_empty() {
        return None;
    }
    for &i in &queue {
        hops[i] = 0;
    }
    while let Some(i) = queue.pop_front() {
        if on(i, b) {
            return Some((hops[i] - 1).max(0) as f64 * cell_km(n));
        }
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            if hops[m] < 0 {
                hops[m] = hops[i] + 1;
                queue.push_back(m);
            }
        }
    }
    None
}

/// How the two plates of a breakup move across their shared boundary, from
/// their motions alone: length-weighted mean opening speed (cm/yr, negative
/// closing) and mean speed along it, and the share of it opening.
#[derive(Clone, Copy)]
struct Parting {
    opening_cm: f64,
    sliding_cm: f64,
    opening_share: f64,
}

fn parting(sim: &Sim, a: i32, b: i32) -> Option<Parting> {
    let mesh = &sim.mesh;
    let d = sim.front();
    let plates = sim.plates();
    let (wa, wb) = (plates.by_id.get(&a)?.omega, plates.by_id.get(&b)?.omega);
    let w = [wa[0] - wb[0], wa[1] - wb[1], wa[2] - wb[2]];
    let cm_per_rad_myr = EARTH_KM * 1e5 / 1e6;
    let (mut total, mut open, mut slide, mut opening) = (0.0, 0.0, 0.0, 0.0);
    for arc in 0..mesh.arc_count() {
        let (ca, cb) = (mesh.arc_cells[arc * 2], mesh.arc_cells[arc * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (pa, pb) = (d.plate_id[ca as usize], d.plate_id[cb as usize]);
        let sign = if pa == a && pb == b {
            1.0
        } else if pa == b && pb == a {
            -1.0
        } else {
            continue;
        };
        let m = [
            mesh.arc_mid[arc * 3] as f64,
            mesh.arc_mid[arc * 3 + 1] as f64,
            mesh.arc_mid[arc * 3 + 2] as f64,
        ];
        let nrm = [
            mesh.arc_normal[arc * 3] as f64,
            mesh.arc_normal[arc * 3 + 1] as f64,
            mesh.arc_normal[arc * 3 + 2] as f64,
        ];
        let r = [
            w[1] * m[2] - w[2] * m[1],
            w[2] * m[0] - w[0] * m[2],
            w[0] * m[1] - w[1] * m[0],
        ];
        // Positive closing, as in `boundaries.rs`, with `a` on the arc's
        // first side.
        let vn = sign * (r[0] * nrm[0] + r[1] * nrm[1] + r[2] * nrm[2]);
        let vt = len([r[0] - vn * sign * nrm[0], r[1] - vn * sign * nrm[1], r[2] - vn * sign * nrm[2]]);
        let l = mesh.arc_length[arc] as f64;
        total += l;
        open -= vn * l;
        slide += vt * l;
        if vn < 0.0 {
            opening += l;
        }
    }
    (total > 0.0).then(|| Parting {
        opening_cm: open / total * cm_per_rad_myr,
        sliding_cm: slide / total * cm_per_rad_myr,
        opening_share: opening / total,
    })
}

/// Share of the boundary between plates `a` and `b` that is diverging, by
/// length; `None` if they no longer touch.
fn opening_share(sim: &Sim, a: i32, b: i32) -> Option<f64> {
    let mesh = &sim.mesh;
    let d = sim.front();
    let kinds = &sim.boundaries().arc_kind;
    let (mut total, mut opening) = (0.0, 0.0);
    for (arc, &kind) in kinds.iter().enumerate() {
        let (ca, cb) = (mesh.arc_cells[arc * 2], mesh.arc_cells[arc * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (pa, pb) = (d.plate_id[ca as usize], d.plate_id[cb as usize]);
        if !((pa == a && pb == b) || (pa == b && pb == a)) {
            continue;
        }
        let len = mesh.arc_length[arc] as f64;
        total += len;
        if kind == DIVERGENT {
            opening += len;
        }
    }
    (total > 0.0).then(|| opening / total)
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// How far apart the continents of each breakup got, and how many were still
/// separate plates, at each of `GAP_HORIZONS`.
fn print_breakups(label: &str, breakups: &[&Breakup], end: f64) {
    let opening: Vec<f64> = breakups.iter().filter_map(|b| b.opening).collect();
    println!(
        "  {label} continental breakups: {}, boundary diverging after 10 Myr: median {:.0}%, none at all in {:.0}%",
        breakups.len(),
        median(opening.clone()) * 100.0,
        opening.iter().filter(|&&o| o == 0.0).count() as f64 / opening.len().max(1) as f64 * 100.0,
    );
    let welded_by = |myr: f64| {
        breakups
            .iter()
            .filter(|b| b.welded.is_some_and(|w| w <= myr))
            .count() as f64
            / breakups.len().max(1) as f64
            * 100.0
    };
    for (slot, at) in [(0usize, 10), (1, 30)] {
        let p: Vec<Parting> = breakups.iter().filter_map(|b| b.parting[slot]).collect();
        println!(
            "    +{at} Myr motion across the rift: opening {:+.2} cm/yr, sliding {:.2} cm/yr, opening along {:.0}% of it, net closing in {:.0}%",
            median(p.iter().map(|x| x.opening_cm).collect()),
            median(p.iter().map(|x| x.sliding_cm).collect()),
            median(p.iter().map(|x| x.opening_share).collect()) * 100.0,
            p.iter().filter(|x| x.opening_cm < 0.0).count() as f64 / p.len().max(1) as f64 * 100.0,
        );
    }
    println!(
        "    welded back together within 10 Myr {:.0}%, 50 Myr {:.0}%, 300 Myr {:.0}%",
        welded_by(10.0),
        welded_by(50.0),
        welded_by(300.0),
    );
    for &h in &GAP_HORIZONS {
        let due: Vec<&&Breakup> = breakups.iter().filter(|b| b.start + h <= end).collect();
        let alive: Vec<f64> = due
            .iter()
            .filter(|b| b.ended.is_none_or(|e| e > b.start + h))
            .filter_map(|b| {
                b.gaps
                    .iter()
                    .rev()
                    .find(|&&(t, _)| t <= b.start + h)
                    .map(|&(_, g)| g)
            })
            .collect();
        let (p25, p75) = {
            let mut v = alive.clone();
            v.sort_by(f64::total_cmp);
            if v.is_empty() {
                (f64::NAN, f64::NAN)
            } else {
                (v[v.len() / 4], v[v.len() * 3 / 4])
            }
        };
        println!(
            "    +{h:>3.0} Myr: {:>3}/{:<3} still apart, gap median {:>5.0} km [{p25:.0}..{p75:.0}]",
            alive.len(),
            due.len(),
            median(alive.clone()),
        );
    }
    let widest: Vec<f64> = breakups
        .iter()
        .map(|b| b.gaps.iter().map(|&(_, g)| g).fold(0.0, f64::max))
        .collect();
    println!("    widest gap ever reached, median {:.0} km", median(widest));
}

/// Where this step's piled-up continent formed: area by rings from the
/// nearest plate boundary (0 on it, `PILE_RINGS` for that far or more), and,
/// for those on a boundary, by its kind (none, convergent, divergent,
/// transform); and the area touching existing continent.
const PILE_RINGS: usize = 4;

#[derive(Default)]
struct Piling {
    by_ring: [f64; PILE_RINGS + 1],
    by_kind: [f64; 4],
    beside_continent: f64,
}

fn tally_piling(sim: &Sim, out: &mut Piling) {
    let mesh = &sim.mesh;
    let d = sim.front();
    let piled = &sim.advection().piled;
    if !piled.iter().any(|&p| p != 0) {
        return;
    }
    let n = mesh.cell_count;
    let mut ring = vec![-1i32; n];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, r) in ring.iter_mut().enumerate() {
        if mesh
            .nbrs(i)
            .any(|k| d.plate_id[mesh.nbr[k] as usize] != d.plate_id[i])
        {
            *r = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        if ring[i] as usize >= PILE_RINGS {
            continue;
        }
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            if ring[m] < 0 {
                ring[m] = ring[i] + 1;
                queue.push_back(m);
            }
        }
    }
    for i in (0..n).filter(|&i| piled[i] != 0) {
        let a = mesh.area(i);
        let r = if ring[i] < 0 { PILE_RINGS } else { ring[i] as usize };
        out.by_ring[r] += a;
        if r == 0 {
            out.by_kind[sim.boundaries().cell_kind[i] as usize] += a;
        }
        if mesh.nbrs(i).any(|k| {
            let m = mesh.nbr[k] as usize;
            d.crust_type[m] == CONTINENTAL && piled[m] == 0
        }) {
            out.beside_continent += a;
        }
    }
}

/// Crust whose age dates it to the first this many Myr counts as ancient.
const ANCIENT_MYR: f64 = 1000.0;
/// Keel above which continental crust counts as craton.
const CRATON_KEEL: f32 = 0.5;
/// Share of the globe a craton must cover to have a shape worth measuring:
/// about three cells at level 5.
const MIN_CRATON_SHARE: f64 = 0.0003;

/// Share of craton area in connected cratons that span more than one plate,
/// so are not moving as one.
fn split_craton_share(sim: &Sim) -> f64 {
    let mesh = &sim.mesh;
    let d = sim.front();
    let n = mesh.cell_count;
    let is = |i: usize| d.crust_type[i] == CONTINENTAL && d.keel[i] >= CRATON_KEEL;
    let mut seen = vec![false; n];
    let (mut total, mut split) = (0.0, 0.0);
    for start in 0..n {
        if seen[start] || !is(start) {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut area = 0.0;
        let mut plates = std::collections::HashSet::new();
        while let Some(i) = stack.pop() {
            area += mesh.area(i);
            plates.insert(d.plate_id[i]);
            for k in mesh.nbrs(i) {
                let j = mesh.nbr[k] as usize;
                if !seen[j] && is(j) {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        total += area;
        if plates.len() > 1 {
            split += area;
        }
    }
    if total > 0.0 { split / total } else { 0.0 }
}

/// Connected cratons: how many, their mean area (share of the globe), and
/// their area-weighted mean aspect.
fn cratons(sim: &Sim) -> (usize, f64, f64) {
    let mesh = &sim.mesh;
    let d = sim.front();
    let n = mesh.cell_count;
    let globe: f64 = (0..n).map(|i| mesh.area(i)).sum();
    let is = |i: usize| d.crust_type[i] == CONTINENTAL && d.keel[i] >= CRATON_KEEL;
    let mut seen = vec![false; n];
    let (mut groups, mut areas) = (Vec::new(), Vec::new());
    for start in 0..n {
        if seen[start] || !is(start) {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut m = Moments::default();
        let mut area = 0.0;
        while let Some(i) = stack.pop() {
            m.add(mesh, i);
            area += mesh.area(i);
            for k in mesh.nbrs(i) {
                let j = mesh.nbr[k] as usize;
                if !seen[j] && is(j) {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if area >= MIN_CRATON_SHARE * globe {
            groups.push(m);
            areas.push(area);
        }
    }
    if groups.is_empty() {
        return (0, 0.0, 0.0);
    }
    let (aspect, _) = shape_summary(&groups, &areas);
    let mean = areas.iter().sum::<f64>() / areas.len() as f64 / globe;
    (groups.len(), mean, aspect)
}

fn len(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
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
         holes/Myr {:.4}% unfilled {:.4}%  pockets {} ({:.2}%)  \
         coast {:.1}% small landmasses {}  torn/Myr {:.4}% rejoined/Myr {:.4}%",
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
        m.coast_share * 100.0,
        m.small_landmass_count,
        m.torn_continent * 100.0,
        m.rejoined * 100.0,
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
            "craton_durability" => t.craton_durability = value,
            "craton_merge_time" => t.craton_merge_time = value,
            "accretion_fraction" => t.accretion_fraction = value,
            "subduction_erosion" => t.subduction_erosion = value,
            "crust_spread_start" => t.crust_spread_start = value,
            "crust_spread_rate" => t.crust_spread_rate = value,
            "continental_thickness_threshold" => t.continental_thickness_threshold = value,
            "continental_revert_thickness" => t.continental_revert_thickness = value,
            "margin_erosion" => t.margin_erosion = value,
            "basin_infill_rate" => t.basin_infill_rate = value,
            "basin_infill_max_area" => t.basin_infill_max_area = value,
            "continental_nucleation_thickness" => t.continental_nucleation_thickness = value,
            "min_continent_area" => t.min_continent_area = value,
            "cohesion_reach" => t.cohesion_reach = value,
            "continental_area_target" => t.continental_area_target = value,
            "continental_recycling_rate" => t.continental_recycling_rate = value,
            "max_thickening_per_myr" => t.max_thickening_per_myr = value,
            "mantle_drag" => t.mantle_drag = value,
            "upwelling_gain" => t.upwelling_gain = value,
            "primordial_heat" => t.primordial_heat = value,
            "primordial_cooling_time" => t.primordial_cooling_time = value,
            "plume_rate" => t.plume_rate = value,
            "plume_radius" => t.plume_radius = value,
            "plume_duration" => t.plume_duration = value,
            "plume_thickness" => t.plume_thickness = value,
            "plume_keel" => t.plume_keel = value,
            "hot_arc_suppression" => t.hot_arc_suppression = value,
            "hot_drip" => t.hot_drip = value,
            "continental_drag" => t.continental_drag = value,
            "collision_drag" => t.collision_drag = value,
            "collision_lock_gain" => t.collision_lock_gain = value,
            "suture_coupling" => t.suture_coupling = value,
            "merge_quiet_speed" => t.merge_quiet_speed = value,
            "boundary_death_time" => t.boundary_death_time = value,
            "insulation_cooling" => t.insulation_cooling = value,
            "insulation_weakening" => t.insulation_weakening = value,
            "passive_margin_collapse_age" => t.passive_margin_collapse_age = value,
            "supercontinent_insulation" => sim.options.supercontinent_insulation = value != 0.0,
            "weld_on_net_closing" => sim.options.weld_on_net_closing = value != 0.0,
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
    let mut series: Vec<Assembly> = Vec::new();
    let quiet = std::env::var("BENCH_QUIET").is_ok();
    let mut breakups: Vec<Breakup> = Vec::new();
    let mut known_breakups: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let follow = GAP_HORIZONS[GAP_HORIZONS.len() - 1];
    let globe: f64 = (0..sim.mesh.cell_count).map(|i| sim.mesh.area(i)).sum();
    // Continental area made so far, by how, as shares of the globe.
    let (mut made_by_plume, mut made_by_arc, mut made_by_piling) = (0.0f64, 0.0f64, 0.0f64);
    let mut piling = Piling::default();
    // Crust mass plumes have made, for the share of it still alive.
    let mut plume_mass_made = 0.0f64;
    // Heading (degrees) and share of the globe of every plate, each tick.
    let mut headings: VecDeque<HashMap<i32, (f64, f64)>> = VecDeque::new();
    let mut turn = [(0.0f64, 0.0f64, 0.0f64); HEADING_LAGS.len()];
    let run = Instant::now();
    // Ancient crust (see ANCIENT_MYR) whose cell another plate overran, by
    // whether it carried a keel, as shares of the globe.
    let (mut overrun_ancient, mut overrun_ancient_keeled) = (0.0f64, 0.0f64);
    let mut unplaced_ancient = 0.0f64;
    // Rifting, deformed (fresh suture), thin.
    let mut keel_erosion = [0.0f64; 3];
    for tick in 0..ticks {
        let before: Vec<(bool, f32, f32)> = {
            let d = sim.front();
            (0..sim.mesh.cell_count)
                .map(|i| (d.crust_type[i] == CONTINENTAL, d.age[i], d.keel[i]))
                .collect()
        };
        let cutoff = (sim.elapsed() - ANCIENT_MYR) as f32;
        {
            // Why keels on ancient crust are eroding, by area-Myr.
            let d = sim.front();
            for i in 0..sim.mesh.cell_count {
                if d.crust_type[i] != CONTINENTAL || d.keel[i] <= 0.0 || d.age[i] < cutoff {
                    continue;
                }
                let a = sim.mesh.area(i) / globe * sim.tuning.dt_myr;
                if d.rift_side[i] != 0 {
                    keel_erosion[0] += a;
                } else if d.weakness[i] >= 0.95 {
                    keel_erosion[1] += a;
                } else if (d.thickness[i] as f64) < sim.tuning.continental_thickness_threshold {
                    keel_erosion[2] += a;
                }
            }
        }
        sim.tick();
        for (i, &(cont, age, keel)) in before.iter().enumerate() {
            if !cont || age < cutoff {
                continue;
            }
            let a = sim.mesh.area(i) / globe;
            match sim.advection().overrun[i] {
                1 => overrun_ancient += a,
                2 => unplaced_ancient += a,
                _ => {}
            }
            if sim.advection().overrun[i] != 0 && keel >= CRATON_KEEL {
                overrun_ancient_keeled += a;
            }
        }
        rifts_at.push_back(sim.rift_count());
        if rifts_at.len() > RIFT_BURST_TICKS + 1 {
            rifts_at.pop_front();
        }
        rift_burst = rift_burst.max(rifts_at[rifts_at.len() - 1] - rifts_at[0]);
        let now_myr = sim.elapsed();
        {
            let adv = sim.advection();
            made_by_plume += adv.continentalised_by_plume / globe;
            made_by_arc += adv.continentalised_by_accretion / globe;
            made_by_piling += adv.continentalised_by_piling / globe;
            plume_mass_made += adv.plume_created_this_step;
        }
        tally_piling(&sim, &mut piling);
        let fresh: Vec<i64> = sim
            .plates()
            .breakups
            .keys()
            .copied()
            .filter(|k| !known_breakups.contains(k))
            .collect();
        if !fresh.is_empty() {
            let land = continental_area_by_plate(&sim);
            for key in fresh {
                known_breakups.insert(key);
                let (a, b) = tectonics_sim::plates::split_key(key);
                let big = |p: i32| land.get(&p).copied().unwrap_or(0.0) >= CONTINENT_SHARE;
                if tick >= settled && big(a) && big(b) {
                    let d = sim.front();
                    let (mut heat, mut count) = (0.0, 0usize);
                    for i in 0..sim.mesh.cell_count {
                        if d.crust_type[i] == CONTINENTAL
                            && (d.plate_id[i] == a || d.plate_id[i] == b)
                        {
                            heat += d.mantle_heat[i] as f64;
                            count += 1;
                        }
                    }
                    breakups.push(Breakup {
                        a,
                        b,
                        start: now_myr,
                        heat: heat / count.max(1) as f64,
                        opening: None,
                        welded: None,
                        parting: [None, None],
                        gaps: Vec::new(),
                        ended: None,
                    });
                }
            }
        }
        if (tick + 1) % GAP_TICKS == 0 {
            for br in breakups
                .iter_mut()
                .filter(|br| br.ended.is_none() && now_myr - br.start <= follow)
            {
                let live = sim.plates().by_id.contains_key(&br.a)
                    && sim.plates().by_id.contains_key(&br.b);
                match live.then(|| gap_km(&sim, br.a, br.b)).flatten() {
                    Some(g) => br.gaps.push((now_myr, g)),
                    None => br.ended = Some(now_myr),
                }
                if live && br.opening.is_none() {
                    br.opening = opening_share(&sim, br.a, br.b);
                }
                let since = now_myr - br.start;
                for (slot, at) in [(0usize, 10.0), (1, 30.0)] {
                    if live && br.parting[slot].is_none() && since >= at {
                        br.parting[slot] = parting(&sim, br.a, br.b);
                    }
                }
                let key = tectonics_sim::plates::suture_key(br.a, br.b);
                if br.welded.is_none() && sim.plates().sutures.contains_key(&key) {
                    br.welded = Some(now_myr - br.start);
                }
            }
        }
        if (tick + 1) % budget_every == 0 && !quiet {
            print_continental_budget(&sim);
        }
        if (tick + 1) % SERIES_TICKS == 0 {
            let a = assembly(&sim);
            println!(
                "series {:>5.0} cont {:.3} strict {:.2} bridged {:.2} clump {:.2} plates {:>3} sutures {:>2} slip {:>6.1} \
                 passive {:.2} age {:>5.0} old {:.2} sink {:.2} align {:+.2} flow/speed {:.2} rifts {} collapses {} \
                 remote {:.0} fragments {:.2} incap {:.2} compact {:.2} reach {:.0} capreach {:.0}",
                sim.elapsed(),
                sim.metrics().continental_area,
                a.strict,
                a.bridged,
                a.clump,
                a.plates,
                a.sutures,
                a.suture_slip,
                a.passive,
                a.passive_age,
                a.passive_old,
                a.sink_dipole,
                a.flow_align,
                a.flow_ratio,
                sim.rift_count(),
                sim.plates().passive_collapses,
                a.remote_km,
                a.fragments,
                a.in_cap,
                a.main_compact,
                a.main_reach,
                a.main_cap_reach,
            );
            let (craton_count, craton_area, craton_aspect) = cratons(&sim);
            // Continental crust dating from the first ANCIENT_MYR, as a share
            // of the globe, and how much of it still has a keel.
            let (ancient, ancient_keeled) = {
                let d = sim.front();
                let cutoff = (sim.elapsed() - ANCIENT_MYR) as f32;
                let (mut all, mut keeled) = (0.0, 0.0);
                for i in 0..sim.mesh.cell_count {
                    if d.crust_type[i] == CONTINENTAL && d.age[i] >= cutoff {
                        all += sim.mesh.area(i);
                        if d.keel[i] >= CRATON_KEEL {
                            keeled += sim.mesh.area(i);
                        }
                    }
                }
                (all / globe, keeled / globe)
            };
            // Plume crust still alive, by the tracer: as continent, as
            // continent under a keel, and anywhere, as shares of all made.
            let (plume_alive, plume_keeled, plume_anywhere) = {
                let d = sim.front();
                let (mut cont, mut keeled, mut any) = (0.0, 0.0, 0.0);
                for i in 0..sim.mesh.cell_count {
                    let m = sim.mesh.area(i) * d.thickness[i] as f64 * d.plume_share[i] as f64;
                    any += m;
                    if d.crust_type[i] == CONTINENTAL {
                        cont += m;
                        if d.keel[i] >= CRATON_KEEL {
                            keeled += m;
                        }
                    }
                }
                let made = plume_mass_made.max(1e-12);
                (cont / made, keeled / made, any / made)
            };
            println!(
                "era {:>5.0} heat {:.2} plumes {} made_plume {:.3} made_arc {:.3} made_piling {:.3} cratons {} craton_area {:.4} craton_aspect {:.2} craton_share {:.2} \
                 ancient {:.3} ancient_keeled {:.3} overrun {:.3} unplaced {:.3} lost_keeled {:.3} \
                 plume_alive {:.3} plume_keeled {:.3} plume_anywhere {:.3} craton_split {:.3}",
                sim.elapsed(),
                sim.mantle_excess(),
                sim.active_plumes(),
                made_by_plume,
                made_by_arc,
                made_by_piling,
                craton_count,
                craton_area,
                craton_aspect,
                sim.metrics().craton_share,
                ancient,
                ancient_keeled,
                overrun_ancient,
                unplaced_ancient,
                overrun_ancient_keeled,
                plume_alive,
                plume_keeled,
                plume_anywhere,
                split_craton_share(&sim),
            );
            if tick >= settled {
                series.push(a);
            }
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
    if !series.is_empty() {
        let stat = |f: &dyn Fn(&Assembly) -> f64| {
            let v: Vec<f64> = series.iter().map(f).collect();
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            let (lo, hi) = v
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &x| (l.min(x), h.max(x)));
            format!("{mean:.2} [{lo:.2}..{hi:.2}]")
        };
        // Swings: rises from under `low` to over `high`, a full gather after a
        // proper dispersal.
        let swings = |f: &dyn Fn(&Assembly) -> f64, low: f64, high: f64| {
            let (mut armed, mut count) = (false, 0);
            for a in &series {
                let x = f(a);
                if x < low {
                    armed = true;
                } else if x > high && armed {
                    armed = false;
                    count += 1;
                }
            }
            count
        };
        println!(
            "  assembly after {GROWN_MYR:.0} Myr, mean [min..max]: strict {} bridged {} clump {} plates {} \
             passive {} passive age {} old passive {} sink dipole {} align {} flow/speed {} suture slip {}",
            stat(&|a| a.strict),
            stat(&|a| a.bridged),
            stat(&|a| a.clump),
            stat(&|a| a.plates as f64),
            stat(&|a| a.passive),
            stat(&|a| a.passive_age),
            stat(&|a| a.passive_old),
            stat(&|a| a.sink_dipole),
            stat(&|a| a.flow_align),
            stat(&|a| a.flow_ratio),
            stat(&|a| a.suture_slip),
        );
        println!(
            "  geography: most remote ocean {} km, fragments {}, in cap {}",
            stat(&|a| a.remote_km),
            stat(&|a| a.fragments),
            stat(&|a| a.in_cap),
        );
        println!(
            "  swings (<0.4 then >0.75): strict {} bridged {}; passive collapses {}",
            swings(&|a| a.strict, 0.4, 0.75),
            swings(&|a| a.bridged, 0.4, 0.75),
            sim.plates().passive_collapses,
        );
    }
    println!(
        "  ancient keels eroding, globe-share Myr: rifting {:.3}, fresh suture {:.3}, thin {:.3}",
        keel_erosion[0], keel_erosion[1], keel_erosion[2],
    );
    let piled: f64 = piling.by_ring.iter().sum::<f64>().max(1e-12);
    let pct = |x: f64| x / piled * 100.0;
    println!(
        "  piled continent by rings from a boundary: {:.0}% on it, {:.0}% 1, {:.0}% 2, {:.0}% 3, {:.0}% 4+; \
         on it by kind: none {:.0}% convergent {:.0}% divergent {:.0}% transform {:.0}%; beside continent {:.0}%",
        pct(piling.by_ring[0]),
        pct(piling.by_ring[1]),
        pct(piling.by_ring[2]),
        pct(piling.by_ring[3]),
        pct(piling.by_ring[4]),
        pct(piling.by_kind[0]),
        pct(piling.by_kind[1]),
        pct(piling.by_kind[2]),
        pct(piling.by_kind[3]),
        pct(piling.beside_continent),
    );
    let (hot, cold): (Vec<&Breakup>, Vec<&Breakup>) =
        breakups.iter().partition(|b| b.heat >= HOT_BREAKUP);
    print_breakups("hot", &hot, sim.elapsed());
    print_breakups("cold", &cold, sim.elapsed());
}
