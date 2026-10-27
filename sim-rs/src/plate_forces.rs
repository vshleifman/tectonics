//! Each plate's angular velocity from a force balance, mirroring
//! `plateForces.ts`: slab pull and ridge push drive, mantle drag and collision
//! resistance resist, and the balance `M omega = torque` gives the motion each
//! plate is heading for. Plates settle onto it over `motion_response_time`
//! rather than jumping straight there.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::elevation::{column_buoyancy, thermal_elevation};
use crate::mantle::MantleFlow;
use crate::math::{
    Sym3, Vec3, clamp01, cross, dominant_axis, dot, len3, normalise, scale, solve_sym3, sub,
};
use crate::mesh::Mesh;
use crate::par;
use crate::plates::{PlateData, PlateStress, split_key, suture_key};
use crate::scratch::Scratch;
use crate::slabs::{slab_pull, subduction};
use crate::tuning::{SimOptions, Tuning};
use std::collections::BTreeMap;

/// A stable id with no live plate.
const NO_SLOT: usize = usize::MAX;

/// Running totals of what acts on each live plate: a symmetric 3x3 as six
/// values, vectors as three.
struct Forces {
    torque: Vec<f64>,
    drag: Vec<f64>,
    pull_sum: Vec<f64>,
    pull_magnitude: Vec<f64>,
    pull_spread: Vec<f64>,
    area: Vec<f64>,
    /// Area-weighted sum of cell positions, for the plate's centre.
    centre: Vec<f64>,
}

impl Forces {
    fn new(plates: usize) -> Forces {
        Forces {
            torque: vec![0.0; plates * 3],
            drag: vec![0.0; plates * 6],
            pull_sum: vec![0.0; plates * 3],
            pull_magnitude: vec![0.0; plates],
            pull_spread: vec![0.0; plates * 6],
            area: vec![0.0; plates],
            centre: vec![0.0; plates * 3],
        }
    }

