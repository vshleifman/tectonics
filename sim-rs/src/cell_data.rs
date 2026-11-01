//! Per-cell storage as flat arrays indexed by cell, mirroring `CellData.ts`.
//! Single precision for storage, as the TypeScript typed arrays are; all
//! arithmetic widens to `f64` and rounds back on store.

use serde::{Deserialize, Serialize};

pub const OCEANIC: u8 = 0;
pub const CONTINENTAL: u8 = 1;

/// Share of the cell's crust, by mass, that a primordial plume made: a
/// tracer for where crust came from. The plume crust still alive is the sum
/// of area x thickness x share.
pub const LANE_PLUME: usize = 0;
/// Share of the cell's crust, by mass, that is reworked sediment.
pub const LANE_SEDIMENT: usize = 1;
/// Share of the cell's crust, by mass, that is mafic igneous rock: basalt and
/// gabbro, the stuff of ocean floor and plume plateaus.
pub const LANE_MAFIC: usize = 2;
/// Share of the cell's crust, by mass, that is felsic igneous rock: granitoid
/// and the volcanics of evolved arcs.
pub const LANE_FELSIC: usize = 3;
/// Share of the cell's crust, by mass, that is sediment cover made of felsic
/// clastic rock (silt and sand shed by granitoid and metasediment).
pub const LANE_COVER_FELSIC: usize = 4;
/// Share of the cell's crust, by mass, that is sediment cover made of mafic
/// clastic rock (shed by basalt and gabbro).
pub const LANE_COVER_MAFIC: usize = 5;
/// Share of the cell's crust, by mass, that is sediment made of felsic clastic
/// rock, cover or buried: the part of [`LANE_SEDIMENT`] that came off
/// continental rock. It keeps that origin once the cover is buried, so
/// greywacke and shale shed by a continent count as the evolved crust they
/// are made of, however cool they have stayed.
pub const LANE_SEDIMENT_FELSIC: usize = 6;
/// Km of basement that erosion has removed from the crust since it formed:
/// how deep the rock now at the surface once lay. A record, not a share.
pub const LANE_EXHUMED: usize = 7;
/// [`LANE_EXHUMED`] as it stood when the crust last took in arc crust, so the
/// difference is the erosion since the last arc magmatism. A record.
pub const LANE_ARC_EXHUMED: usize = 8;
/// The hottest (°C) the rock now at the surface has been. A record.
pub const LANE_PEAK_TEMP: usize = 9;
/// The greatest pressure (kbar) the rock now at the surface has been under.
/// A record.
pub const LANE_PEAK_PRESSURE: usize = 10;
/// How many composition lanes a cell carries.
///
/// Mafic, felsic and sediment partition the crust and sum to 1; the plume
/// lane is a record of where crust came from, not another part of the sum.
/// Sediment is all the sediment in the column. Of it, the cover lanes are the
/// part still lying on top, loose and in reach of erosion; the rest has been
/// buried and deformed into the basement as metasediment. Felsic sediment is
/// the part of the sediment shed by felsic rock, and holds the felsic cover.
///
/// The last four lanes are records of what the exposed rock has been through
/// (see [`is_share`]). They ride with the crust and mix by mass like the
/// shares, but hold quantities, so they are not bounded above.
pub const LANES: usize = 11;

/// Is lane `l` a share of the crust, in [0, 1], rather than a record of the
/// rock's history?
#[inline]
pub fn is_share(l: usize) -> bool {
    l < LANE_EXHUMED
}

/// Bring lane `l`'s value into the range the lane can hold.
#[inline]
pub fn clamp_lane(l: usize, x: f64) -> f64 {
    if is_share(l) { x.clamp(0.0, 1.0) } else { x.max(0.0) }
}

/// How many kinds of cover there are.
pub const COVER_LANES: usize = 2;

/// Is lane `l` one of the cover lanes? Crust that is thrust or pulled from one
/// cell into another is deformed, and what was cover on it becomes
/// metasediment in the basement, so these lanes are left behind.
#[inline]
pub fn is_cover(l: usize) -> bool {
    l == LANE_COVER_FELSIC || l == LANE_COVER_MAFIC
}

/// Thickness (km) of the sediment cover on cell `i`.
#[inline]
pub fn cover_km(lanes: &Lanes, thickness: &[f32], i: usize) -> f64 {
    (lanes.lane(LANE_COVER_FELSIC)[i] + lanes.lane(LANE_COVER_MAFIC)[i]) as f64
        * thickness[i] as f64
}

/// The lanes of fresh ocean floor: all mafic.
pub const FRESH_FLOOR: [f32; LANES] = {
    let mut lanes = [0.0; LANES];
    lanes[LANE_MAFIC] = 1.0;
    lanes
};

