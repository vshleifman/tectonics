//! Cratons: the ancient cores continents are built around.
//!
//! Continental crust that sits long enough in the quiet interior of a
//! continent, thick, unstretched and well inland of the ocean, drains its
//! mantle root into a keel some 200 km deep: cold, buoyant and stiff. That keel
//! is what makes a craton. Rifts go around one rather than through it (East
//! Africa parts either side of the Tanzanian craton), trenches cannot scrape
//! it, and it couples to the mantle flowing beneath far more strongly than
//! thinner lithosphere does. So cratons outlast every supercontinent they are
//! part of, and continents break up and reassemble around the same cores,
//! growing by the younger belts accreted against them, as North America has
//! around the Superior, Slave and Wyoming cratons.
//!
//! A keel is the depleted residue of heavy melting, which only a hotter mantle
//! than today's could manage: nearly every craton is Archean or
//! Palaeoproterozoic, and younger interiors, however quiet, do not grow one.
//! So the keel grows over `craton_growth_time` scaled by the primordial
//! mantle's excess heat (see `primordial.rs`), and not at all once it has
//! cooled or with the era off. Inherited weakness slows it too: the mobile
//! belts between cratons stay weak and keel-less for billions of years, and
//! are where the supercontinents later part.
//!
//! Until it is deep enough to count as craton, a keel is eroded over
//! `craton_decay_time` where it is rifted or caught in a collision. After
//! that, rifts go round it and it wins every collision and weld (see
//! `rifting.rs` and `advect.rs`), so only thinning the crust away beneath it,
//! as North China's root was in the last 200 Myr, can take it. What it does
//! is up to the passes that read it: rifting (strength and the path a rift
//! takes), plate forces (drag and mantle flow), advection (collisions and
//! margin erosion) and recycling.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::mesh::Mesh;
use crate::par;
use crate::primordial::mantle_excess;
use crate::scratch::Scratch;
use crate::tuning::Tuning;

/// Weakness above which crust counts as being deformed right now, and its keel
/// erodes: the seam of a suture still in contact (stamped at full weakness),
/// not the fading scar of one long closed.
const DEFORMED: f32 = 0.95;
/// Weakness below which crust can grow a keel at all. Below it, growth still
/// slows with whatever weakness is left.
const QUIET: f32 = 0.9;
/// Keel above which a collision cannot wear the craton down.
const RIGID_KEEL: f32 = 0.5;

pub fn update_cratons(
    mesh: &Mesh,
    data: &mut CellData,
    dt_myr: f64,
    elapsed: f64,
    s: &mut Scratch,
    t: &Tuning,
) {
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
    let grow = (dt_myr * mantle_excess(t, elapsed) / t.craton_growth_time.max(1e-6)) as f32;
    let erode = (dt_myr / t.craton_decay_time.max(1e-6)) as f32;
    // A craton's crust is of ordinary continental thickness or more; crust
    // that has only just crossed the continental threshold has no root.
    let thick_enough = 2.0 * t.continental_thickness_threshold;
    let hops = &*hops;
    let (crust_type, thickness, weakness) = (&data.crust_type, &data.thickness, &data.weakness);
    let (rift_side, age) = (&data.rift_side, &data.age);
    par::for_each_mut(&mut data.keel, |i, keel| {
        if crust_type[i] != CONTINENTAL {
            *keel = 0.0;
            return;
        }
        let h = thickness[i] as f64;
        let weak = weakness[i];
        // A well-rooted craton is rigid: a collision against it deforms the
        // younger crust around it, not its core, which is why orogens wrap
        // cratons rather than cut through them. Only rifting and thinning
        // reach the keel itself.
        let crushed = weak >= DEFORMED && *keel < RIGID_KEEL;
        // Every cell of a rifting plate carries the side it is on; only the
        // rift itself, where the sides meet or the new ocean has opened,
        // is being stretched. Rifts skirt cratons (see `rifting.rs`), so it
        // is only a keel still too shallow to be one that a rift can wear.
        let side = rift_side[i];
        let rifted = side != 0
            && *keel < RIGID_KEEL
            && ((hops[i] >= 0 && hops[i] as f64 * spacing < t.craton_inland)
                || mesh.nbrs(i).any(|k| {
                    let m = rift_side[mesh.nbr[k] as usize];
                    m != 0 && m != side
                }));
        if rifted || crushed || h < t.continental_thickness_threshold {
            let wear = (erode as f64 * t.craton_wear(*keel)) as f32;
            *keel = (*keel - wear).max(0.0);
            return;
        }
        if grow <= 0.0 {
            return;
        }
        // Never the coastal cell itself, however coarse the mesh.
        let inland = hops[i] < 0 || (hops[i] > 1 && hops[i] as f64 * spacing >= t.craton_inland);
        if inland && weak < QUIET && h >= thick_enough && age[i] as f64 >= t.craton_min_age {
            *keel = (*keel + grow * (1.0 - weak)).min(1.0);
        }
    });
}
