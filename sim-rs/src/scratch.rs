//! Working buffers for the per-tick passes, allocated once per world so a tick
//! allocates nothing proportional to the mesh. Contents are meaningless between
//! passes: each pass initialises what it reads.

use crate::math::{Vec3, cross, len3, read3};
use crate::mesh::Mesh;
use crate::par;
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
    pub queue: Vec<i32>,
    pub source_of: Vec<i32>,
    pub claimed: Vec<u8>,
    pub cand: Vec<i32>,
    pub cand_count: Vec<i32>,
    /// Transport queue as packed sort keys (see `advect::descending_key`).
    pub order: Vec<u64>,
    pub f32a: Vec<f32>,
    pub f32b: Vec<f32>,
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
    /// Per-plate fastest surface speed (rad/Myr), from
    /// [`Scratch::pack_surface_speeds`].
    pub plate_speed: Vec<f64>,
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
            queue: vec![0; n],
            source_of: vec![0; n],
            claimed: vec![0; n],
            cand: vec![0; n * MAX_NBRS],
            cand_count: vec![0; n],
            order: Vec::with_capacity(n),
            f32a: vec![0.0; n],
            f32b: vec![0.0; n],
            f32d: vec![0.0; n],
            i32a: vec![0; n],
            i32b: vec![0; n],
            i32c: vec![0; n + 1],
            u8a: vec![0; n],
            u8b: vec![0; n],
            plate_omega: Vec::new(),
            plate_live: Vec::new(),
            plate_flag: Vec::new(),
            plate_speed: Vec::new(),
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
            self.plate_speed = vec![0.0; size];
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

    /// The packed plates, borrowed apart from the rest of the scratch.
    #[inline]
    pub fn plates(&self) -> PackedPlates<'_> {
        PackedPlates {
            omega: &self.plate_omega,
            live: &self.plate_live,
        }
    }

    #[inline]
    pub fn is_live(&self, plate: i32) -> bool {
        self.plates().is_live(plate)
    }

    /// Each packed plate's fastest surface speed over its own cells: `|omega x
    /// p|` at the cell farthest from its pole. A plate spinning about a pole
    /// inside itself moves its crust far slower than `|omega|`.
    pub fn pack_surface_speeds(&mut self, mesh: &Mesh, plate_id: &[i32]) {
        let plates = PackedPlates {
            omega: &self.plate_omega,
            live: &self.plate_live,
        };
        let bound = self.plate_speed.len();
        // The fastest in each chunk, then over the chunks: a maximum, so the
        // order they are taken in makes no difference.
        let fastest = par::chunks(plate_id.len(), |cells| {
            let mut fastest = vec![0.0f64; bound];
            for i in cells {
                let p = plate_id[i];
                if !plates.is_live(p) {
                    continue;
                }
                let v = cross(plates.omega(p), read3(&mesh.centres, i));
                let speed = len3(v[0], v[1], v[2]);
                let slot = &mut fastest[p as usize];
                if speed > *slot {
                    *slot = speed;
                }
            }
            fastest
        });
        self.plate_speed.fill(0.0);
        for part in fastest {
            for (slot, speed) in self.plate_speed.iter_mut().zip(part) {
                if speed > *slot {
                    *slot = speed;
                }
            }
        }
    }

    /// The packed angular velocity of `plate`, or zero if it is not live.
    #[inline]
    pub fn omega(&self, plate: i32) -> Vec3 {
        self.plates().omega(plate)
    }
}

/// Every live plate's angular velocity by stable id, as packed by
/// [`Scratch::pack_plates`].
#[derive(Clone, Copy)]
pub struct PackedPlates<'a> {
    omega: &'a [f64],
    live: &'a [u8],
}

impl PackedPlates<'_> {
    /// Built from the two fields alone, so the rest of the scratch stays free
    /// to borrow mutably alongside it.
    pub fn of<'a>(omega: &'a [f64], live: &'a [u8]) -> PackedPlates<'a> {
        PackedPlates { omega, live }
    }

    #[inline]
    pub fn is_live(&self, plate: i32) -> bool {
        plate >= 0 && (plate as usize) < self.live.len() && self.live[plate as usize] == 1
    }

    /// The angular velocity of `plate`, or zero if it is not live.
    #[inline]
    pub fn omega(&self, plate: i32) -> Vec3 {
        if self.is_live(plate) {
            let i = plate as usize * 3;
            [self.omega[i], self.omega[i + 1], self.omega[i + 2]]
        } else {
            [0.0, 0.0, 0.0]
        }
    }
}