/// Bulk continental crust: half mafic, half felsic, which is what
/// `Tuning::mean_continental_density` assumes. For worlds built by hand.
pub const BULK_CONTINENT: [f32; LANES] = {
    let mut lanes = [0.0; LANES];
    lanes[LANE_MAFIC] = 0.5;
    lanes[LANE_FELSIC] = 0.5;
    lanes
};

/// The makeup of crust an arc builds, `felsic` of it felsic and the rest
/// mafic. It has been nowhere yet, so its records are 0.
pub fn arc_mix(felsic: f64) -> Mix {
    let mut mix = [0.0; LANES];
    mix[LANE_MAFIC] = 1.0 - felsic;
    mix[LANE_FELSIC] = felsic;
    mix
}

/// Marks a cell whose crust has never had arc magmatism, in [`CellData::arc_age`].
pub const NEVER: f32 = f32::MAX;

/// The makeup of crust being added to a cell: its share in each lane.
pub type Mix = [f64; LANES];

/// Composition lanes: per-cell shares of the crust, by mass, each in [0, 1].
/// Every lane travels with the crust, mixes by mass wherever crust is added
/// (see [`mixed_share`]) and is untouched by removal. Stored lane-major, so
/// each lane is one contiguous slice that the per-cell passes can fill or
/// split across threads like any other field.
#[derive(Clone, Serialize, Deserialize)]
pub struct Lanes {
    n: usize,
    v: Vec<f32>,
}

impl Lanes {
    pub fn new(n: usize) -> Lanes {
        Lanes {
            n,
            v: vec![0.0; n * LANES],
        }
    }

    /// Lanes holding [`FRESH_FLOOR`] in every cell.
    pub fn fresh_floor(n: usize) -> Lanes {
        let mut lanes = Lanes::new(n);
        for (l, &share) in FRESH_FLOOR.iter().enumerate() {
            lanes.lane_mut(l).fill(share);
        }
        lanes
    }

    #[inline]
    pub fn lane(&self, l: usize) -> &[f32] {
        &self.v[l * self.n..(l + 1) * self.n]
    }

    #[inline]
    pub fn lane_mut(&mut self, l: usize) -> &mut [f32] {
        &mut self.v[l * self.n..(l + 1) * self.n]
    }

    /// Share of cell `i`'s basement that has evolved: felsic rock, and buried
    /// sediment shed by felsic rock, over the basement (the column less loose
    /// cover, which lies on whatever crust is beneath it and says nothing about
    /// it). Sediment shed by mafic rock is not evolved until it melts (see
    /// `differentiate`). 0 where there is no basement to speak of.
    #[inline]
    pub fn evolved(&self, i: usize) -> f64 {
        let n = self.n;
        let lane = |l: usize| self.v[l * n + i] as f64;
        let cover = lane(LANE_COVER_FELSIC) + lane(LANE_COVER_MAFIC);
        let basement = 1.0 - cover;
        if basement < 1e-6 {
            return 0.0;
        }
        let buried = (lane(LANE_SEDIMENT_FELSIC) - lane(LANE_COVER_FELSIC)).max(0.0);
        ((lane(LANE_FELSIC) + buried) / basement).clamp(0.0, 1.0)
    }

    /// Every lane's value at cell `i`.
    #[inline]
    pub fn get(&self, i: usize) -> [f32; LANES] {
        std::array::from_fn(|l| self.v[l * self.n + i])
    }

    #[inline]
    pub fn set(&mut self, i: usize, value: [f32; LANES]) {
        for (l, x) in value.into_iter().enumerate() {
            self.v[l * self.n + i] = x;
        }
    }

    /// Trade cells `a` and `b`.
    #[inline]
    pub fn swap(&mut self, a: usize, b: usize) {
        for l in 0..LANES {
            self.v.swap(l * self.n + a, l * self.n + b);
        }
    }

    /// Every lane as its own mutable slice, for passes that write several at
    /// once.
    pub fn split_mut(&mut self) -> [&mut [f32]; LANES] {
        let mut chunks = self.v.chunks_exact_mut(self.n.max(1));
        std::array::from_fn(|_| chunks.next().expect("one chunk per lane"))
    }

    /// Mix `added` km of crust of makeup `mix` into cell `i`, which held
    /// `before` km.
    #[inline]
    pub fn mix(&mut self, i: usize, before: f64, added: f64, mix: &Mix) {
        for (l, &share) in mix.iter().enumerate() {
            self.mix_lane(l, i, before, added, share);
        }
    }

