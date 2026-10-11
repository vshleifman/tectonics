//! Rifting: a plate tears where the tension across it most overcomes the
//! strength of the line it would tear along, and the far side of that line
//! becomes a new plate.
//!
//! Tension is the slab pull that cancels across the plate, spread over the
//! plate's mean width across it, so a large plate is under as much stress as a
//! small one pulled the same way per unit of margin. Trapped mantle heat adds
//! extension of its own and weakens the line; inherited weakness only weakens.
//! What the line runs through matters too: continental lithosphere is the weak
//! kind, while oceanic lithosphere thickens as it cools, so young ocean near a
//! ridge can tear but old ocean all but never does.
//!
//! The line may run at any angle to the tension, which it then feels resolved
//! onto it, and it must not leave either piece a sliver. A plate's tension
//! tends to run across its longest margins, so a rift always cut square to it
//! would slice an elongated plate lengthwise into ever thinner strips.
//!
//! The line is chosen straight and then allowed to wander: within a band either
//! side of it, the two halves are grown in from the band's edges, strongest
//! lithosphere first, so they meet along the weakest path near the line. A rift
//! near an old suture follows the suture rather than cutting across it.
//!
//! A rift through continent does not open at once. It stretches for
//! `rift_duration` Myr, thinning the crust along it into a basin and piling what
//! it loses onto shoulders on either side, and only then breaks the plate in
//! two. If the tension behind it dies away first it stalls: a failed rift,
//! thinned and left weak, but whole. Oceanic lithosphere has no such phase; a
//! rift opens in proportion to how much of its line runs through continent.
//!
//! Breaking open vents much of the heat beneath the plate, as the melting of
//! breakup does, so a hot supercontinent comes apart in stages rather than
//! shattering into every piece it can in a few ticks. Heat only builds some way
//! in from a continent's coast, so it is large continents that trap it.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::cell_data::{CONTINENTAL, CellData, RIFT_FAR, RIFT_NEAR};
use crate::mantle::MantleFlow;
use crate::math::{Vec3, clamp01, cross, dot, len3, normalise, rotate};
use crate::mesh::Mesh;
use crate::par;
use crate::plates::{OpeningRift, PlateData, PlateProps, PlateStress, suture_key};
use crate::registry::Registry;
use crate::scratch::Scratch;
use crate::shape::Moments;
use crate::tuning::{CRATON_KEEL, SimOptions, Tuning};

const CUT_BAND: f64 = 0.06;
const SCAN_BINS: usize = 64;
/// Orientations tried for a rift, evenly spread over a half turn.
const RIFT_ANGLES: usize = 6;
/// Half-width of the band, as a share of the plate's extent across the rift,
/// within which a rift may stray from its straight line.
const WANDER_BAND: f64 = 0.15;
/// How strongly a rift is held to its straight line against the pull of weaker
/// lithosphere off it, in units of relative strength.
const STRAIGHTNESS: f64 = 0.3;
/// Share by which a stretching rift's drive may fall below its strength before
/// it stalls, so a rift that has started does not fail at the first wobble.
const RIFT_HOLD: f64 = 0.5;
/// Rings of cells either side of a stretching rift that take up the crust it
/// thins away, as raised shoulders.
const SHOULDER_RINGS: usize = 2;

struct Cut {
    offset: f64,
    /// How far the line's drive exceeds its strength; negative if it holds.
    excess: f64,
}

