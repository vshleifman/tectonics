//! Cratons: the ancient cores continents are built around.
//!
//! Continental crust that sits long enough in the quiet interior of a
//! continent, thick, unstretched and well inland of the ocean, cools and drains its mantle
//! root into a keel some 200 km deep: cold, buoyant and stiff. That keel is
//! what makes a craton. Rifts go around one rather than through it (East
//! Africa parts either side of the Tanzanian craton), trenches cannot scrape
//! it, and it couples to the mantle flowing beneath far more strongly than
//! thinner lithosphere does. So cratons outlast every supercontinent they are
//! part of, and continents break up and reassemble around the same cores,
//! growing by the younger belts accreted against them, as North America has
//! around the Superior, Slave and Wyoming cratons.
//!
//! The keel grows over `craton_growth_time` where the crust qualifies, and is
//! eroded over `craton_decay_time` where it is rifted, weakened or thinned, as
//! North China's root was in the last 200 Myr. What it does is up to the
//! passes that read it: rifting (strength), plate forces (drag and mantle
//! flow), advection (margin erosion) and recycling.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::scratch::Scratch;
use crate::tuning::Tuning;

/// Weakness above which crust counts as being deformed right now, and its keel
/// erodes: the seam of a suture still in contact (stamped at full weakness),
/// not the fading scar of one long closed.
const DEFORMED: f32 = 0.95;
/// Weakness below which crust can grow a keel. Weakness fades over billions of
/// years, and nearly all continental crust carries some from a suture or rift
/// it has been through; cratons are the cores that stopped being deformed, not
/// ones that never were. Only crust in or fresh out of a suture is barred.
const QUIET: f32 = 0.9;

pub fn update_cratons(mesh: &Mesh, data: &mut CellData, dt_myr: f64, s: &mut Scratch, t: &Tuning) {
    let n = mesh.cell_count;
    // Rings in from the nearest ocean, by a breadth-first search out from it
    // all at once. A plate boundary through a continent does not count: a
    // craton may well lie against a suture, which is held off by its weakness.
    let hops = &mut s.i32a;
    let queue = &mut s.queue;
    let mut tail = 0usize;
    for i in 0..n {
        if data.crust_type[i] != CONTINENTAL {
            hops[i] = 0;
            queue[tail] = i as i32;
            tail += 1;
        } else {
            hops[i] = -1;
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
    let grow = (dt_myr / t.craton_growth_time.max(1e-6)) as f32;
    let erode = (dt_myr / t.craton_decay_time.max(1e-6)) as f32;
    // A craton's crust is of ordinary continental thickness or more; crust
    // that has only just crossed the continental threshold has no root.
    let thick_enough = 2.0 * t.continental_thickness_threshold;
    for i in 0..n {
        if data.crust_type[i] != CONTINENTAL {
            data.keel[i] = 0.0;
            continue;
        }
        let h = data.thickness[i] as f64;
        let weak = data.weakness[i];
        if data.rift_side[i] != 0 || weak >= DEFORMED || h < t.continental_thickness_threshold {
            data.keel[i] = (data.keel[i] - erode).max(0.0);
            continue;
        }
        // Never the coastal cell itself, however coarse the mesh.
        let inland = hops[i] < 0 || (hops[i] > 1 && hops[i] as f64 * spacing >= t.craton_inland);
        if inland && weak < QUIET && h >= thick_enough && data.age[i] as f64 >= t.craton_min_age {
            data.keel[i] = (data.keel[i] + grow).min(1.0);
        }
    }
}
