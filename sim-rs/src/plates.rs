//! Per-plate properties keyed by stable plate id, mirroring `PlateData.ts`.
//!
//! Ordered maps throughout: the TypeScript iterates its `Map`s in insertion
//! order, and the order plates are visited in (merges, splits, coupling) feeds
//! back into the outcome, so keeping the same order keeps the two comparable.

use crate::cell_data::{CellData, OCEANIC};
use crate::elevation::{baseline_elevation, subsidence};
use crate::math::Vec3;
use crate::registry::RelabelReport;
use crate::rng::Rng;
use crate::tuning::Tuning;
use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct PlateProps {
    /// Angular velocity (rad/Myr): direction is the Euler pole.
    pub omega: Vec3,
    /// Displacement (radians) banked but not yet taken as a whole-cell step.
    pub pending: f64,
}

#[derive(Clone, Debug)]
pub struct PlateStress {
    /// Slab pull that cancels out across the plate: a force, which becomes a
    /// stress once spread over the width of the plate it has to cross.
    pub opposed_pull: f64,
    pub axis: Vec3,
}

/// A rift laid out across a plate that has not yet broken it.
#[derive(Clone, Debug)]
pub struct OpeningRift {
    /// Myr it has been stretching.
    pub elapsed: f64,
    /// Myr it must stretch before the plate breaks.
    pub duration: f64,
    /// The direction the plate is being pulled apart in, turned with the plate
    /// as it moves.
    pub axis: Vec3,
}

#[derive(Default)]
pub struct PlateData {
    pub by_id: IndexMap<i32, PlateProps>,
    /// Myr each sutured plate pair has been welded, keyed by [`suture_key`].
    pub sutures: IndexMap<i64, f64>,
    /// Myr each neighbouring pair has been moving as one without a weld.
    pub quiet: IndexMap<i64, f64>,
    /// Myr each welded pair's cratons have touched across the weld: one rigid
    /// craton that happens to straddle two plates, so held to a common motion
    /// and, after `craton_merge_time`, made one plate.
    pub craton_welds: IndexMap<i64, f64>,
    /// Extensional state from the last force balance.
    pub stress: HashMap<i32, PlateStress>,
    /// Plates with a rift still stretching.
    pub opening: IndexMap<i32, OpeningRift>,
    /// Myr since each pair of plates broke apart over trapped heat, keyed by
    /// [`suture_key`]: the dome between them pushes them apart until it has
    /// vented, then stops.
    pub breakups: IndexMap<i64, f64>,
    /// Rifts that broke a plate in two since the world was built.
    pub rifts: u64,
    /// Rifts that began to stretch but stalled and healed.
    pub failed_rifts: u64,
    /// Oceans broken away from the continent they aged against.
    pub passive_collapses: u64,
}

/// Key for an unordered pair of plate ids.
#[inline]
pub fn suture_key(a: i32, b: i32) -> i64 {
    if a < b {
        a as i64 * 1_000_000 + b as i64
    } else {
        b as i64 * 1_000_000 + a as i64
    }
}

#[inline]
pub fn split_key(key: i64) -> (i32, i32) {
    ((key / 1_000_000) as i32, (key % 1_000_000) as i32)
}

impl PlateData {
    /// Surface velocity plate `id` imparts at `pos`: `omega x pos`.
    pub fn linear_velocity(&self, id: i32, pos: Vec3) -> Vec3 {
        match self.by_id.get(&id) {
            Some(p) => crate::math::cross(p.omega, pos),
            None => [0.0, 0.0, 0.0],
        }
    }
}

/// Oceanic crust density for an age: young ridge crust is light and densifies
/// as it cools, along the same curve the sea floor subsides by.
pub fn oceanic_density_from_age(t: &Tuning, age: f64) -> f64 {
    let s = subsidence(t, age);
    t.oceanic_density_young + s * (t.oceanic_density_old - t.oceanic_density_young)
}

/// Bring per-plate properties in line with a relabel: retired plates go, split
/// fragments inherit their parent's motion, genuinely new plates start at rest.
pub fn reconcile(plates: &mut PlateData, report: &RelabelReport) {
    for id in &report.retired {
        plates.by_id.shift_remove(id);
    }
    for (&id, &parent) in report.new_ids.iter().zip(&report.parent_of) {
        let omega = if parent >= 0 {
            plates.by_id.get(&parent).map(|p| p.omega)
        } else {
            None
        };
        plates.by_id.insert(
            id,
            PlateProps {
                omega: omega.unwrap_or([0.0, 0.0, 0.0]),
                pending: 0.0,
            },
        );
    }
}

/// Waves summed into the starting age field.
const AGE_WAVES: usize = 8;

/// Seed every cell as oceanic crust, with an elevation on the isostatic
/// baseline for its age. Ages vary smoothly over the globe, as a sum of a few
/// long random waves, so that neighbouring floor is of much the same age as
/// sea floor laid down at ridges is, rather than noise from cell to cell.
pub fn initialise_crust(t: &Tuning, centres: &[f32], data: &mut CellData, seed: &str) {
    let mut rng = Rng::new(&format!("{seed}:ages"));
    let waves: Vec<(Vec3, f64, f64)> = (0..AGE_WAVES)
        .map(|_| {
            let z = 2.0 * rng.next_f64() - 1.0;
            let phi = 2.0 * std::f64::consts::PI * rng.next_f64();
            let r = (1.0 - z * z).max(0.0).sqrt();
            let dir = [r * phi.cos(), r * phi.sin(), z];
            let wavenumber = 1.0 + 2.0 * rng.next_f64();
            let phase = 2.0 * std::f64::consts::PI * rng.next_f64();
            (dir, wavenumber, phase)
        })
        .collect();
    let n = data.age.len();
    let field: Vec<f64> = (0..n)
        .map(|i| {
            let p = [centres[i * 3] as f64, centres[i * 3 + 1] as f64, centres[i * 3 + 2] as f64];
            waves
                .iter()
                .map(|(d, k, phase)| (k * (p[0] * d[0] + p[1] * d[1] + p[2] * d[2]) + phase).cos())
                .sum()
        })
        .collect();
    let (lo, hi) = field
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &x| (l.min(x), h.max(x)));
    let span = (hi - lo).max(1e-12);
    for i in 0..n {
        let age = (field[i] - lo) / span * t.max_oceanic_age;
        data.crust_type[i] = OCEANIC;
        data.age[i] = age as f32;
        data.density[i] = oceanic_density_from_age(t, age) as f32;
        data.thickness[i] = t.oceanic_thickness as f32;
        data.elevation[i] = baseline_elevation(t, OCEANIC, age, t.oceanic_thickness) as f32;
        data.slab[i] = 0.0;
        data.plume_share[i] = 0.0;
    }
}