/// A rift a plate is ready to open: the line perpendicular to `axis` at
/// `offset`.
struct Rift {
    axis: Vec3,
    offset: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn update_rifting(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    mantle: &MantleFlow,
    dt_myr: f64,
    registry: &mut Registry,
    s: &mut Scratch,
    t: &Tuning,
    o: &SimOptions,
) {
    // Weakness heals, but barely.
    let keep = (-dt_myr / t.weakness_decay_time.max(1e-6)).exp();
    par::for_each_mut(&mut data.weakness, |_, w| *w = (*w as f64 * keep) as f32);
    if o.supercontinent_insulation {
        insulate(mesh, data, mantle.cell_temps(), dt_myr, s, t);
    }

    // Each plate's cells by one counting sort, visited in order of lowest cell.
    let n = mesh.cell_count;
    let max_id = data.plate_id.iter().copied().max().unwrap_or(-1);
    let bound = (max_id + 1).max(0) as usize;
    let mut start = vec![0usize; bound + 1];
    let mut area_of = vec![0.0f64; bound];
    let mut visit: Vec<i32> = Vec::new();
    let mut total_area = 0.0f64;
    for i in 0..n {
        total_area += mesh.area(i);
        let id = data.plate_id[i];
        if id < 0 {
            continue;
        }
        let p = id as usize;
        if start[p + 1] == 0 {
            visit.push(id);
        }
        start[p + 1] += 1;
        area_of[p] += mesh.area(i);
    }
    for p in 0..bound {
        start[p + 1] += start[p];
    }
    let mut cells_by_plate = std::mem::take(&mut s.i32b);
    let mut write = start.clone();
    for i in 0..n {
        let id = data.plate_id[i];
        if id >= 0 {
            cells_by_plate[write[id as usize]] = i as i32;
            write[id as usize] += 1;
        }
    }

    // Rifts on plates that are gone, and the marks of rifts that are.
    let by_id = &plates.by_id;
    plates.opening.retain(|id, _| by_id.contains_key(id));
    let (opening, plate_id) = (&plates.opening, &data.plate_id);
    par::for_each_mut(&mut data.rift_side, |i, side| {
        if *side != 0 && !opening.contains_key(&plate_id[i]) {
            *side = 0;
        }
    });
    s.u8b.fill(0);

    // A rift must leave pieces of `min_area` when it starts. By the time it
    // breaks, either may have lost crust down a trench, and then it only has
    // to leave pieces big enough to be plates at all.
    let min_area = t.min_rift_area * total_area;
    let min_piece = t.min_plate_area * total_area;
    for &id in &visit {
        let p = id as usize;
        let cells = &cells_by_plate[start[p]..start[p + 1]];
        if plates.opening.contains_key(&id) {
            stretch(
                mesh, data, plates, id, cells, dt_myr, registry, min_piece, s, t, o,
            );
            continue;
        }
        if area_of[p] < 2.0 * min_area {
            continue;
        }
        let stress = plates.stress.get(&id);
        let Some(rift) = pick_rift(mesh, data, cells, area_of[p], min_area, stress, t, o) else {
            continue;
        };
        // Continental lithosphere stretches for a long time before it parts;
        // oceanic lithosphere, once it gives, opens almost at once.
        let zone = zone_of(mesh, data, cells, id);
        let zone_area: f64 = zone.iter().map(|&i| mesh.area(i)).sum();
        let continental: f64 = zone
            .iter()
            .filter(|&&i| data.crust_type[i] == CONTINENTAL)
            .map(|&i| mesh.area(i))
            .sum();
        let duration = t.rift_duration * continental / zone_area.max(1e-12);
        if duration >= dt_myr {
            plates.opening.insert(
                id,
                OpeningRift {
                    elapsed: 0.0,
                    duration,
                    axis: rift.axis,
                },
            );
        } else if !break_up(mesh, data, plates, id, cells, registry, min_piece, s, t) {
            clear_sides(data, cells);
        }
    }
    s.i32b = cells_by_plate;
}

/// The cells of plate `id` that border the other side of its rift, and are
/// stretched by it. A rift skirting a craton stretches the younger crust
/// beside it, never the craton's edge.
fn zone_of(mesh: &Mesh, data: &CellData, cells: &[i32], id: i32) -> Vec<usize> {
    cells
        .iter()
        .map(|&c| c as usize)
        .filter(|&i| {
            !cratonic(data, i)
                && mesh.nbrs(i).any(|k| {
                let m = mesh.nbr[k] as usize;
                data.plate_id[m] == id && data.rift_side[m] != data.rift_side[i]
            })
        })
        .collect()
}

/// Build heat under continents and lose it under ocean. A continent only
/// insulates the mantle beneath it some way in from its coast, reaching full
/// effect `insulation_reach` inland, so heat builds fastest under the interior
/// of a large continent and hardly at all under a small one. It builds faster
/// still over upper mantle that runs hot, and slower over mantle chilled by
/// slabs; `upper_temp` is that anomaly beneath each cell (K).
fn insulate(
    mesh: &Mesh,
    data: &mut CellData,
    upper_temp: &[f32],
    dt_myr: f64,
    s: &mut Scratch,
    t: &Tuning,
) {
    let n = mesh.cell_count;
    let hops = &mut s.i32a;
    let queue = &mut s.queue;
    let mut tail = 0usize;
    for i in 0..n {
        if data.crust_type[i] == CONTINENTAL {
            hops[i] = -1;
        } else {
            hops[i] = 0;
            queue[tail] = i as i32;
            tail += 1;
        }
    }
    let mut head = 0usize;
    while head < tail {
        let cell = queue[head] as usize;
        head += 1;
        for k in mesh.nbrs(cell) {
            let m = mesh.nbr[k] as usize;
            if hops[m] < 0 {
                hops[m] = hops[cell] + 1;
                queue[tail] = m as i32;
                tail += 1;
            }
        }
    }
    let spacing = (4.0 * std::f64::consts::PI / n as f64).sqrt();
    let (hops, crust_type) = (&*hops, &data.crust_type);
    par::for_each_mut(&mut data.mantle_heat, |i, heat| {
        let rate = if crust_type[i] == CONTINENTAL {
            // A world with no ocean left is all interior.
            let inland = if hops[i] < 0 || t.insulation_reach <= 0.0 {
                1.0
            } else {
                clamp01(hops[i] as f64 * spacing / t.insulation_reach)
            };
            let site = if t.upper_temp_max > 0.0 {
                (1.0 + t.insulation_site_gain * upper_temp[i] as f64 / t.upper_temp_max).max(0.0)
            } else {
                1.0
            };
            t.insulation_rate * inland * site
        } else {
            -t.insulation_cooling
        };
        *heat = clamp01(*heat as f64 + rate * dt_myr) as f32;
    });
}

/// Strength of a cell's lithosphere relative to continental, before weakness
/// and heat. Oceanic lithosphere thickens as the square root of its age.
fn lithosphere_strength(t: &Tuning, data: &CellData, i: usize) -> f64 {
    if data.crust_type[i] == CONTINENTAL {
        // A cratonic keel is cold and stiff; rifts go around it, and break
        // continents along the weak seams between cratons instead: the
        // Atlantic opened along the sutures that had closed the Iapetus.
        return 1.0 + t.craton_strength * data.keel[i] as f64;
    }
    let cooled = clamp01(data.age[i] as f64 / t.max_oceanic_age).sqrt();
    t.oceanic_strength_young + (t.oceanic_strength_old - t.oceanic_strength_young) * cooled
}

/// Mantle heat under a cell as rifting feels it: none with insulation off.
fn heat_at(data: &CellData, i: usize, o: &SimOptions) -> f64 {
    if o.supercontinent_insulation {
        data.mantle_heat[i] as f64
    } else {
        0.0
    }
}

/// How hard a cell is to tear through, as a multiple of `rift_strength`.
fn cell_strength(t: &Tuning, data: &CellData, i: usize, o: &SimOptions) -> f64 {
    let relief =
        t.weakness_relief * data.weakness[i] as f64 + t.insulation_weakening * heat_at(data, i, o);
    lithosphere_strength(t, data, i) * (1.0 - relief).max(0.0)
}

/// `v` projected into the plane tangent at `centre`, as a unit vector.
fn tangent_at(centre: Vec3, v: Vec3) -> Option<Vec3> {
    let along = dot(v, centre);
    let u = [
        v[0] - along * centre[0],
        v[1] - along * centre[1],
        v[2] - along * centre[2],
    ];
    (len3(u[0], u[1], u[2]) > 1e-6).then(|| normalise(u))
}

/// Choose and lay out the rift that most overcomes the plate's strength, over
/// lines at several angles to its tension and at every offset across it,
/// among those that cut through no craton, leave each piece at least
/// `min_area` of the plate's `plate_area`, and leave no piece longer than
/// `max_rift_aspect` times its width, or at least rounder than the plate was.
/// `None`, with no sides laid out, if no such line gives.
#[allow(clippy::too_many_arguments)]
fn pick_rift(
    mesh: &Mesh,
    data: &mut CellData,
    cells: &[i32],
    plate_area: f64,
    min_area: f64,
    stress: Option<&PlateStress>,
    t: &Tuning,
    o: &SimOptions,
) -> Option<Rift> {
    let shape = Moments::of(mesh, cells.iter().map(|&c| c as usize));
    let centre = shape.centre()?;
    // Tension with no slab pull behind it (a dome over trapped heat) has no
    // direction of its own, so the scan starts from the plate's long axis.
    let pull = stress.map_or(0.0, |st| st.opposed_pull);
    let u = stress
        .and_then(|st| tangent_at(centre, st.axis))
        .or_else(|| shape.long_axis().and_then(|a| tangent_at(centre, a)))?;
    let v = normalise(cross(centre, u));
    let parent_aspect = shape.aspect();
    // Each cell's area, and its heat and strength weighted by it, for the
    // scans at every angle.
    let weighted: Vec<[f64; 3]> = cells
        .iter()
        .map(|&c| {
            let i = c as usize;
            let a = mesh.area(i);
            [a, heat_at(data, i, o) * a, cell_strength(t, data, i, o) * a]
        })
        .collect();

    let mut found: Vec<(f64, Rift)> = Vec::new();
    for k in 0..RIFT_ANGLES {
        let angle = k as f64 * std::f64::consts::PI / RIFT_ANGLES as f64;
        let (sin, cos) = angle.sin_cos();
        let axis = [
            cos * u[0] + sin * v[0],
            cos * u[1] + sin * v[1],
            cos * u[2] + sin * v[2],
        ];
        for cut in cuts(mesh, cells, &weighted, axis, pull * cos * cos, t) {
            found.push((
                cut.excess,
                Rift {
                    axis,
                    offset: cut.offset,
                },
            ));
        }
    }
    // Stable, so of equal lines the first angle tried, and the most central
    // line at it, still come first.
    found.sort_by(|a, b| b.0.total_cmp(&a.0));

    let cratons = Cratons::of(mesh, data, cells);
    for (_, rift) in found {
        if cratons.crossed_by(mesh, cells, &rift) {
            continue;
        }
        let (mut near, mut far) = (Moments::default(), Moments::default());
        for &cell in cells {
            let i = cell as usize;
            if along(mesh, i, rift.axis) > rift.offset {
                far.add(mesh, i);
            } else {
                near.add(mesh, i);
            }
        }
        let pieces = near.aspect().max(far.aspect());
        if pieces > t.max_rift_aspect && pieces >= parent_aspect {
            continue;
        }
        if !lay_out(mesh, data, cells, &rift, &cratons, t, o) {
            clear_sides(data, cells);
            continue;
        }
        let far_area: f64 = cells
            .iter()
            .filter(|&&c| data.rift_side[c as usize] == RIFT_FAR)
            .map(|&c| mesh.area(c as usize))
            .sum();
        if far_area < min_area || plate_area - far_area < min_area {
            clear_sides(data, cells);
            continue;
        }
        return Some(rift);
    }
    None
}

#[inline]
fn along(mesh: &Mesh, cell: usize, axis: Vec3) -> f64 {
    let c = &mesh.centres;
    c[cell * 3] as f64 * axis[0]
        + c[cell * 3 + 1] as f64 * axis[1]
        + c[cell * 3 + 2] as f64 * axis[2]
}

/// Lowest and highest of the plate's cells along `axis`.
fn extent(mesh: &Mesh, cells: &[i32], axis: Vec3) -> (f64, f64) {
    cells
        .iter()
        .map(|&c| along(mesh, c as usize, axis))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        })
}

