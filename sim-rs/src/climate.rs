//! What the weather does to the surface. There is no climate model yet, so
//! nothing here changes anything: [`Climate`] holds no data, and the two
//! functions erosion calls through ([`weathering_rate`] and [`deposit_lanes`])
//! give the answer that needs none. A climate model fills `Climate` in, and
//! these functions become its answer, with no other part of the sim touched.

use crate::cell_data::COVER_LANES;
use serde::{Deserialize, Serialize};

/// Per-cell climate, each field present once a model supplies it.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Climate {
    /// Mean surface temperature (°C).
    pub temperature: Option<Vec<f32>>,
    /// Water running off the surface (km of depth per Myr).
    pub runoff: Option<Vec<f32>>,
}

/// One cell's climate, when there is a model to give it.
#[derive(Clone, Copy, Debug)]
pub struct ClimateSample {
    pub temperature: f64,
    pub runoff: f64,
}

impl Climate {
    /// Cell `i`'s climate, or `None` while there is no model.
    pub fn sample(&self, i: usize) -> Option<ClimateSample> {
        let (temperature, runoff) = (self.temperature.as_ref()?, self.runoff.as_ref()?);
        Some(ClimateSample {
            temperature: temperature[i] as f64,
            runoff: runoff[i] as f64,
        })
    }
}

/// Where eroded rock is laid down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// The first ring of sea floor off the coast.
    Shelf,
    /// The sea floor beyond the shelf.
    Slope,
    /// A closed basin, or the edge of the region eroded rock can reach.
    Basin,
}

/// How much faster or slower land wears down than the base rate: 1 without a
/// climate. Warm, wet land weathers faster than cold, dry land.
pub fn weathering_rate(_sample: Option<ClimateSample>) -> f64 {
    1.0
}

/// What a deposit is made of, given the clastic rock that reached it: `source`
/// holds the mass (area x km) of each cover lane, in the order of the cover
/// lanes, and the answer the same. Without a climate it is the clastic rock
/// as it arrived. A model with one can lay down other kinds (carbonate where
/// warm shallow water is clear of silt, evaporite where it is dry) by adding
/// lanes and returning them here.
pub fn deposit_lanes(
    _setting: Setting,
    _sample: Option<ClimateSample>,
    source: [f64; COVER_LANES],
) -> [f64; COVER_LANES] {
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_model_there_is_no_climate_and_nothing_changes() {
        let climate = Climate::default();
        assert!(climate.sample(0).is_none());
        assert_eq!(weathering_rate(climate.sample(0)), 1.0);
        let source = [3.0, 4.0];
        assert_eq!(deposit_lanes(Setting::Shelf, None, source), source);
    }

    #[test]
    fn a_cell_has_a_climate_once_the_model_has_filled_it_in() {
        let climate = Climate {
            temperature: Some(vec![12.0, 30.0]),
            runoff: Some(vec![0.1, 0.4]),
        };
        let sample = climate.sample(1).expect("a model supplied it");
        assert_eq!((sample.temperature, sample.runoff), (30.0, 0.4 as f32 as f64));
        // Half a climate is none: both are needed.
        let partial = Climate {
            temperature: Some(vec![12.0]),
            runoff: None,
        };
        assert!(partial.sample(0).is_none());
    }
}
