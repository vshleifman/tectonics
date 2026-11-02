//! Sculpting: a brush that raises or lowers the land under it by thickening
//! or thinning the crust there. Elevation is not written: it stays what the
//! crust's buoyancy makes it, so the change rides with the plate, erodes and
//! takes part in everything else the crust does.
//!
//! Each cell under the brush is asked to rise (or sink) by a share of the
//! stroke's height, and takes whatever thickness of crust that needs given its
//! own column: more for a submerged or dense column than for dry continent.
//! The share fades from the centre to the edge, and is broken up by a noise
//! pattern fixed for the whole press, so holding the brush grows the same
//! lumps (or, raising, ridges) taller rather than averaging out to a dome.
//! The noise also pushes the edge in and out, so the footprint is ragged.
//!
//! Thickened continent keeps the makeup it had. Ocean floor built up is a
//! plateau, which in this world only plumes build, so it is made as plume
//! crust is and counted as plume crust on the next tick: thick enough, it
//! turns continental as a plume's plateau does, rather than foundering as
//! ocean floor thickened any other way would in a hot mantle. The count is
//! kept per cell, not carried with the crust, so at the brush's edge it can
//! land a cell off if the plates move before it is read.

use crate::cell_data::{CONTINENTAL, CellData};
use crate::elevation::column_height;
use crate::math::{Vec3, dot, read3};
use crate::mesh::Mesh;
use crate::primordial::mix_in_plume_crust;
use crate::tuning::Tuning;

/// Thinnest crust the brush will leave: thinning stops there rather than
/// wearing a hole in the plate.
const MIN_THICKNESS: f64 = 3.5;
/// Lumps of noise across the brush's radius.
const FEATURES: f64 = 3.0;
/// Octaves of noise, each twice as fine and half as strong as the last.
const OCTAVES: usize = 4;
/// How far, as a share of the radius, the noise moves the edge in or out.
const EDGE_WARP: f64 = 0.35;
/// Thickness step (km) the slope of a column's height is read over.
const PROBE: f64 = 0.1;
/// Mean and standard deviation of [`fbm`], plain and ridged, as measured over
/// many points. Summing octaves draws the noise in toward its mean, so it is
/// spread back out from these to fill its range.
const PLAIN_SPREAD: (f64, f64) = (0.50, 0.115);
const RIDGED_SPREAD: (f64, f64) = (0.53, 0.166);
/// Standard deviation the noise is spread out to.
const SPREAD: f64 = 0.25;

#[derive(Clone, Copy, Debug)]
pub struct Sculpt {
    /// Unit vector to the brush's centre.
    pub centre: Vec3,
    /// Angular distance (radians) the brush reaches.
    pub radius: f64,
    /// How much the noise breaks up the stroke: 0 for an even dome, 1 for
    /// lumps (or ridges) from nothing to twice the mean.
    pub noise: f64,
    /// The press's noise pattern.
    pub seed: u32,
    /// Ridged noise, which makes crests, rather than rounded lumps.
    pub ridged: bool,
}