/// The pull carried across the plate, spread over the plate's mean width
/// across `axis` (its area over its extent) rather than the width at any one
/// line. Taking each line's own width instead would favour the narrow lines
/// near the ends.
fn spread_tension(mesh: &Mesh, cells: &[i32], axis: Vec3, pull: f64) -> f64 {
    let (lo, hi) = extent(mesh, cells, axis);
    let area: f64 = cells.iter().map(|&c| mesh.area(c as usize)).sum();
    pull * (hi - lo) / area.max(1e-12)
}

/// The lines across the plate perpendicular to `axis` that overcome its
/// strength, scanned over bins and kept clear of the ends, from the middle
/// outwards. `weighted` holds each cell's area and its heat and strength
/// times that area; `pull` is the opposed slab pull resolved onto `axis`.
fn cuts(
    mesh: &Mesh,
    cells: &[i32],
    weighted: &[[f64; 3]],
    axis: Vec3,
    pull: f64,
    t: &Tuning,
) -> Vec<Cut> {
    let (lo, hi) = extent(mesh, cells, axis);
    let span = hi - lo;
    if !span.is_finite() || span <= 1e-6 {
        return Vec::new();
    }
    let mut area_bin = [0.0f64; SCAN_BINS];
    let mut heat_bin = [0.0f64; SCAN_BINS];
    let mut strength_bin = [0.0f64; SCAN_BINS];
    let mut area = 0.0;
    for (&cell, &[a, heat, strength]) in cells.iter().zip(weighted) {
        let u = (along(mesh, cell as usize, axis) - lo) / span;
        let bin = ((u * SCAN_BINS as f64).floor().max(0.0) as usize).min(SCAN_BINS - 1);
        area += a;
        area_bin[bin] += a;
        heat_bin[bin] += heat;
        strength_bin[bin] += strength;
    }
    let tension = pull * span / area.max(1e-12);

    let half = ((CUT_BAND * SCAN_BINS as f64).round() as usize).max(1);
    let first = (0.25 * SCAN_BINS as f64).round() as usize;
    let last = (0.75 * SCAN_BINS as f64).round() as usize;
    let middle = (first + last) / 2;
    // From the middle outwards, so the first of equal lines is the central one.
    let from_middle = (0..=(last - first)).map(|k| {
        if k % 2 == 0 {
            middle + k / 2
        } else {
            middle - k.div_ceil(2)
        }
    });
    let mut found = Vec::new();
    for centre in from_middle {
        let (mut weight, mut heat, mut strength) = (0.0, 0.0, 0.0);
        for b in centre.saturating_sub(half)..=(centre + half).min(SCAN_BINS - 1) {
            weight += area_bin[b];
            heat += heat_bin[b];
            strength += strength_bin[b];
        }
        if weight <= 0.0 {
            continue;
        }
        let drive = tension + t.insulation_drive * heat / weight;
        let excess = drive - t.rift_strength * strength / weight;
        if excess >= 0.0 {
            found.push(Cut {
                offset: lo + span * (centre as f64 + 0.5) / SCAN_BINS as f64,
                excess,
            });
        }
    }
    found
}