    /// [`Lanes::mix`] for one lane, leaving the others as they are.
    #[inline]
    pub fn mix_lane(&mut self, l: usize, i: usize, before: f64, added: f64, share: f64) {
        let slot = &mut self.v[l * self.n + i];
        *slot = mixed_lane(l, *slot, before, added, share);
    }

    /// Every value, lane by lane.
    pub fn values(&self) -> &[f32] {
        &self.v
    }

    pub fn values_mut(&mut self) -> &mut [f32] {
        &mut self.v
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CellData {
    pub elevation: Vec<f32>,
    pub plate_id: Vec<i32>,
    pub crust_type: Vec<u8>,
    pub age: Vec<f32>,
    pub density: Vec<f32>,
    pub thickness: Vec<f32>,
    /// How much weaker than intact lithosphere each cell is; travels with crust.
    pub weakness: Vec<f32>,
    /// Heat in the mantle beneath each cell; stays put by position.
    pub mantle_heat: Vec<f32>,
    /// Length (radians) of slab descending beneath this cell from the plate
    /// across the margin. Non-zero marks the cell as the overriding side of a
    /// trench; travels with the crust.
    pub slab: Vec<f32>,
    /// Which side of a rift that is still stretching the cell lies on:
    /// 0 for none, otherwise [`RIFT_NEAR`] or [`RIFT_FAR`]. Travels with crust.
    pub rift_side: Vec<u8>,
    /// How far the cell's continental crust has grown a cratonic keel, in
    /// [0, 1]: the deep, cold, buoyant root of an ancient continental core.
    /// Travels with crust.
    pub keel: Vec<f32>,
    /// Thickness (km) of the cold mantle lid beneath the crust: grows as the
    /// square root of ocean floor's age, deepest under a keel, thinned over a
    /// rift or a plume. Travels with crust.
    pub lithosphere: Vec<f32>,
    /// Composition lanes ([`LANE_PLUME`], [`LANE_SEDIMENT`], [`LANE_MAFIC`],
    /// [`LANE_FELSIC`], [`LANE_COVER_FELSIC`], [`LANE_COVER_MAFIC`],
    /// [`LANE_SEDIMENT_FELSIC`]): what the
    /// crust is made of, as shares by mass in [0, 1].
    pub lanes: Lanes,
    /// Myr since the cell's crust last took in arc crust, or [`NEVER`].
    /// Travels with the crust; where crusts meet, the younger event stands.
    pub arc_age: Vec<f32>,
}

pub const RIFT_NEAR: u8 = 1;
pub const RIFT_FAR: u8 = 2;

impl CellData {
    /// Thickness (km) of cell `i`'s crust less its loose sediment cover.
    #[inline]
    pub fn basement_km(&self, i: usize) -> f64 {
        (self.thickness[i] as f64 - cover_km(&self.lanes, &self.thickness, i)).max(0.0)
    }

    pub fn new(n: usize) -> CellData {
        CellData {
            elevation: vec![0.0; n],
            plate_id: vec![0; n],
            crust_type: vec![OCEANIC; n],
            age: vec![0.0; n],
            density: vec![0.0; n],
            thickness: vec![0.0; n],
            weakness: vec![0.0; n],
            mantle_heat: vec![0.0; n],
            slab: vec![0.0; n],
            rift_side: vec![0; n],
            keel: vec![0.0; n],
            lithosphere: vec![0.0; n],
            lanes: Lanes::fresh_floor(n),
            arc_age: vec![NEVER; n],
        }
    }
}

/// Mix `added` km of crust carrying tracer share `share` into a cell holding
/// `before` km at share `current`.
#[inline]
pub fn mixed_share(current: f32, before: f64, added: f64, share: f64) -> f32 {
    mixed_lane(0, current, before, added, share)
}

/// [`mixed_share`] for lane `l`, which may be a record rather than a share.
#[inline]
pub fn mixed_lane(l: usize, current: f32, before: f64, added: f64, value: f64) -> f32 {
    let total = before.max(0.0) + added;
    if added <= 0.0 || total <= 0.0 {
        return current;
    }
    clamp_lane(l, (current as f64 * before.max(0.0) + value * added) / total) as f32
}

/// Total live crust mass: the sum of `area * thickness` over every cell.
pub fn total_crust_mass(areas: &[f32], data: &CellData) -> f64 {
    let thickness = &data.thickness;
    crate::par::sum(areas.len(), |i| areas[i] as f64 * thickness[i] as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
        #[test]
    fn evolved_counts_felsic_rock_and_buried_felsic_sediment_over_the_basement_not_loose_cover() {
        let mut l = Lanes::new(1);
        // Basement is 0.6 of the column: 0.3 felsic, 0.1 buried sediment, 0.2
        // mafic; cover is the other 0.4.
        let mut v = [0.0f32; LANES];
        v[LANE_FELSIC] = 0.3;
        v[LANE_MAFIC] = 0.2;
        v[LANE_SEDIMENT] = 0.5;
        v[LANE_COVER_FELSIC] = 0.25;
        v[LANE_COVER_MAFIC] = 0.15;
        // The buried sediment was shed by mafic rock: only the felsic rock counts.
        v[LANE_SEDIMENT_FELSIC] = 0.25;
        l.set(0, v);
        assert!((l.evolved(0) - 0.3 / 0.6).abs() < 1e-6, "{}", l.evolved(0));
        // Shed by felsic rock: it counts, however cool it has stayed.
        v[LANE_SEDIMENT_FELSIC] = 0.35;
        l.set(0, v);
        assert!((l.evolved(0) - (0.3 + 0.1) / 0.6).abs() < 1e-6);
        // Ocean floor under thick cover is still mafic.
        let mut v = [0.0f32; LANES];
        v[LANE_MAFIC] = 0.1;
        v[LANE_SEDIMENT] = 0.9;
        v[LANE_COVER_MAFIC] = 0.9;
        l.set(0, v);
        assert_eq!(l.evolved(0), 0.0);
        // Nothing but cover: no basement to judge.
        let mut v = [0.0f32; LANES];
        v[LANE_SEDIMENT] = 1.0;
        v[LANE_SEDIMENT_FELSIC] = 1.0;
        v[LANE_COVER_FELSIC] = 1.0;
        l.set(0, v);
        assert_eq!(l.evolved(0), 0.0);
    }

    /// A cell's lanes, the first few as given and the rest 0.
    fn lane_values(first: &[f32]) -> [f32; LANES] {
        std::array::from_fn(|l| first.get(l).copied().unwrap_or(0.0))
    }

    fn mix_values(first: &[f64]) -> Mix {
        std::array::from_fn(|l| first.get(l).copied().unwrap_or(0.0))
    }

    #[test]
    fn a_lane_mixes_by_mass_and_leaves_the_others_alone() {
        let mut lanes = Lanes::new(4);
        lanes.mix_lane(LANE_SEDIMENT, 2, 10.0, 10.0, 1.0);
        assert_eq!(lanes.lane(LANE_SEDIMENT)[2], 0.5);
        assert_eq!(lanes.get(2), lane_values(&[0.0, 0.5]));
        assert_eq!(lanes.get(1), [0.0; LANES]);
        // The full mix applies every lane's share.
        lanes.mix(2, 20.0, 20.0, &mix_values(&[1.0, 0.0, 0.5]));
        assert_eq!(lanes.get(2), lane_values(&[0.5, 0.25, 0.25]));
    }

    #[test]
    fn lanes_swap_and_split_by_cell() {
        let mut lanes = Lanes::new(3);
        lanes.set(0, lane_values(&[0.25, 0.75]));
        lanes.swap(0, 2);
        assert_eq!(lanes.get(0), [0.0; LANES]);
        assert_eq!(lanes.get(2), lane_values(&[0.25, 0.75]));
        let split = lanes.split_mut();
        assert_eq!((split[LANE_PLUME][2], split[LANE_SEDIMENT][2]), (0.25, 0.75));
        assert_eq!(split[LANE_PLUME].len(), 3);
    }

    #[test]
    fn fresh_floor_is_all_mafic_and_an_arc_is_a_mix_of_mafic_and_felsic() {
        let lanes = Lanes::fresh_floor(2);
        assert_eq!(lanes.get(1), FRESH_FLOOR);
        let arc = arc_mix(0.25);
        assert_eq!((arc[LANE_MAFIC], arc[LANE_FELSIC]), (0.75, 0.25));
        assert_eq!(arc[LANE_PLUME] + arc[LANE_SEDIMENT], 0.0);
    }

    #[test]
    fn records_mix_by_mass_and_are_not_capped_at_one() {
        let mut lanes = Lanes::new(1);
        lanes.set(0, lane_values(&[0.0; 0]));
        lanes.mix_lane(LANE_PEAK_TEMP, 0, 10.0, 10.0, 600.0);
        assert_eq!(lanes.lane(LANE_PEAK_TEMP)[0], 300.0);
        lanes.mix_lane(LANE_PEAK_TEMP, 0, 10.0, 10.0, 0.0);
        assert_eq!(lanes.lane(LANE_PEAK_TEMP)[0], 150.0);
        // A share is still held to 1.
        lanes.mix_lane(LANE_SEDIMENT, 0, 1.0, 1.0, 5.0);
        assert_eq!(lanes.lane(LANE_SEDIMENT)[0], 1.0);
    }
}
