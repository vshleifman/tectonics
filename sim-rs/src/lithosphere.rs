//! The lithosphere: the cold, rigid lid of mantle that rides beneath the
//! crust, and what decides whether a column of the two floats or sinks.
//!
//! Ocean crust is lighter than the mantle at every age; what makes old sea
//! floor founder is the lid beneath it. Mantle cooling from the top down
//! thickens it as the square root of age, as a cooling half-space does, until
//! the plate has cooled through at `plate_thickness`. Cooled mantle has
//! contracted, so the lid is denser than the mantle below it, and once there
//! is enough of it to outweigh the crust the column as a whole is heavier
//! than the mantle: with ordinary crust some 25 Myr after it left the ridge.
//! Thicker crust, as a hotter mantle melts at its ridges or a plume piles up,
//! holds it up for longer, or for good.
//!
//! Continental lithosphere is thicker, and deepest under a craton, whose keel
//! is the depleted residue of heavy melting: light enough to offset its cold,
//! so a keel floats however deep it reaches. A continent's lid cools toward
//! its depth over `lithosphere_growth_time`; rifts, plume heads and
//! foundering roots thin it over `lithosphere_thinning_time`.

use crate::advect::AdvectionField;
use crate::cell_data::{CONTINENTAL, CellData};
use crate::elevation::lid_density;
use crate::par;
use crate::tuning::Tuning;

/// Thickness (km) of the lid under ocean floor of this age.
pub fn ocean_lithosphere(t: &Tuning, age: f64) -> f64 {
    (t.lithosphere_growth * age.max(0.0).sqrt()).min(t.plate_thickness)
}

/// Thickness (km) a continent's lid cools toward under this keel.
pub fn continental_lithosphere(t: &Tuning, keel: f64) -> f64 {
    t.continental_lithosphere + keel * t.keel_lithosphere
}

/// Age (Myr) at which ocean floor over `thickness` km of crust grows heavy
/// enough to sink; infinite if its lid never outweighs the crust.
pub fn neutral_age(t: &Tuning, thickness: f64) -> f64 {
    let lid = lid_density(t, 0.0) - t.mantle_density;
    let crust = thickness.max(0.0) * (t.mantle_density - t.oceanic_density);
    if lid <= 0.0 || t.lithosphere_growth <= 0.0 {
        return f64::INFINITY;
    }
    let depth = crust / lid;
    if depth >= t.plate_thickness {
        return f64::INFINITY;
    }
    (depth / t.lithosphere_growth).powi(2)
}

/// Share of the way to its target a lid goes over `dt_myr`, at `time` Myr.
fn approach(dt_myr: f64, time: f64) -> f64 {
    if time > 0.0 {
        1.0 - (-dt_myr / time).exp()
    } else {
        1.0
    }
}

/// A lid of `lithosphere` km after `dt_myr` of being thinned toward
/// `rift_lithosphere`; a lid already thinner is left alone.
pub fn thinned(t: &Tuning, lithosphere: f64, dt_myr: f64) -> f64 {
    let floor = t.rift_lithosphere.min(lithosphere);
    lithosphere + (floor - lithosphere) * approach(dt_myr, t.lithosphere_thinning_time)
}

/// Set every cell's lid to the thickness its crust settles at, as if it had
/// always been there: for a world built by hand.
pub fn settle_lithosphere(data: &mut CellData, t: &Tuning) {
    let (crust_type, age, keel) = (&data.crust_type, &data.age, &data.keel);
    par::fill(&mut data.lithosphere, |i| {
        if crust_type[i] == CONTINENTAL {
            continental_lithosphere(t, keel[i] as f64) as f32
        } else {
            ocean_lithosphere(t, age[i] as f64) as f32
        }
    });
}