/// A band cell waiting to be claimed by one side of a rift as it is laid out.
struct Grow {
    priority: f32,
    cell: usize,
    side: u8,
}

impl PartialEq for Grow {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Grow {}

impl PartialOrd for Grow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Grow {
    /// Reversed, so the heap yields the lowest priority first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .total_cmp(&self.priority)
            .then(other.cell.cmp(&self.cell))
    }
}

/// Whether a cell is craton, which no rift may run through.
fn cratonic(data: &CellData, i: usize) -> bool {
    data.crust_type[i] == CONTINENTAL && data.keel[i] >= CRATON_KEEL
}

/// A plate's cratons: each connected body of craton cells on it.
struct Cratons {
    /// Cells of every craton, one craton's after another's.
    members: Vec<usize>,
    /// Where each craton's cells start in `members`, and one past the last.
    starts: Vec<usize>,
    /// The craton each cell is in, -1 for none.
    of: Vec<i32>,
}

impl Cratons {
    fn of(mesh: &Mesh, data: &CellData, cells: &[i32]) -> Cratons {
        let mut of = vec![-1i32; mesh.cell_count];
        let (mut members, mut starts) = (Vec::new(), Vec::new());
        for &cell in cells {
            let first = cell as usize;
            if !cratonic(data, first) || of[first] >= 0 {
                continue;
            }
            let id = starts.len() as i32;
            starts.push(members.len());
            of[first] = id;
            members.push(first);
            let mut head = members.len() - 1;
            while head < members.len() {
                let c = members[head];
                head += 1;
                for k in mesh.nbrs(c) {
                    let m = mesh.nbr[k] as usize;
                    if of[m] < 0 && data.plate_id[m] == data.plate_id[c] && cratonic(data, m) {
                        of[m] = id;
                        members.push(m);
                    }
                }
            }
        }
        starts.push(members.len());
        Cratons { members, starts, of }
    }

