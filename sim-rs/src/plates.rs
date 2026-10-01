//! Per-plate properties keyed by stable plate id, mirroring `PlateData.ts`.
//!
//! Ordered maps throughout: the TypeScript iterates its `Map`s in insertion
//! order, and the order plates are visited in (merges, splits, coupling) feeds
//! back into the outcome, so keeping the same order keeps the two comparable.

use crate::cell_data::{CellData, OCEANIC};
use crate::elevation::baseline_elevation;
use crate::math::{Vec3, clamp01};
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
    pub tension: f64,
    pub axis: Vec3,
}

#[derive(Default)]
pub struct PlateData {
    pub by_id: IndexMap<i32, PlateProps>,
    /// Myr each sutured plate pair has been welded, keyed by [`suture_key`].
    pub sutures: IndexMap<i64, f64>,
    /// Myr each neighbouring pair has been moving as one without a weld.
    pub quiet: IndexMap<i64, f64>,
    /// Extensional state from the last force balance.
    pub stress: HashMap<i32, PlateStress>,
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
/// as it cools, saturating at `max_oceanic_age`.
pub fn oceanic_density_from_age(t: &Tuning, age: f64) -> f64 {
    let s = clamp01(age / t.max_oceanic_age);
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

/// Seed every cell as young oceanic crust with an age drawn per cell, and an
/// elevation on the isostatic baseline for that age.
pub fn initialise_crust(t: &Tuning, data: &mut CellData, seed: &str) {
    for i in 0..data.age.len() {
        let mut rng = Rng::new(&format!("{seed}:cell:{i}"));
        let age = rng.next_f64() * t.max_oceanic_age;
        data.crust_type[i] = OCEANIC;
        data.age[i] = age as f32;
        data.density[i] = oceanic_density_from_age(t, age) as f32;
        data.thickness[i] = t.oceanic_thickness as f32;
        data.elevation[i] = baseline_elevation(t, OCEANIC, age, t.oceanic_thickness) as f32;
    }
}