    fn add(&mut self, other: &Forces) {
        for (sum, part) in [
            (&mut self.torque, &other.torque),
            (&mut self.drag, &other.drag),
            (&mut self.pull_sum, &other.pull_sum),
            (&mut self.pull_magnitude, &other.pull_magnitude),
            (&mut self.pull_spread, &other.pull_spread),
            (&mut self.area, &other.area),
            (&mut self.centre, &other.centre),
        ] {
            sum.iter_mut().zip(part).for_each(|(s, x)| *s += x);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_plate_motion(
    mesh: &Mesh,
    data: &CellData,
    plates: &mut PlateData,
    boundaries: &BoundaryField,
    mantle: &MantleFlow,
    scratch: &mut Scratch,
    t: &Tuning,
    dt_myr: f64,
    o: &SimOptions,
) {
    let bound = scratch.pack_plates(plates);
    let by_id = &plates.by_id;
    plates.breakups.retain(|&key, age| {
        *age += dt_myr;
        let (a, b) = split_key(key);
        *age < t.dome_push_duration && by_id.contains_key(&a) && by_id.contains_key(&b)
    });
    // Totals are kept per live plate, by its place in `ids`, so each chunk's
    // share is only as large as the plate count.
    let ids: Vec<i32> = plates.by_id.keys().copied().collect();
    let mut slots = vec![NO_SLOT; bound];
    for (k, &id) in ids.iter().enumerate() {
        slots[id as usize] = k;
    }
    let slot_of = |plate: i32| -> Option<usize> {
        let s = *slots.get(usize::try_from(plate).ok()?)?;
        (s != NO_SLOT).then_some(s)
    };
    let c = &mesh.centres;

    // Resistance under the plate: every cell drags on the mantle beneath it,
    // continental keels harder. Where the mantle is itself flowing, toward a
    // downwelling, continental keels are carried along with it; ocean floor
    // rides on a soft asthenosphere that lets the deeper flow slide past, and
    // goes where its slabs pull it.
    let carried = t.mantle_flow_coupling;
    let ridge = t.ridge_weight;
    let under = par::chunks(mesh.cell_count, |cells| {
        let mut f = Forces::new(ids.len());
        for i in cells {
            let plate = data.plate_id[i];
            let Some(p) = slot_of(plate) else {
                continue;
            };
            let keel = if data.crust_type[i] == CONTINENTAL {
                t.continental_drag * (1.0 + t.craton_keel_gain * data.keel[i] as f64)
            } else {
                1.0
            };
            let k = t.mantle_drag * mesh.area(i) * keel;
            let (x, y, z) = (c[i * 3] as f64, c[i * 3 + 1] as f64, c[i * 3 + 2] as f64);
            let m = p * 6;
            f.drag[m] += k * (1.0 - x * x);
            f.drag[m + 1] += k * (1.0 - y * y);
            f.drag[m + 2] += k * (1.0 - z * z);
            f.drag[m + 3] -= k * x * y;
            f.drag[m + 4] -= k * x * z;
            f.drag[m + 5] -= k * y * z;
            f.area[p] += mesh.area(i);
            f.centre[p * 3] += mesh.area(i) * x;
            f.centre[p * 3 + 1] += mesh.area(i) * y;
            f.centre[p * 3 + 2] += mesh.area(i) * z;
            if carried > 0.0 && data.crust_type[i] == CONTINENTAL {
                let u = mantle.wind_at(i);
                let g = k * carried;
                f.torque[p * 3] += g * (y * u[2] - z * u[1]);
                f.torque[p * 3 + 1] += g * (z * u[0] - x * u[2]);
                f.torque[p * 3 + 2] += g * (x * u[1] - y * u[0]);
            }
            // Ridge push: ocean lithosphere thickens and sinks as it cools
            // away from a ridge, and slides down the slope of its own base. A
            // body force over the whole plate, down the gradient of its
            // cooling depth, so its pull adds up to the ridge's relief over
            // the plate's oldest floor rather than coming from the ridge line
            // alone.
            if ridge > 0.0 && data.crust_type[i] != CONTINENTAL {
                let depth = thermal_elevation(t, data.age[i] as f64);
                let mut push = [0.0f64; 3];
                for nk in mesh.nbrs(i) {
                    let m = mesh.nbr[nk] as usize;
                    let arc = mesh.nbr_arc[nk];
                    if arc < 0 || data.plate_id[m] != plate || data.crust_type[m] == CONTINENTAL {
                        continue;
                    }
                    let drop = (depth - thermal_elevation(t, data.age[m] as f64))
                        * mesh.arc_length[arc as usize] as f64;
                    for (axis, pa) in push.iter_mut().enumerate() {
                        *pa += drop * mesh.chord_dir[nk * 3 + axis] as f64;
                    }
                }
                f.torque[p * 3] += ridge * (y * push[2] - z * push[1]);
                f.torque[p * 3 + 1] += ridge * (z * push[0] - x * push[2]);
                f.torque[p * 3 + 2] += ridge * (x * push[1] - y * push[0]);
            }
        }
        f
    });

    let mid_of = |a: usize| -> [f64; 3] {
        [
            mesh.arc_mid[a * 3] as f64,
            mesh.arc_mid[a * 3 + 1] as f64,
            mesh.arc_mid[a * 3 + 2] as f64,
        ]
    };
    let normal_of = |a: usize| -> [f64; 3] {
        [
            mesh.arc_normal[a * 3] as f64,
            mesh.arc_normal[a * 3 + 1] as f64,
            mesh.arc_normal[a * 3 + 2] as f64,
        ]
    };
    // Torque of a tangential force `sign * normal * mag` at the arc midpoint.
    let add_torque = |torque: &mut [f64], plate: i32, a: usize, sign: f64, mag: f64| {
        let Some(p) = slot_of(plate) else {
            return;
        };
        let m = mid_of(a);
        let n = normal_of(a);
        let f = [n[0] * sign * mag, n[1] * sign * mag, n[2] * sign * mag];
        let p = p * 3;
        torque[p] += m[1] * f[2] - m[2] * f[1];
        torque[p + 1] += m[2] * f[0] - m[0] * f[2];
        torque[p + 2] += m[0] * f[1] - m[1] * f[0];
    };

    let (breakups, sutures) = (&plates.breakups, &plates.sutures);
    let across = par::chunks(mesh.arc_count(), |arcs| {
        let mut f = Forces::new(ids.len());
        for a in arcs {
            let cell_a = mesh.arc_cells[a * 2];
            let cell_b = mesh.arc_cells[a * 2 + 1];
            if cell_a < 0 || cell_b < 0 {
                continue;
            }
            let (cell_a, cell_b) = (cell_a as usize, cell_b as usize);
            let plate_a = data.plate_id[cell_a];
            let plate_b = data.plate_id[cell_b];
            if plate_a < 0 || plate_b < 0 || plate_a == plate_b {
                continue;
            }
            let kind = boundaries.arc_kind[a];
            let arc_len = mesh.arc_length[a] as f64;
            let cont_a = data.crust_type[cell_a] == CONTINENTAL;
            let cont_b = data.crust_type[cell_b] == CONTINENTAL;

            // Dome push: lithosphere raised over trapped mantle heat slides
            // off it, so the margin where a plate broke apart over a hot dome
            // pushes both sides away. This is what separates the halves of a
            // plate that rifted over heat alone, with no slab to pull them.
            // It lasts only `dome_push_duration` after the breakup, as breakup
            // melting vents the dome; a lasting push between every pair of hot
            // neighbours would hold all continents apart for good. A weld
            // holds against it. The dome has to straddle the margin, so the
            // cooler side sets the push.
            let heat = data.mantle_heat[cell_a].min(data.mantle_heat[cell_b]) as f64;
            let pair = suture_key(plate_a, plate_b);
            if heat > 0.0 && breakups.contains_key(&pair) && !sutures.contains_key(&pair) {
                let push = t.dome_push * heat * arc_len;
                add_torque(&mut f.torque, plate_a, a, -1.0, push);
                add_torque(&mut f.torque, plate_b, a, 1.0, push);
            }

            // Slab pull, not gated on the boundary converging right now: a
            // slab's weight is there whether or not the plates happen to be
            // moving. But it takes a slab, so only where crust has actually
            // gone down.
            if let Some(sub) = subduction(mesh, data, cell_a, cell_b, t) {
                let (down_cell, down_plate, over_plate, sign) = if sub.down == cell_b {
                    (cell_b, plate_b, plate_a, -1.0)
                } else {
                    (cell_a, plate_a, plate_b, 1.0)
                };
                let pull = slab_pull(t, column_buoyancy(t, data, down_cell)) * sub.strength;
                if let Some(p) = slot_of(down_plate).filter(|_| pull > 0.0) {
                    let force = t.slab_weight * pull * arc_len;
                    add_torque(&mut f.torque, down_plate, a, sign, force);
                    // Trench suction: the slab rolls back, and the mantle
                    // wedge above it draws the overriding plate after the
                    // trench.
                    add_torque(
                        &mut f.torque,
                        over_plate,
                        a,
                        -sign,
                        t.trench_suction * force,
                    );
                    let n = normal_of(a);
                    let d = [n[0] * sign, n[1] * sign, n[2] * sign];
                    f.pull_sum[p * 3] += d[0] * force;
                    f.pull_sum[p * 3 + 1] += d[1] * force;
                    f.pull_sum[p * 3 + 2] += d[2] * force;
                    f.pull_magnitude[p] += force;
                    let s = p * 6;
                    f.pull_spread[s] += force * d[0] * d[0];
                    f.pull_spread[s + 1] += force * d[1] * d[1];
                    f.pull_spread[s + 2] += force * d[2] * d[2];
                    f.pull_spread[s + 3] += force * d[0] * d[1];
                    f.pull_spread[s + 4] += force * d[0] * d[2];
                    f.pull_spread[s + 5] += force * d[1] * d[2];
                }
            }

            if kind != CONVERGENT {
                continue;
            }

            // Collision resistance, as drag across the boundary only.
            let lock = if cont_a && cont_b {
                1.0
            } else if cont_a || cont_b {
                1.0 / t.collision_lock_gain.sqrt()
            } else {
                1.0 / t.collision_lock_gain
            };
            let m = mid_of(a);
            let n = normal_of(a);
            let g = [
                m[1] * n[2] - m[2] * n[1],
                m[2] * n[0] - m[0] * n[2],
                m[0] * n[1] - m[1] * n[0],
            ];
            let k = t.collision_drag * lock * arc_len;
            for plate in [plate_a, plate_b] {
                let Some(p) = slot_of(plate) else {
                    continue;
                };
                let d = p * 6;
                f.drag[d] += k * g[0] * g[0];
                f.drag[d + 1] += k * g[1] * g[1];
                f.drag[d + 2] += k * g[2] * g[2];
                f.drag[d + 3] += k * g[0] * g[1];
                f.drag[d + 4] += k * g[0] * g[2];
                f.drag[d + 5] += k * g[1] * g[2];
            }
        }
        f
    });
    let mut forces = Forces::new(ids.len());
    for part in under.iter().chain(&across) {
        forces.add(part);
    }
    let Forces {
        torque,
        drag,
        pull_sum,
        pull_magnitude,
        pull_spread,
        area,
        centre,
    } = forces;

    let sym = |arr: &[f64], id: usize| -> Sym3 {
        [
            arr[id * 6],
            arr[id * 6 + 1],
            arr[id * 6 + 2],
            arr[id * 6 + 3],
            arr[id * 6 + 4],
            arr[id * 6 + 5],
        ]
    };

    // What is left over as tension: pulls that cancel do not move the plate,
    // they stretch it.
    plates.stress.clear();
    if o.two_sided_tension {
        // A trench along one side of a plate pulls it that way. It is tension
        // only where slabs pull from both sides, so the zigzag of the mesh and
        // the curvature of the sphere no longer read as a plate tearing itself.
        let mut axis = vec![[0.0; 3]; ids.len()];
        let mut at = vec![[0.0; 3]; ids.len()];
        for (p, centre_p) in at.iter_mut().enumerate() {
            axis[p] = dominant_axis(&sym(&pull_spread, p), [1.0, 0.0, 0.0]);
            if area[p] > 0.0 {
                *centre_p = normalise([
                    centre[p * 3] / area[p],
                    centre[p * 3 + 1] / area[p],
                    centre[p * 3 + 2] / area[p],
                ]);
            }
        }
        let (plus, minus) = opposing_sides(mesh, data, &slots, &at, &axis, t);
        for (p, &id) in ids.iter().enumerate() {
            let opposed = 2.0 * plus[p].min(minus[p]);
            if opposed <= 0.0 || area[p] <= 0.0 {
                continue;
            }
            plates.stress.insert(
                id,
                PlateStress {
                    opposed_pull: opposed,
                    axis: axis[p],
                },
            );
        }
    } else {
        for (p, &id) in ids.iter().enumerate() {
            let opposed =
                pull_magnitude[p] - len3(pull_sum[p * 3], pull_sum[p * 3 + 1], pull_sum[p * 3 + 2]);
            if opposed <= 0.0 || area[p] <= 0.0 {
                continue;
            }
            plates.stress.insert(
                id,
                PlateStress {
                    opposed_pull: opposed,
                    axis: dominant_axis(&sym(&pull_spread, p), [1.0, 0.0, 0.0]),
                },
            );
        }
    }

    // Solve the balance for the motion each plate is heading for.
    let mut target = vec![0.0f64; bound * 3];
    for (p, &id) in ids.iter().enumerate() {
        let w = solve_sym3(
            &sym(&drag, p),
            [torque[p * 3], torque[p * 3 + 1], torque[p * 3 + 2]],
        );
        let i = id as usize * 3;
        target[i..i + 3].copy_from_slice(&w);
    }

    // Edge coupling: each plate's target is drawn toward its neighbours', in
    // proportion to how much boundary they share relative to the plate's
    // width. A small plate has a long edge for its area, so it is carried
    // along by whoever surrounds it; a large plate barely notices one small
    // neighbour. The weights are taken from the targets as solved, so the
    // order of the plates does not matter, and they sum to at most 1.
    if t.edge_coupling > 0.0 {
        let mut shared: BTreeMap<i64, f64> = BTreeMap::new();
        for a in 0..mesh.arc_count() {
            let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
            if ca < 0 || cb < 0 {
                continue;
            }
            let (pa, pb) = (data.plate_id[ca as usize], data.plate_id[cb as usize]);
            if pa < 0 || pb < 0 || pa == pb {
                continue;
            }
            *shared.entry(suture_key(pa, pb)).or_insert(0.0) += mesh.arc_length[a] as f64;
        }
        let area_of = |id: i32| slot_of(id).map(|p| area[p]).unwrap_or(0.0);
        let mut weight: BTreeMap<i32, BTreeMap<i32, f64>> = BTreeMap::new();
        for (&key, &len) in &shared {
            let (a, b) = split_key(key);
            for (id, other) in [(a, b), (b, a)] {
                let plate_area = area_of(id);
                if plate_area <= 0.0 {
                    continue;
                }
                let w = clamp01(t.edge_coupling * len / plate_area.sqrt());
                if w > 0.0 {
                    weight.entry(id).or_default().insert(other, w);
                }
            }
        }
        let before = target.clone();
        for (&id, neighbours) in &weight {
            let sum: f64 = neighbours.values().sum();
            let scale_w = if sum > 1.0 { 1.0 / sum } else { 1.0 };
            let p = id as usize * 3;
            for (&other, &w) in neighbours {
                let n = other as usize * 3;
                let w = w * scale_w;
                for k in 0..3 {
                    target[p + k] += w * (before[n + k] - before[p + k]);
                }
            }
        }
    }

    // Suture coupling: welded plates head for a common motion. It acts on the
    // targets, not the motion itself, so it does not compound tick on tick.
    // A craton straddling the weld is one rigid body: the pair is held to a
    // common motion as firmly as cratons are durable.
    let welded = clamp01(t.suture_coupling * dt_myr);
    let rigid = welded.max(clamp01(t.craton_durability));
    if rigid > 0.0 {
        for &key in plates.sutures.keys() {
            let (ia, ib) = split_key(key);
            if !plates.by_id.contains_key(&ia) || !plates.by_id.contains_key(&ib) {
                continue;
            }
            let blend = if plates.craton_welds.contains_key(&key) {
                rigid
            } else {
                welded
            };
            let (a, b) = (ia as usize * 3, ib as usize * 3);
            for k in 0..3 {
                let mean = 0.5 * (target[a + k] + target[b + k]);
                target[a + k] += blend * (mean - target[a + k]);
                target[b + k] += blend * (mean - target[b + k]);
            }
        }
    }

    // Plates settle onto their targets rather than jumping there. A plate of a
    // few cells has a target made of a few cells' forces, which change with
    // every cell the margins move, so the smaller the plate, the longer it
    // averages over: up to `1 + microplate_damping` times the response time
    // for a plate with no area, and none for a plate of `microplate_area`
    // and over. Only the plate's own motion is slowed; its target is not
    // touched, so neighbours coupled to it by sutures still see it as it is.
    let globe: f64 = area.iter().sum();
    let response_of = |id: i32| {
        motion_response(t, dt_myr, slot_of(id).map_or(0.0, |p| area[p]), globe)
    };
    let max_turn = (t.max_turn_rate.to_radians() * dt_myr).max(0.0);
    for (&id, props) in plates.by_id.iter_mut() {
        let p = id as usize * 3;
        let response = response_of(id);
        let mut next = props.omega;
        for k in 0..3 {
            next[k] += response * (target[p + k] - props.omega[k]);
        }
        if max_turn > 0.0 {
            next = limit_turn(props.omega, next, max_turn);
        }
        props.omega = next;
    }
}

/// Share of the way to its target a plate of `plate_area` (on a globe of
/// `globe`) settles in `dt_myr`: `motion_response_time` for a plate of
/// `microplate_area` of the globe or more, up to `1 + microplate_damping`
/// times longer for one of no area.
pub fn motion_response(t: &Tuning, dt_myr: f64, plate_area: f64, globe: f64) -> f64 {
    if t.motion_response_time <= 0.0 {
        return 1.0;
    }
    let small = t.microplate_area * globe;
    let smallness = if small > 0.0 {
        clamp01(1.0 - plate_area / small)
    } else {
        0.0
    };
    let time = t.motion_response_time * (1.0 + t.microplate_damping.max(0.0) * smallness);
    1.0 - (-dt_myr / time).exp()
}

/// `v` with the part along `centre` removed, so it lies in the tangent plane.
fn tangent_component(centre: Vec3, v: Vec3) -> Vec3 {
    let along = dot(v, centre);
    [
        v[0] - along * centre[0],
        v[1] - along * centre[1],
        v[2] - along * centre[2],
    ]
}

/// Pull from slabs on the positive side of each plate's axis, and from slabs
/// on the negative side, each projected into the tangent plane at the plate's
/// centre. A trench that all lies on one side fills only one of the two.
fn opposing_sides(
    mesh: &Mesh,
    data: &CellData,
    slots: &[usize],
    centre: &[Vec3],
    axis: &[Vec3],
    t: &Tuning,
) -> (Vec<f64>, Vec<f64>) {
    let n = centre.len();
    let slot_of = |plate: i32| -> Option<usize> {
        let s = *slots.get(usize::try_from(plate).ok()?)?;
        (s != NO_SLOT).then_some(s)
    };
    let parts = par::chunks(mesh.arc_count(), |arcs| {
        let mut plus = vec![0.0f64; n];
        let mut minus = vec![0.0f64; n];
        for a in arcs {
            let cell_a = mesh.arc_cells[a * 2];
            let cell_b = mesh.arc_cells[a * 2 + 1];
            if cell_a < 0 || cell_b < 0 {
                continue;
            }
            let (cell_a, cell_b) = (cell_a as usize, cell_b as usize);
            let (plate_a, plate_b) = (data.plate_id[cell_a], data.plate_id[cell_b]);
            if plate_a < 0 || plate_b < 0 || plate_a == plate_b {
                continue;
            }
            let Some(sub) = subduction(mesh, data, cell_a, cell_b, t) else {
                continue;
            };
            let (down_cell, down_plate, sign) = if sub.down == cell_b {
                (cell_b, plate_b, -1.0)
            } else {
                (cell_a, plate_a, 1.0)
            };
            let pull = slab_pull(t, column_buoyancy(t, data, down_cell)) * sub.strength;
            let Some(p) = slot_of(down_plate).filter(|_| pull > 0.0) else {
                continue;
            };
            let force = t.slab_weight * pull * mesh.arc_length[a] as f64;
            let normal = [
                mesh.arc_normal[a * 3] as f64,
                mesh.arc_normal[a * 3 + 1] as f64,
                mesh.arc_normal[a * 3 + 2] as f64,
            ];
            let mid = [
                mesh.arc_mid[a * 3] as f64,
                mesh.arc_mid[a * 3 + 1] as f64,
                mesh.arc_mid[a * 3 + 2] as f64,
            ];
            let c = centre[p];
            let axis_t = tangent_component(c, axis[p]);
            let axis_len = len3(axis_t[0], axis_t[1], axis_t[2]);
            if axis_len < 1e-9 {
                continue;
            }
            let axis_hat = scale(axis_t, 1.0 / axis_len);
            let along = dot(tangent_component(c, scale(normal, sign)), axis_hat);
            let side = dot(tangent_component(c, mid), axis_hat);
            let component = along * force;
            if side > 0.0 && component > 0.0 {
                plus[p] += component;
            } else if side < 0.0 && component < 0.0 {
                minus[p] += -component;
            }
        }
        (plus, minus)
    });
    let (mut plus, mut minus) = (vec![0.0; n], vec![0.0; n]);
    for (part_plus, part_minus) in parts {
        for i in 0..n {
            plus[i] += part_plus[i];
            minus[i] += part_minus[i];
        }
    }
    (plus, minus)
}

/// `next`, with its Euler pole turned no more than `max_turn` radians from
/// `now`'s; its rate is kept.
fn limit_turn(now: Vec3, next: Vec3, max_turn: f64) -> Vec3 {
    let (a, b) = (
        len3(now[0], now[1], now[2]),
        len3(next[0], next[1], next[2]),
    );
    if a < 1e-12 || b < 1e-12 {
        return next;
    }
    let cos = (dot(now, next) / (a * b)).clamp(-1.0, 1.0);
    if cos.acos() <= max_turn {
        return next;
    }
    let from = scale(now, 1.0 / a);
    // Toward `next` within the plane the two share; straight back the other
    // way, any perpendicular does.
    let mut toward = sub(scale(next, 1.0 / b), scale(from, cos));
    if len3(toward[0], toward[1], toward[2]) < 1e-9 {
        let any = if from[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        toward = cross(from, any);
    }
    let toward = normalise(toward);
    let (sin, cos) = max_turn.sin_cos();
    scale(
        [
            from[0] * cos + toward[0] * sin,
            from[1] * cos + toward[1] * sin,
            from[2] * cos + toward[2] * sin,
        ],
        b,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle(a: Vec3, b: Vec3) -> f64 {
        let (la, lb) = (len3(a[0], a[1], a[2]), len3(b[0], b[1], b[2]));
        (dot(a, b) / (la * lb)).clamp(-1.0, 1.0).acos()
    }

    #[test]
    fn small_plates_settle_onto_their_targets_more_slowly() {
        let t = Tuning {
            motion_response_time: 40.0,
            microplate_area: 0.01,
            microplate_damping: 4.0,
            ..Tuning::default()
        };
        let globe = 4.0 * std::f64::consts::PI;
        let at = |share: f64| motion_response(&t, 2.0, share * globe, globe);
        let plain = 1.0 - (-2.0f64 / 40.0).exp();
        assert!((at(0.01) - plain).abs() < 1e-12, "a plate at the limit is damped");
        assert!((at(0.3) - plain).abs() < 1e-12, "a large plate is damped");
        let slowest = 1.0 - (-2.0f64 / 200.0).exp();
        assert!((at(0.0) - slowest).abs() < 1e-12);
        assert!(at(0.0) < at(0.002) && at(0.002) < at(0.008) && at(0.008) < plain);
        let off = Tuning {
            microplate_damping: 0.0,
            ..t.clone()
        };
        assert!((motion_response(&off, 2.0, 0.0, globe) - plain).abs() < 1e-12);
    }

    #[test]
    fn a_sharp_turn_is_held_to_the_limit_at_the_new_rate() {
        let now = [0.0, 0.0, 0.01];
        let next = [0.02, 0.0, 0.0];
        let limit = 5f64.to_radians();
        let turned = limit_turn(now, next, limit);
        assert!((angle(now, turned) - limit).abs() < 1e-9);
        assert!((len3(turned[0], turned[1], turned[2]) - 0.02).abs() < 1e-12);
        assert!(
            angle(turned, next) < angle(now, next),
            "turned the wrong way"
        );
    }

    #[test]
    fn a_gentle_turn_or_reversal_is_handled() {
        let now = [0.0, 0.0, 0.01];
        let gentle = [0.0001, 0.0, 0.01];
        assert_eq!(limit_turn(now, gentle, 0.1), gentle);
        let reversed = limit_turn(now, [0.0, 0.0, -0.01], 0.1);
        assert!((angle(now, reversed) - 0.1).abs() < 1e-9);
    }

    use crate::boundaries::BoundaryField;
    use crate::cell_data::{CONTINENTAL, CellData, OCEANIC};
    use crate::mantle::MantleFlow;
    use crate::mesh::Mesh;
    use crate::plates::PlateProps;
    use crate::scratch::Scratch;
    use crate::sutures::update_sutures;
    use crate::tuning::SimOptions;

    fn props(omega: Vec3) -> PlateProps {
        PlateProps {
            omega,
            pending: 0.0,
            steps: 0,
        }
    }

    fn heavy_ocean(t: &Tuning, data: &mut CellData, i: usize) {
        data.crust_type[i] = OCEANIC;
        data.age[i] = t.max_oceanic_age as f32;
        data.density[i] = t.oceanic_density as f32;
        data.thickness[i] = t.oceanic_thickness as f32;
        data.lithosphere[i] = t.plate_thickness as f32;
    }

    fn continent(t: &Tuning, data: &mut CellData, i: usize, slab: f32) {
        data.crust_type[i] = CONTINENTAL;
        data.density[i] = t.continental_density as f32;
        data.thickness[i] = 35.0;
        data.slab[i] = slab;
    }

    fn tension(mesh: &Mesh, data: &CellData, plates: &PlateData, on: bool) -> f64 {
        let mut plates = plates_copy(plates);
        let boundaries = BoundaryField::new(mesh.cell_count, mesh.arc_count());
        let mantle = MantleFlow::new(mesh);
        let mut scratch = Scratch::new(mesh.cell_count);
        let t = Tuning {
            mantle_flow_coupling: 0.0,
            ridge_weight: 0.0,
            edge_coupling: 0.0,
            ..Tuning::default()
        };
        let o = SimOptions {
            two_sided_tension: on,
            ..SimOptions::default()
        };
        update_plate_motion(
            mesh,
            data,
            &mut plates,
            &boundaries,
            &mantle,
            &mut scratch,
            &t,
            t.dt_myr,
            &o,
        );
        plates.stress.get(&0).map(|s| s.opposed_pull).unwrap_or(0.0)
    }

    /// PlateData has no Clone; copy the motions the tests set up.
    fn plates_copy(plates: &PlateData) -> PlateData {
        let mut out = PlateData::default();
        for (&id, p) in &plates.by_id {
            out.by_id.insert(id, props(p.omega));
        }
        out
    }

    #[test]
    fn a_trench_along_one_side_is_not_tension() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning::default();
        let mut data = CellData::new(n);
        for i in 0..n {
            let (x, z) = (mesh.centres[i * 3], mesh.centres[i * 3 + 2]);
            if z > 0.2 {
                data.plate_id[i] = 0;
                heavy_ocean(&t, &mut data, i);
            } else if x > 0.0 {
                data.plate_id[i] = 1;
                continent(&t, &mut data, i, t.slab_full_length as f32);
            } else {
                data.plate_id[i] = 2;
                data.crust_type[i] = OCEANIC;
                data.density[i] = t.oceanic_density as f32;
                data.thickness[i] = t.oceanic_thickness as f32;
            }
        }
        let mut plates = PlateData::default();
        for id in 0..3 {
            plates.by_id.insert(id, props([0.0; 3]));
        }
        let one_side = tension(&mesh, &data, &plates, true);
        let artefact = tension(&mesh, &data, &plates, false);
        assert_eq!(
            one_side, 0.0,
            "a one-sided trench read as tension {one_side}"
        );
        assert!(
            artefact > 0.0,
            "the old measure found no pull to compare against"
        );
    }

    #[test]
    fn trenches_on_opposite_sides_tension_the_weaker_pull() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning::default();
        // A northern cap pulled by a trench on its +x side and, optionally, on
        // its -x side. One side of a cap stays on one side of the cap's centre,
        // which a trench wrapped all the way round it would not.
        let world = |west_slab: f32| {
            let mut data = CellData::new(n);
            for i in 0..n {
                let (x, z) = (mesh.centres[i * 3], mesh.centres[i * 3 + 2]);
                if z > 0.2 {
                    data.plate_id[i] = 0;
                    heavy_ocean(&t, &mut data, i);
                } else if x > 0.0 {
                    data.plate_id[i] = 1;
                    continent(&t, &mut data, i, t.slab_full_length as f32);
                } else {
                    data.plate_id[i] = 2;
                    continent(&t, &mut data, i, west_slab);
                }
            }
            data
        };
        let mut plates = PlateData::default();
        for id in 0..3 {
            plates.by_id.insert(id, props([0.0; 3]));
        }
        let both = tension(&mesh, &world(t.slab_full_length as f32), &plates, true);
        let half = tension(
            &mesh,
            &world(0.5 * t.slab_full_length as f32),
            &plates,
            true,
        );
        let one = tension(&mesh, &world(0.0), &plates, true);
        assert!(both > 0.0, "opposite trenches produced no tension");
        assert_eq!(one, 0.0, "dropping one trench left tension {one}");
        // Halving the weaker side halves the tension, which is twice that side.
        assert!(
            (half - 0.5 * both).abs() < 0.15 * both,
            "half {half}, both {both}"
        );
        assert!(half > 0.0 && half < both, "half {half}, both {both}");
    }

    fn step_motion(
        mesh: &Mesh,
        data: &mut CellData,
        plates: &mut PlateData,
        t: &Tuning,
        o: &SimOptions,
    ) {
        let boundaries = BoundaryField::new(mesh.cell_count, mesh.arc_count());
        let mantle = MantleFlow::new(mesh);
        let mut scratch = Scratch::new(mesh.cell_count);
        update_plate_motion(
            mesh,
            data,
            plates,
            &boundaries,
            &mantle,
            &mut scratch,
            t,
            t.dt_myr,
            o,
        );
        update_sutures(
            mesh,
            data,
            plates,
            &boundaries,
            t.dt_myr,
            &mut scratch,
            t,
            o,
        );
    }

    #[test]
    fn a_small_plate_is_captured_by_the_plate_around_it() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning {
            mantle_flow_coupling: 0.0,
            ridge_weight: 0.0,
            mantle_drag: 2.0,
            // Strong enough that the patch, whose edge is long for its width,
            // is carried by the plate around it. Weak enough that the same
            // plate is not dragged to a stop by the short trench that pulls it.
            edge_coupling: 0.2,
            suture_coupling: 0.0,
            craton_merge_time: 0.0,
            boundary_death_time: 24.0,
            motion_response_time: 4.0,
            merge_quiet_speed: 0.005,
            ..Tuning::default()
        };
        let o = SimOptions {
            quiet_by_mean_speed: true,
            two_sided_tension: true,
            ..SimOptions::default()
        };

        // A small western cap with a slab on only its northern rim, so the
        // pull does not cancel and the ocean plate has a motion of its own.
        // The cap is small, so the ocean plate barely couples to it. A patch
        // at the opposite pole shares edges only with that ocean plate.
        let west = (0..n)
            .min_by(|&a, &b| {
                mesh.centres[a * 3]
                    .total_cmp(&mesh.centres[b * 3])
                    .then(a.cmp(&b))
            })
            .unwrap();
        let mut cap = vec![west];
        let mut seen = vec![false; n];
        seen[west] = true;
        let mut head = 0;
        while cap.len() < 24 && head < cap.len() {
            let cell = cap[head];
            head += 1;
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if !seen[m] && cap.len() < 24 {
                    seen[m] = true;
                    cap.push(m);
                }
            }
        }
        let east = (0..n)
            .max_by(|&a, &b| {
                mesh.centres[a * 3]
                    .total_cmp(&mesh.centres[b * 3])
                    .then(b.cmp(&a))
            })
            .unwrap();
        let mut patch = vec![east];
        let mut seen = vec![false; n];
        seen[east] = true;
        let mut head = 0;
        while patch.len() < 12 && head < patch.len() {
            let cell = patch[head];
            head += 1;
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if !seen[m] && patch.len() < 12 {
                    seen[m] = true;
                    patch.push(m);
                }
            }
        }
        let mut data = CellData::new(n);
        for i in 0..n {
            data.plate_id[i] = 0;
            heavy_ocean(&t, &mut data, i);
        }
        for &i in &cap {
            let z = mesh.centres[i * 3 + 2];
            data.plate_id[i] = 2;
            let slab = if z > 0.0 {
                t.slab_full_length as f32
            } else {
                0.0
            };
            continent(&t, &mut data, i, slab);
        }
        for &i in &patch {
            data.plate_id[i] = 1;
            heavy_ocean(&t, &mut data, i);
        }

        let mut plates = PlateData::default();
        plates.by_id.insert(0, props([0.0; 3]));
        plates.by_id.insert(1, props([0.02, 0.0, 0.0]));
        plates.by_id.insert(2, props([0.0; 3]));
        let mut still = plates_copy(&plates);
        let mut data_free = data.clone();
        let mut t_free = t.clone();
        t_free.edge_coupling = 0.0;
        for _ in 0..40 {
            step_motion(&mesh, &mut data, &mut plates, &t, &o);
            step_motion(&mesh, &mut data_free, &mut still, &t_free, &o);
        }
        assert!(
            !plates.by_id.contains_key(&1),
            "the small plate was not captured"
        );
        assert!(plates.quiet_merges >= 1);
        assert!(
            still.by_id.contains_key(&1),
            "it merged with no edge coupling"
        );
    }