    fn count(&self) -> usize {
        self.starts.len() - 1
    }

    fn cells(&self, c: usize) -> &[usize] {
        &self.members[self.starts[c]..self.starts[c + 1]]
    }

    /// Whether `rift` cuts clean across a craton, reaching out of the band
    /// it may wander in on both sides, so it could only open through it.
    fn crossed_by(&self, mesh: &Mesh, cells: &[i32], rift: &Rift) -> bool {
        if self.count() == 0 {
            return false;
        }
        let (lo, hi) = extent(mesh, cells, rift.axis);
        let band = wander_band(lo, hi);
        (0..self.count()).any(|c| {
            let (mut near, mut far) = (false, false);
            for &m in self.cells(c) {
                let d = along(mesh, m, rift.axis) - rift.offset;
                near |= d < -band;
                far |= d > band;
            }
            near && far
        })
    }
}

fn wander_band(lo: f64, hi: f64) -> f64 {
    (WANDER_BAND * (hi - lo)).max(1e-9)
}

/// Mark which side of `rift` each of the plate's cells lies on. Well clear of
/// the line that is simply which side of it the cell is. Within `WANDER_BAND`
/// of it the two sides grow in from the band's edges, strongest lithosphere
/// first, so the weakest cells near the line are the last reached and the
/// sides meet along them. A craton goes whole to whichever side reaches it
/// first, so the rift skirts it; `false` if the line cuts clean across one,
/// with no way round it within the band.
fn lay_out(
    mesh: &Mesh,
    data: &mut CellData,
    cells: &[i32],
    rift: &Rift,
    cratons: &Cratons,
    t: &Tuning,
    o: &SimOptions,
) -> bool {
    let (lo, hi) = extent(mesh, cells, rift.axis);
    let band = wander_band(lo, hi);
    let offset_of = |i: usize| along(mesh, i, rift.axis) - rift.offset;
    let side_of = |d: f64| if d > 0.0 { RIFT_FAR } else { RIFT_NEAR };
    for &cell in cells {
        let i = cell as usize;
        let d = offset_of(i);
        data.rift_side[i] = if d.abs() > band { side_of(d) } else { 0 };
    }

    let craton = |c: usize| cratons.cells(c);
    let craton_of = &cratons.of;
    let claim = |data: &mut CellData, c: usize, side: u8| {
        for &m in craton(c) {
            data.rift_side[m] = side;
        }
    };
    for c in 0..cratons.count() {
        let (mut near, mut far) = (false, false);
        for &m in craton(c) {
            near |= data.rift_side[m] == RIFT_NEAR;
            far |= data.rift_side[m] == RIFT_FAR;
        }
        if near && far {
            return false;
        }
        if near || far {
            claim(data, c, if near { RIFT_NEAR } else { RIFT_FAR });
        }
    }

    let priority = |data: &CellData, i: usize| -> f32 {
        let straight = STRAIGHTNESS * (1.0 - offset_of(i).abs() / band);
        (straight - cell_strength(t, data, i, o)) as f32
    };
    let mut heap = BinaryHeap::new();
    for &cell in cells {
        let i = cell as usize;
        if data.rift_side[i] != 0 {
            continue;
        }
        for k in mesh.nbrs(i) {
            let m = mesh.nbr[k] as usize;
            let side = data.rift_side[m];
            if side != 0 && data.plate_id[m] == data.plate_id[i] {
                heap.push(Grow {
                    priority: priority(data, i),
                    cell: i,
                    side,
                });
            }
        }
    }
    while let Some(Grow { cell, side, .. }) = heap.pop() {
        if data.rift_side[cell] != 0 {
            continue;
        }
        let reached: &[usize] = match craton_of[cell] {
            c if c >= 0 => {
                claim(data, c as usize, side);
                craton(c as usize)
            }
            _ => {
                data.rift_side[cell] = side;
                std::slice::from_ref(&cell)
            }
        };
        for &r in reached {
            for k in mesh.nbrs(r) {
                let m = mesh.nbr[k] as usize;
                if data.rift_side[m] == 0 && data.plate_id[m] == data.plate_id[r] {
                    heap.push(Grow {
                        priority: priority(data, m),
                        cell: m,
                        side,
                    });
                }
            }
        }
    }
    for &cell in cells {
        let i = cell as usize;
        if data.rift_side[i] != 0 {
            continue;
        }
        let side = side_of(offset_of(i));
        match craton_of[i] {
            c if c >= 0 => claim(data, c as usize, side),
            _ => data.rift_side[i] = side,
        }
    }
    true
}

