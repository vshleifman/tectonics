//! Working buffers for the per-tick passes, allocated once per world so a tick
//! allocates nothing proportional to the mesh. Contents are meaningless between
//! passes: each pass initialises what it reads.

use crate::math::Vec3;
use crate::plates::PlateData;

/// Highest neighbour count a cell can have.
pub const MAX_NBRS: usize = 6;

pub struct Scratch {
    pub forward: Vec<i32>,
    pub forward_arc: Vec<i32>,
    pub new_plate: Vec<i32>,
    pub claimant: Vec<i32>,
    pub eaten: Vec<u8>,
    pub stalled: Vec<u8>,
    pub behind_start: Vec<i32>,
    pub behind: Vec<i32>,
    pub fill: Vec<i32>,
    pub queue: Vec<i32>,
    pub from_cell: Vec<i32>,
    pub onto_cell: Vec<i32>,
    pub source_of: Vec<i32>,
    pub claimed: Vec<u8>,
    pub cand: Vec<i32>,
    pub cand_score: Vec<f32>,
    pub cand_count: Vec<i32>,
    /// Transport queue as packed sort keys (see `advect::descending_key`).
    pub order: Vec<u64>,
    pub f32a: Vec<f32>,
    pub f32b: Vec<f32>,
    pub f32c: Vec<f32>,
    pub f32d: Vec<f32>,
    pub i32a: Vec<i32>,
    pub i32b: Vec<i32>,
    pub i32c: Vec<i32>,
    pub u8a: Vec<u8>,
    pub u8b: Vec<u8>,

    /// Per-plate angular velocity, `[wx, wy, wz]` per stable id.
    pub plate_omega: Vec<f64>,
    /// 1 for each stable id that is a live plate.
    pub plate_live: Vec<u8>,
    /// Per-plate flag, sized with `plate_live`.
    pub plate_flag: Vec<u8>,
}

impl Scratch {
    pub fn new(n: usize) -> Scratch {
        Scratch {
            forward: vec![0; n],
            forward_arc: vec![0; n],
            new_plate: vec![0; n],
            claimant: vec![0; n],
            eaten: vec![0; n],
            stalled: vec![0; n],
            behind_start: vec![0; n + 1],
            behind: vec![0; n],
            fill: vec![0; n],
            queue: vec![0; n],
            from_cell: vec![0; n],
            onto_cell: vec![0; n],
            source_of: vec![0; n],
            claimed: vec![0; n],
            cand: vec![0; n * MAX_NBRS],
            cand_score: vec![0.0; n * MAX_NBRS],
            cand_count: vec![0; n],
            order: Vec::with_capacity(n),
            f32a: vec![0.0; n],
            f32b: vec![0.0; n],
            f32c: vec![0.0; n],
            f32d: vec![0.0; n],
            i32a: vec![0; n],
            i32b: vec![0; n],
            i32c: vec![0; n + 1],
            u8a: vec![0; n],
            u8b: vec![0; n],
            plate_omega: Vec::new(),
            plate_live: Vec::new(),
            plate_flag: Vec::new(),
        }
    }

    /// Pack every live plate's angular velocity by stable id, growing the
    /// per-plate buffers if ids have outrun them. Returns their capacity.
    pub fn pack_plates(&mut self, plates: &PlateData) -> usize {
        let bound = plates
            .by_id
            .keys()
            .map(|&id| id as usize + 1)
            .max()
            .unwrap_or(0);
        if bound > self.plate_live.len() {
            let size = bound.max(self.plate_live.len() * 2).max(64);
            self.plate_omega = vec![0.0; size * 3];
            self.plate_live = vec![0; size];
            self.plate_flag = vec![0; size];
        }
        self.plate_omega.fill(0.0);
        self.plate_live.fill(0);
        for (&id, props) in &plates.by_id {
            let i = id as usize;
            self.plate_omega[i * 3..i * 3 + 3].copy_from_slice(&props.omega);
            self.plate_live[i] = 1;
        }
        self.plate_live.len()
    }

    #[inline]
    pub fn is_live(&self, plate: i32) -> bool {
        plate >= 0
            && (plate as usize) < self.plate_live.len()
            && self.plate_live[plate as usize] == 1
    }

    /// The packed angular velocity of `plate`, or zero if it is not live.
    #[inline]
    pub fn omega(&self, plate: i32) -> Vec3 {
        if self.is_live(plate) {
            let i = plate as usize * 3;
            [
                self.plate_omega[i],
                self.plate_omega[i + 1],
                self.plate_omega[i + 2],
            ]
        } else {
            [0.0, 0.0, 0.0]
        }
    }
}
