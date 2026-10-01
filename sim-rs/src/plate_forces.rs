//! Each plate's angular velocity from a force balance, mirroring
//! `plateForces.ts`: slab pull and ridge push drive, mantle drag and collision
//! resistance resist, and the balance `M omega = torque` is solved directly.

use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{Sym3, clamp01, dominant_axis, len3, solve_sym3};
use crate::mesh::Mesh;
use crate::plates::{PlateData, PlateStress, split_key};
use crate::scratch::Scratch;
use crate::tuning::Tuning;

/// Pull per unit length of trench from crust of the given age, in [0, 1].
fn slab_pull(t: &Tuning, age: f64) -> f64 {
    let onset = t.slab_pull_onset_age;
    let span = (t.slab_pull_saturation_age - onset).max(1e-6);
    clamp01((age - onset) / span)
}

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
            continue;
        }

        // Slab pull, not gated on the boundary converging right now: a slab's
        // weight is there whether or not the plates happen to be moving.
        if !(cont_a && cont_b) {
            let b_down = cont_a || (!cont_b && data.density[cell_b] > data.density[cell_a]);
            let (down_cell, down_plate, sign) = if b_down {
                (cell_b, plate_b, -1.0)
            } else {
                (cell_a, plate_a, 1.0)
            };
            let pull = slab_pull(t, data.age[down_cell] as f64);
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
                tension: opposed / area[p],
                axis: dominant_axis(&sym(&pull_spread, p), [1.0, 0.0, 0.0]),
            },
        );
    }

    // Solve the balance.
    for (&id, props) in plates.by_id.iter_mut() {
        let p = id as usize;
        props.omega = solve_sym3(
            &sym(&drag, p),
            [torque[p * 3], torque[p * 3 + 1], torque[p * 3 + 2]],
        );
    }

    // Suture coupling: welded plates converge onto a common motion.
    let blend = clamp01(t.suture_coupling * dt_myr);
    if blend > 0.0 {
        let keys: Vec<i64> = plates.sutures.keys().copied().collect();
        for key in keys {
            let (ia, ib) = split_key(key);
            let (Some(a), Some(b)) = (
                plates.by_id.get(&ia).map(|p| p.omega),
                plates.by_id.get(&ib).map(|p| p.omega),
            ) else {
                continue;
            };
            let mut na = a;
            let mut nb = b;
            for k in 0..3 {
                let mean = 0.5 * (a[k] + b[k]);
                na[k] += blend * (mean - a[k]);
                nb[k] += blend * (mean - b[k]);
            }
            if let Some(p) = plates.by_id.get_mut(&ia) {
                p.omega = na;
            }
            if let Some(p) = plates.by_id.get_mut(&ib) {
                p.omega = nb;
            }
        }
    }
}