fn clear_sides(data: &mut CellData, cells: &[i32]) {
    for &cell in cells {
        data.rift_side[cell as usize] = 0;
    }
}

/// Give a side to any of the plate's cells that joined it since the rift was
/// laid out, from the nearest cell that has one.
fn fill_sides(mesh: &Mesh, data: &mut CellData, cells: &[i32], s: &mut Scratch) {
    let queue = &mut s.queue;
    let mut tail = 0usize;
    for &cell in cells {
        if data.rift_side[cell as usize] != 0 {
            queue[tail] = cell;
            tail += 1;
        }
    }
    let mut head = 0usize;
    while head < tail {
        let cell = queue[head] as usize;
        head += 1;
        for k in mesh.nbrs(cell) {
            let m = mesh.nbr[k] as usize;
            if data.rift_side[m] == 0 && data.plate_id[m] == data.plate_id[cell] {
                data.rift_side[m] = data.rift_side[cell];
                queue[tail] = m as i32;
                tail += 1;
            }
        }
    }
    for &cell in cells {
        let i = cell as usize;
        if data.rift_side[i] == 0 {
            data.rift_side[i] = RIFT_NEAR;
        }
    }
}

/// Advance a rift that is stretching: keep it open while it is still driven,
/// thin its zone between rising shoulders, and break the plate in two once it
/// has stretched for as long as it must.
#[allow(clippy::too_many_arguments)]
fn stretch(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    id: i32,
    cells: &[i32],
    dt_myr: f64,
    registry: &mut Registry,
    min_area: f64,
    s: &mut Scratch,
    t: &Tuning,
    o: &SimOptions,
) {
    fill_sides(mesh, data, cells, s);
    let zone = zone_of(mesh, data, cells, id);

    let omega = plates.by_id.get(&id).map_or([0.0; 3], |p| p.omega);
    let Some(rift) = plates.opening.get_mut(&id) else {
        return;
    };
    rift.elapsed += dt_myr;
    rift.axis = rotate(rift.axis, omega, dt_myr);
    let (elapsed, duration, axis) = (rift.elapsed, rift.duration, rift.axis);

    let centre = Moments::of(mesh, cells.iter().map(|&c| c as usize)).centre();
    let normal = centre.and_then(|c| tangent_at(c, axis));
    let both_sides = cells
        .iter()
        .any(|&c| data.rift_side[c as usize] == RIFT_FAR)
        && cells
            .iter()
            .any(|&c| data.rift_side[c as usize] == RIFT_NEAR);
    let (Some(centre), Some(normal), true, false) = (centre, normal, both_sides, zone.is_empty())
    else {
        clear_sides(data, cells);
        plates.opening.shift_remove(&id);
        plates.failed_rifts += 1;
        return;
    };

    let pull = plates.stress.get(&id).map_or(0.0, |st| {
        let along_rift = tangent_at(centre, st.axis).map_or(0.0, |a| dot(a, normal));
        st.opposed_pull * along_rift * along_rift
    });
    let (mut weight, mut heat, mut strength) = (0.0, 0.0, 0.0);
    for &i in &zone {
        let a = mesh.area(i);
        weight += a;
        heat += heat_at(data, i, o) * a;
        strength += cell_strength(t, data, i, o) * a;
    }
    let drive = spread_tension(mesh, cells, normal, pull) + t.insulation_drive * heat / weight;
    let strength = t.rift_strength * strength / weight;
    if drive < (1.0 - RIFT_HOLD) * strength {
        // Stalled: the thinned zone stays behind as a weak scar.
        let margin = t.rift_margin_weakness as f32;
        for &i in &zone {
            data.weakness[i] = data.weakness[i].max(margin);
        }
        clear_sides(data, cells);
        plates.opening.shift_remove(&id);
        plates.failed_rifts += 1;
        return;
    }

    // Stretching weakens the zone as it goes, so it keeps the strain to itself.
    let worn = (t.rift_margin_weakness * clamp01(elapsed / duration).sqrt()) as f32;
    for &i in &zone {
        data.weakness[i] = data.weakness[i].max(worn);
    }
    thin(mesh, data, &zone, dt_myr / duration, s, t);

    if elapsed >= duration && !break_up(mesh, data, plates, id, cells, registry, min_area, s, t) {
        clear_sides(data, cells);
        plates.opening.shift_remove(&id);
        plates.failed_rifts += 1;
    }
}

