//! Each plate's angular velocity from a force balance, mirroring
//! `plateForces.ts`: slab pull and ridge push drive, mantle drag and collision
//! resistance resist, and the balance `M omega = torque` gives the motion each
//! plate is heading for. Plates settle onto it over `motion_response_time`
//! rather than jumping straight there.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{Sym3, clamp01, dominant_axis, len3, solve_sym3};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateStress, split_key, suture_key};
use crate::scratch::Scratch;
use crate::slabs::{slab_pull, subduction};
use crate::tuning::Tuning;

pub fn update_plate_motion(
    mesh: &Mesh,
    data: &CellData,
    plates: &mut PlateData,
    boundaries: &BoundaryField,
    scratch: &mut Scratch,
    t: &Tuning,
    dt_myr: f64,
) {
    let bound = scratch.pack_plates(plates);
    let live = |p: i32| scratch.is_live(p);
    let mut torque = vec![0.0f64; bound * 3];
    let mut drag = vec![0.0f64; bound * 6];
    let mut pull_sum = vec![0.0f64; bound * 3];
    let mut pull_magnitude = vec![0.0f64; bound];
    let mut pull_spread = vec![0.0f64; bound * 6];
    let mut area = vec![0.0f64; bound];
    let c = &mesh.centres;

    // Resistance under the plate: every cell drags on the mantle beneath it,
    // continental keels harder.
    for i in 0..mesh.cell_count {
        let plate = data.plate_id[i];
        if !live(plate) {
            continue;
        }
        let p = plate as usize;
        let keel = if data.crust_type[i] == CONTINENTAL {
            t.continental_drag
        } else {
            1.0
        };
        let k = t.mantle_drag * mesh.area(i) * keel;
        let (x, y, z) = (c[i * 3] as f64, c[i * 3 + 1] as f64, c[i * 3 + 2] as f64);
        let m = p * 6;
        drag[m] += k * (1.0 - x * x);
        drag[m + 1] += k * (1.0 - y * y);
        drag[m + 2] += k * (1.0 - z * z);
        drag[m + 3] -= k * x * y;
        drag[m + 4] -= k * x * z;
        drag[m + 5] -= k * y * z;
        area[p] += mesh.area(i);
    }

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
        if !live(plate) {
            return;
        }
        let m = mid_of(a);
        let n = normal_of(a);
        let f = [n[0] * sign * mag, n[1] * sign * mag, n[2] * sign * mag];
        let p = plate as usize * 3;
        torque[p] += m[1] * f[2] - m[2] * f[1];
        torque[p + 1] += m[2] * f[0] - m[0] * f[2];
        torque[p + 2] += m[0] * f[1] - m[1] * f[0];
    };

    for a in 0..mesh.arc_count() {
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

        // Dome push: lithosphere raised over trapped mantle heat slides off
        // it, so a margin over a hot dome pushes both sides apart. This is what
        // separates the halves of a plate that rifted over heat alone, with no
        // slab to pull them. A weld holds against it. The dome has to straddle
        // the margin, so the cooler side sets the push.
        let heat = data.mantle_heat[cell_a].min(data.mantle_heat[cell_b]) as f64;
        if heat > 0.0 && !plates.sutures.contains_key(&suture_key(plate_a, plate_b)) {
            let push = t.dome_push * heat * arc_len;
            add_torque(&mut torque, plate_a, a, -1.0, push);
            add_torque(&mut torque, plate_b, a, 1.0, push);
        }

        if kind == DIVERGENT {
            // Ridge push: each side slides off the ridge, away from the other,
            // driven by how far it has subsided below the crest.
            let relief_a = t.ridge_elevation - data.elevation[cell_a] as f64;
            if relief_a > 0.0 {
                add_torque(
                    &mut torque,
                    plate_a,
                    a,
                    -1.0,
                    t.ridge_weight * relief_a * arc_len,
                );
            }
            let relief_b = t.ridge_elevation - data.elevation[cell_b] as f64;
            if relief_b > 0.0 {
                add_torque(
                    &mut torque,
                    plate_b,
                    a,
                    1.0,
                    t.ridge_weight * relief_b * arc_len,
                );
            }
        }

        // Slab pull, not gated on the boundary converging right now: a slab's
        // weight is there whether or not the plates happen to be moving. But it
        // takes a slab, so only where crust has actually gone down.
        if let Some(sub) = subduction(mesh, data, cell_a, cell_b, t) {
            let (down_cell, down_plate, sign) = if sub.down == cell_b {
                (cell_b, plate_b, -1.0)
            } else {
                (cell_a, plate_a, 1.0)
            };
            let pull = slab_pull(t, data.age[down_cell] as f64) * sub.strength;
            if pull > 0.0 && live(down_plate) {
                let force = t.slab_weight * pull * arc_len;
                add_torque(&mut torque, down_plate, a, sign, force);
                let n = normal_of(a);
                let d = [n[0] * sign, n[1] * sign, n[2] * sign];
                let p = down_plate as usize;
                pull_sum[p * 3] += d[0] * force;
                pull_sum[p * 3 + 1] += d[1] * force;
                pull_sum[p * 3 + 2] += d[2] * force;
                pull_magnitude[p] += force;
                let s = p * 6;
                pull_spread[s] += force * d[0] * d[0];
                pull_spread[s + 1] += force * d[1] * d[1];
                pull_spread[s + 2] += force * d[2] * d[2];
                pull_spread[s + 3] += force * d[0] * d[1];
                pull_spread[s + 4] += force * d[0] * d[2];
                pull_spread[s + 5] += force * d[1] * d[2];
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
            if !live(plate) {
                continue;
            }
            let d = plate as usize * 6;
            drag[d] += k * g[0] * g[0];
            drag[d + 1] += k * g[1] * g[1];
            drag[d + 2] += k * g[2] * g[2];
            drag[d + 3] += k * g[0] * g[1];
            drag[d + 4] += k * g[0] * g[2];
            drag[d + 5] += k * g[1] * g[2];
        }
    }

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
    let ids: Vec<i32> = plates.by_id.keys().copied().collect();
    for &id in &ids {
        let p = id as usize;
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

    // Solve the balance for the motion each plate is heading for.
    let mut target = vec![0.0f64; bound * 3];
    for &id in &ids {
        let p = id as usize;
        let w = solve_sym3(
            &sym(&drag, p),
            [torque[p * 3], torque[p * 3 + 1], torque[p * 3 + 2]],
        );
        target[p * 3..p * 3 + 3].copy_from_slice(&w);
    }

    // Suture coupling: welded plates head for a common motion. It acts on the
    // targets, not the motion itself, so it does not compound tick on tick.
    let blend = clamp01(t.suture_coupling * dt_myr);
    if blend > 0.0 {
        for &key in plates.sutures.keys() {
            let (ia, ib) = split_key(key);
            if !plates.by_id.contains_key(&ia) || !plates.by_id.contains_key(&ib) {
                continue;
            }
            let (a, b) = (ia as usize * 3, ib as usize * 3);
            for k in 0..3 {
                let mean = 0.5 * (target[a + k] + target[b + k]);
                target[a + k] += blend * (mean - target[a + k]);
                target[b + k] += blend * (mean - target[b + k]);
            }
        }
    }

    // Plates settle onto their targets rather than jumping there.
    let response = if t.motion_response_time > 0.0 {
        1.0 - (-dt_myr / t.motion_response_time).exp()
    } else {
        1.0
    };
    for (&id, props) in plates.by_id.iter_mut() {
        let p = id as usize * 3;
        for k in 0..3 {
            props.omega[k] += response * (target[p + k] - props.omega[k]);
        }
    }
}