/// Raise every cell under the brush by up to `height` km at its centre (lower
/// it if negative), by changing the thickness of its crust, and add the ocean
/// floor it builds up to `sculpt_in`. The crust mass (area x km) this adds,
/// negative if it takes some away.
#[allow(clippy::too_many_arguments)]
pub fn sculpt(
    mesh: &Mesh,
    data: &mut CellData,
    sculpt_in: &mut [f32],
    dyn_topo: &[f32],
    sea: f64,
    t: &Tuning,
    brush: &Sculpt,
    height: f64,
) -> f64 {
    if brush.radius <= 0.0 || height == 0.0 {
        return 0.0;
    }
    let p = brush.centre;
    let reach = (brush.radius * (1.0 + EDGE_WARP)).min(std::f64::consts::PI).cos();
    let scale = FEATURES / brush.radius;
    let mut mass = 0.0;
    for i in 0..mesh.cell_count {
        if data.plate_id[i] < 0 {
            continue;
        }
        let x = read3(&mesh.centres, i);
        let along = dot(x, p);
        if along <= reach {
            continue;
        }
        let q = [x[0] * scale, x[1] * scale, x[2] * scale];
        let ragged = spread(fbm(q, brush.seed ^ 0x9e37, false), false);
        let edge = 1.0 + EDGE_WARP * brush.noise * (2.0 * ragged - 1.0);
        let r = along.clamp(-1.0, 1.0).acos() / (brush.radius * edge);
        if r >= 1.0 {
            continue;
        }
        let fade = (1.0 - r * r).powi(2);
        let lumps = spread(fbm(q, brush.seed, brush.ridged), brush.ridged);
        let share = fade * ((1.0 - brush.noise) + brush.noise * 2.0 * lumps);
        let rise = height * share;

        let h = data.thickness[i] as f64;
        let column = |thickness: f64| {
            column_height(
                t,
                thickness,
                data.density[i] as f64,
                data.lithosphere[i] as f64,
                data.keel[i] as f64,
                dyn_topo[i] as f64,
                sea,
            )
        };
        let slope = (column(h + PROBE) - column(h)) / PROBE;
        if slope <= 1e-6 {
            continue;
        }
        let want = h + rise / slope;
        // Never past the limits, but a column already beyond one is left be
        // rather than snapped back to it.
        let next = if rise > 0.0 {
            want.min(t.max_crust_thickness.max(h))
        } else {
            want.max(MIN_THICKNESS.min(h))
        };
        data.thickness[i] = next as f32;
        let added = next - h;
        if added > 0.0 && data.crust_type[i] != CONTINENTAL {
            mix_in_plume_crust(data, i, h, added);
            sculpt_in[i] += added as f32;
        }
        mass += mesh.area(i) * added;
    }
    mass
}

/// Noise from [`fbm`] spread out about 0.5 to fill [0, 1].
fn spread(n: f64, ridged: bool) -> f64 {
    let (mean, sd) = if ridged { RIDGED_SPREAD } else { PLAIN_SPREAD };
    (0.5 + (n - mean) * SPREAD / sd).clamp(0.0, 1.0)
}

/// Noise at `p` in [0, 1], summed over octaves: plain, or ridged, where each
/// octave peaks along the middle of its range so the lumps become crests.
fn fbm(p: Vec3, seed: u32, ridged: bool) -> f64 {
    let (mut sum, mut weight, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0);
    for octave in 0..OCTAVES {
        let n = value_noise(
            [p[0] * freq, p[1] * freq, p[2] * freq],
            seed.wrapping_add(octave as u32 * 0x632b_e5ab),
        );
        let n = if ridged {
            (1.0 - (2.0 * n - 1.0).abs()).powi(2)
        } else {
            n
        };
        sum += amp * n;
        weight += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / weight
}

/// Smoothly interpolated noise in [0, 1] over a lattice of random values.
fn value_noise(p: Vec3, seed: u32) -> f64 {
    let base = p.map(f64::floor);
    let f = [p[0] - base[0], p[1] - base[1], p[2] - base[2]];
    let s = f.map(|v| v * v * (3.0 - 2.0 * v));
    let at = |dx: i64, dy: i64, dz: i64| {
        lattice(
            base[0] as i64 + dx,
            base[1] as i64 + dy,
            base[2] as i64 + dz,
            seed,
        )
    };
    let lerp = |a: f64, b: f64, w: f64| a + (b - a) * w;
    let x00 = lerp(at(0, 0, 0), at(1, 0, 0), s[0]);
    let x10 = lerp(at(0, 1, 0), at(1, 1, 0), s[0]);
    let x01 = lerp(at(0, 0, 1), at(1, 0, 1), s[0]);
    let x11 = lerp(at(0, 1, 1), at(1, 1, 1), s[0]);
    lerp(lerp(x00, x10, s[1]), lerp(x01, x11, s[1]), s[2])
}

/// A random value in [0, 1) for a lattice point.
fn lattice(x: i64, y: i64, z: i64, seed: u32) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ (seed as u64).wrapping_mul(0x27d4_eb2f_1656_67c5);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_stays_in_range_and_the_same_seed_gives_the_same_pattern() {
        for k in 0..500 {
            let p = [k as f64 * 0.37, k as f64 * -0.11, k as f64 * 0.05];
            for ridged in [false, true] {
                let n = fbm(p, 7, ridged);
                assert!((0.0..=1.0).contains(&n), "{n}");
                assert_eq!(n, fbm(p, 7, ridged));
            }
        }
        let p = [1.3, 2.1, -0.4];
        assert_ne!(fbm(p, 7, false), fbm(p, 8, false));
    }
}