/// Thin the continental crust of a stretching zone by `share` of the whole
/// stretch, and lay what it loses on the continental shoulders either side, so
/// the zone sinks into a basin between raised flanks and no crust is lost.
/// Ocean along the zone is left alone, and never takes up the crust, which
/// would otherwise thicken it into new continent.
fn thin(mesh: &Mesh, data: &mut CellData, zone: &[usize], share: f64, s: &mut Scratch, t: &Tuning) {
    const ZONE: u8 = 1;
    const SHOULDER: u8 = 2;
    let continental = |data: &CellData, i: usize| data.crust_type[i] == CONTINENTAL;
    let zone: Vec<usize> = zone
        .iter()
        .copied()
        .filter(|&i| continental(data, i))
        .collect();
    let mark = &mut s.u8b;
    for &i in &zone {
        mark[i] = ZONE;
    }
    let mut shoulders: Vec<usize> = Vec::new();
    let mut ring: Vec<usize> = zone.clone();
    for _ in 0..SHOULDER_RINGS {
        let mut next = Vec::new();
        for &cell in &ring {
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if mark[m] == 0 && data.plate_id[m] == data.plate_id[cell] && continental(data, m) {
                    mark[m] = SHOULDER;
                    next.push(m);
                }
            }
        }
        shoulders.extend_from_slice(&next);
        ring = next;
    }

    let shoulder_area: f64 = shoulders.iter().map(|&i| mesh.area(i)).sum();
    if shoulder_area > 0.0 {
        let keep = (-t.rift_stretch.max(1.0).ln() * share).exp();
        let mut lost = 0.0;
        for &i in &zone {
            let h = data.thickness[i] as f64;
            data.thickness[i] = (h * keep) as f32;
            lost += h * (1.0 - keep) * mesh.area(i);
        }
        let gain = lost / shoulder_area;
        for &i in &shoulders {
            data.thickness[i] = (data.thickness[i] as f64 + gain) as f32;
        }
    }
    for &i in zone.iter().chain(&shoulders) {
        mark[i] = 0;
    }
}