    #[test]
    fn two_large_plates_pulled_apart_are_not_held_together() {
        let mesh = Mesh::icosphere(4);
        let n = mesh.cell_count;
        let t = Tuning {
            mantle_flow_coupling: 0.0,
            ridge_weight: 0.0,
            mantle_drag: 2.0,
            // The same coupling that captures a small plate.
            edge_coupling: 0.2,
            suture_coupling: 0.0,
            craton_merge_time: 0.0,
            // Long enough that merely being slow at the start does not merge
            // them. What would merge them is the coupling dragging their
            // motions together.
            boundary_death_time: 1.0e6,
            motion_response_time: 4.0,
            ..Tuning::default()
        };
        let o = SimOptions::default();
        // Plate 0 is pulled by a one-sided trench. Plate 1, the other
        // hemisphere, has none, so the two motions stay apart. The boundary
        // they share is long, but each is wide, so the default coupling only
        // nudges them.
        let mut data = CellData::new(n);
        for i in 0..n {
            let (x, z) = (mesh.centres[i * 3], mesh.centres[i * 3 + 2]);
            if x < -0.55 {
                data.plate_id[i] = 2;
                let slab = if z > 0.0 {
                    t.slab_full_length as f32
                } else {
                    0.0
                };
                continent(&t, &mut data, i, slab);
            } else if x < 0.0 {
                data.plate_id[i] = 0;
                heavy_ocean(&t, &mut data, i);
            } else {
                data.plate_id[i] = 1;
                heavy_ocean(&t, &mut data, i);
            }
        }
        let mut plates = PlateData::default();
        for id in 0..3 {
            plates.by_id.insert(id, props([0.0; 3]));
        }
        for _ in 0..20 {
            step_motion(&mesh, &mut data, &mut plates, &t, &o);
        }
        assert!(
            plates.by_id.contains_key(&0) && plates.by_id.contains_key(&1),
            "a large plate was absorbed"
        );
        let (a, b) = (plates.by_id[&0].omega, plates.by_id[&1].omega);
        let apart = len3(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        let speed = len3(a[0], a[1], a[2]).max(len3(b[0], b[1], b[2]));
        assert!(
            apart > 0.5 * speed && apart > t.merge_quiet_speed,
            "the two plates were held to one motion: apart {apart}, speed {speed}"
        );
    }
}