/// Bring every lid up to date over a tick, once the crust's age, type and
/// keel are final: ocean's by its age, continent's toward its depth, and
/// thinned over a stretching rift or a rising plume.
pub fn update_lithosphere(data: &mut CellData, field: &AdvectionField, dt_myr: f64, t: &Tuning) {
    let grow = approach(dt_myr, t.lithosphere_growth_time);
    let (crust_type, age, keel, rift_side) = (&data.crust_type, &data.age, &data.keel, &data.rift_side);
    let plume_in = &field.plume_in;
    par::for_each_mut(&mut data.lithosphere, |i, lid| {
        let l = *lid as f64;
        let cooled = if crust_type[i] == CONTINENTAL {
            l + (continental_lithosphere(t, keel[i] as f64) - l) * grow
        } else {
            ocean_lithosphere(t, age[i] as f64)
        };
        let next = if rift_side[i] != 0 || plume_in[i] > 0.0 {
            cooled.min(thinned(t, l, dt_myr))
        } else {
            cooled
        };
        *lid = next.max(0.0) as f32;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_data::OCEANIC;
    use crate::elevation::buoyancy;
    use crate::slabs::slab_pull;

    /// Buoyancy of ocean floor of `age` over `thickness` km of crust.
    fn ocean(t: &Tuning, age: f64, thickness: f64) -> f64 {
        buoyancy(t, thickness, t.oceanic_density, ocean_lithosphere(t, age), 0.0)
    }

    #[test]
    fn ordinary_ocean_floor_starts_to_sink_after_twenty_to_thirty_myr() {
        let t = Tuning::default();
        let h = t.oceanic_thickness;
        let neutral = neutral_age(&t, h);
        assert!((20.0..30.0).contains(&neutral), "neutral at {neutral:.1} Myr");
        assert!(ocean(&t, 0.0, h) > 0.0);
        assert!(ocean(&t, neutral - 1.0, h) > 0.0);
        assert!(ocean(&t, neutral + 1.0, h) < 0.0);
        assert!(ocean(&t, neutral, h).abs() < 1e-9);
    }

    #[test]
    fn thicker_crust_holds_ocean_floor_up_for_longer() {
        let t = Tuning::default();
        let ordinary = neutral_age(&t, t.oceanic_thickness);
        let hot = neutral_age(&t, 1.2 * t.oceanic_thickness);
        assert!(hot > 1.3 * ordinary, "hot-era crust neutral at {hot:.1}, ordinary at {ordinary:.1}");
        assert_eq!(neutral_age(&t, t.plume_thickness), f64::INFINITY, "a plume plateau sinks");
    }

    #[test]
    fn a_cratonic_keel_weighs_about_what_the_lid_it_replaces_does() {
        let t = Tuning::default();
        let craton = buoyancy(&t, 40.0, t.continental_density, continental_lithosphere(&t, 1.0), 1.0);
        let continent = buoyancy(&t, 40.0, t.continental_density, continental_lithosphere(&t, 0.0), 0.0);
        assert!(craton > 0.0, "a full keel sank: {craton:.2}");
        assert!(
            (craton - continent).abs() < 0.15,
            "craton floats {craton:.2}, ordinary continent {continent:.2}"
        );
    }

    #[test]
    fn slab_pull_grows_with_age_and_saturates() {
        let t = Tuning::default();
        let h = t.oceanic_thickness;
        let pull = |age: f64| slab_pull(&t, ocean(&t, age, h));
        assert_eq!(pull(10.0), 0.0);
        assert!(pull(40.0) > 0.0);
        assert!(pull(80.0) > pull(40.0));
        assert_eq!(pull(200.0), 1.0);
        assert_eq!(pull(200.0), pull(400.0));
    }

    #[test]
    fn lids_grow_cool_and_thin() {
        let t = Tuning::default();
        let mut data = CellData::new(4);
        let field = AdvectionField::new(4);
        data.crust_type = vec![OCEANIC, CONTINENTAL, CONTINENTAL, OCEANIC];
        data.age = vec![25.0, 1000.0, 1000.0, 400.0];
        data.keel = vec![0.0, 1.0, 0.0, 0.0];
        data.lithosphere = vec![0.0, 0.0, 150.0, 0.0];
        data.rift_side[2] = crate::cell_data::RIFT_NEAR;
        for _ in 0..1000 {
            update_lithosphere(&mut data, &field, t.dt_myr, &t);
        }
        assert_eq!(data.lithosphere[0], ocean_lithosphere(&t, 25.0) as f32);
        assert!((data.lithosphere[1] as f64 - continental_lithosphere(&t, 1.0)).abs() < 1.0);
        assert!((data.lithosphere[2] as f64 - t.rift_lithosphere).abs() < 1.0, "the rift kept its lid");
        assert_eq!(data.lithosphere[3], t.plate_thickness as f32);
    }
}