/// Break a plate along its laid-out rift: the largest connected piece of the
/// far side becomes a new plate, both new margins are stamped weak, and much
/// of the heat beneath is vented. `false`, changing nothing, if either piece
/// would be smaller than `min_area`.
#[allow(clippy::too_many_arguments)]
fn break_up(
    mesh: &Mesh,
    data: &mut CellData,
    plates: &mut PlateData,
    id: i32,
    cells: &[i32],
    registry: &mut Registry,
    min_area: f64,
    s: &mut Scratch,
    t: &Tuning,
) -> bool {
    let seen = &mut s.u8b;
    let queue = &mut s.queue;
    let is_far =
        |data: &CellData, i: usize| data.plate_id[i] == id && data.rift_side[i] == RIFT_FAR;
    let mut tail = 0usize;
    let (mut best_start, mut best_end, mut best_area) = (0usize, 0usize, 0.0f64);
    for &first in cells {
        let f = first as usize;
        if !is_far(data, f) || seen[f] != 0 {
            continue;
        }
        let piece_start = tail;
        queue[tail] = first;
        tail += 1;
        seen[f] = 1;
        let mut area = 0.0;
        let mut head = piece_start;
        while head < tail {
            let cell = queue[head] as usize;
            head += 1;
            area += mesh.area(cell);
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if is_far(data, m) && seen[m] == 0 {
                    seen[m] = 1;
                    queue[tail] = m as i32;
                    tail += 1;
                }
            }
        }
        if area > best_area {
            best_area = area;
            best_start = piece_start;
            best_end = tail;
        }
    }
    for &cell in cells {
        seen[cell as usize] = 0;
    }
    let plate_area: f64 = cells.iter().map(|&c| mesh.area(c as usize)).sum();
    if best_area < min_area || plate_area - best_area < min_area {
        return false;
    }

    // Breakup melting taps the heat that built up under the plate.
    let keep_heat = (1.0 - t.rift_heat_release).clamp(0.0, 1.0) as f32;
    for &cell in cells {
        data.mantle_heat[cell as usize] *= keep_heat;
    }
    plates.rifts += 1;
    plates.opening.shift_remove(&id);
    let new_id = registry.allocate();
    plates.breakups.insert(suture_key(id, new_id), 0.0);
    let omega = plates.by_id.get(&id).map_or([0.0, 0.0, 0.0], |p| p.omega);
    plates.by_id.insert(
        new_id,
        PlateProps {
            omega,
            pending: 0.0,
            steps: 0,
        },
    );
    for q in best_start..best_end {
        data.plate_id[queue[q] as usize] = new_id;
    }
    let margin = t.rift_margin_weakness as f32;
    for q in best_start..best_end {
        let cell = queue[q] as usize;
        for k in mesh.nbrs(cell) {
            let m = mesh.nbr[k] as usize;
            if data.plate_id[m] == new_id {
                continue;
            }
            for c in [cell, m] {
                if !cratonic(data, c) {
                    data.weakness[c] = data.weakness[c].max(margin);
                }
            }
        }
    }
    clear_sides(data, cells);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plate 0 is a continental cap over `z > 0`; everything else is plate 1.
    fn capped(level: u32) -> (Mesh, CellData, Vec<i32>) {
        let mesh = Mesh::icosphere(level);
        let mut data = CellData::new(mesh.cell_count);
        let mut cap = Vec::new();
        for i in 0..mesh.cell_count {
            let up = mesh.centres[i * 3 + 2] > 0.0;
            data.plate_id[i] = if up { 0 } else { 1 };
            data.crust_type[i] = CONTINENTAL;
            data.thickness[i] = 35.0;
            if up {
                cap.push(i as i32);
            }
        }
        (mesh, data, cap)
    }

    #[test]
    fn rift_follows_a_curved_suture() {
        let (mesh, mut data, cap) = capped(5);
        let c = |i: usize, k: usize| mesh.centres[i * 3 + k] as f64;
        // A bent seam across the cap, curving up to 0.15 off the straight line.
        let on_seam =
            |i: usize| (c(i, 0) - 0.25 * c(i, 1) * c(i, 1)).abs() < 0.04 && c(i, 1).abs() < 0.8;
        for &cell in &cap {
            if on_seam(cell as usize) {
                data.weakness[cell as usize] = 1.0;
            }
        }
        let rift = Rift {
            axis: [1.0, 0.0, 0.0],
            offset: 0.0,
        };
        let cratons = Cratons::of(&mesh, &data, &cap);
        lay_out(
            &mesh,
            &mut data,
            &cap,
            &rift,
            &cratons,
            &Tuning::default(),
            &SimOptions::default(),
        );

        // The far side's edge: cells that border the near side, within the
        // stretch of the seam.
        let edge: Vec<usize> = cap
            .iter()
            .map(|&c| c as usize)
            .filter(|&i| c(i, 1).abs() < 0.7 && data.rift_side[i] == RIFT_FAR)
            .filter(|&i| {
                mesh.nbrs(i).any(|k| {
                    let m = mesh.nbr[k] as usize;
                    data.plate_id[m] == 0 && data.rift_side[m] == RIFT_NEAR
                })
            })
            .collect();
        let near_seam = edge
            .iter()
            .filter(|&&i| mesh.nbrs(i).any(|k| on_seam(mesh.nbr[k] as usize)) || on_seam(i))
            .count();
        assert!(!edge.is_empty());
        let share = near_seam as f64 / edge.len() as f64;
        assert!(
            share > 0.8,
            "rift strays from the seam: {share:.2} of its edge on it"
        );
    }

    #[test]
    fn a_rift_left_straight_where_nothing_is_weaker() {
        let (mesh, mut data, cap) = capped(5);
        let rift = Rift {
            axis: [1.0, 0.0, 0.0],
            offset: 0.1,
        };
        let cratons = Cratons::of(&mesh, &data, &cap);
        lay_out(
            &mesh,
            &mut data,
            &cap,
            &rift,
            &cratons,
            &Tuning::default(),
            &SimOptions::default(),
        );
        for &cell in &cap {
            let i = cell as usize;
            let x = mesh.centres[i * 3] as f64;
            let expect = if x > 0.1 { RIFT_FAR } else { RIFT_NEAR };
            if (x - 0.1).abs() > 0.05 {
                assert_eq!(
                    data.rift_side[i], expect,
                    "cell at x {x:.3} on the wrong side"
                );
            }
        }
    }

    #[test]
    fn old_ocean_is_stronger_than_continent_and_young_ocean_weaker() {
        let t = Tuning::default();
        let mut data = CellData::new(3);
        data.crust_type[0] = CONTINENTAL;
        data.age[1] = 0.0;
        data.age[2] = t.max_oceanic_age as f32;
        let s = |i| lithosphere_strength(&t, &data, i);
        assert!(s(1) < s(0) && s(0) < s(2));
    }

    #[test]
    fn heat_builds_under_a_large_continent_not_a_small_one() {
        let mesh = Mesh::icosphere(6);
        let mut data = CellData::new(mesh.cell_count);
        let c = |i: usize, k: usize| mesh.centres[i * 3 + k] as f64;
        let t = Tuning::default();
        // A broad continent over the north, and far to the south an island
        // whose middle lies well within the insulating reach of its coast.
        let island_edge = -(0.3 * t.insulation_reach).cos();
        let mut interior = 0;
        let mut island = 0;
        for i in 0..mesh.cell_count {
            if c(i, 2) > 0.3 {
                data.crust_type[i] = CONTINENTAL;
                if c(i, 2) > c(interior, 2) {
                    interior = i;
                }
            }
            if c(i, 2) < island_edge {
                data.crust_type[i] = CONTINENTAL;
                if c(i, 2) < c(island, 2) {
                    island = i;
                }
            }
        }
        let mut s = Scratch::new(mesh.cell_count);
        insulate(&mesh, &mut data, &vec![0.0; mesh.cell_count], 10.0, &mut s, &t);
        let (deep, small) = (data.mantle_heat[interior], data.mantle_heat[island]);
        assert!(
            deep as f64 > 0.99 * t.insulation_rate * 10.0,
            "interior {deep}"
        );
        assert!(small < 0.6 * deep, "island {small} vs interior {deep}");
    }

    #[test]
    fn heat_builds_faster_over_hot_upper_mantle() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning::default();
        let built = |anomaly: f32| {
            let mut data = CellData::new(n);
            data.crust_type.fill(CONTINENTAL);
            let mut s = Scratch::new(n);
            insulate(&mesh, &mut data, &vec![anomaly; n], 10.0, &mut s, &t);
            data.mantle_heat[0]
        };
        let (cold, still, hot) = (built(-150.0), built(0.0), built(150.0));
        assert!(cold < still && still < hot, "cold {cold}, still {still}, hot {hot}");
        assert_eq!(built(-(t.upper_temp_max as f32)), 0.0);
    }
}
