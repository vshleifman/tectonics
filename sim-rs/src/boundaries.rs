//! Boundary classification, mirroring `boundaries.ts`: every arc between two
//! plates is convergent, divergent or transform by the relative velocity of the
//! plates across it.

use crate::cell_data::CellData;
use crate::math::{cross, len3, read3};
use crate::mesh::Mesh;
use crate::plates::{PlateData, suture_key};
use crate::scratch::Scratch;
use crate::tuning::Tuning;

pub const NONE: i8 = 0;
pub const CONVERGENT: i8 = 1;
pub const DIVERGENT: i8 = 2;
pub const TRANSFORM: i8 = 3;

pub struct BoundaryField {
    pub arc_kind: Vec<i8>,
    /// Signed normal relative speed per arc (+ closing, - opening).
    pub arc_closing: Vec<f32>,
    /// Dominant boundary kind touching each cell.
    pub cell_kind: Vec<u8>,
    pub cell_intensity: Vec<f32>,
}

impl BoundaryField {
    pub fn new(cells: usize, arcs: usize) -> BoundaryField {
        BoundaryField {
            arc_kind: vec![NONE; arcs],
            arc_closing: vec![0.0; arcs],
            cell_kind: vec![0; cells],
            cell_intensity: vec![0.0; cells],
        }
    }
}

pub fn classify(
    mesh: &Mesh,
    data: &CellData,
    plates: &PlateData,
    out: &mut BoundaryField,
    scratch: &mut Scratch,
    t: &Tuning,
) {
    scratch.pack_plates(plates);
    out.arc_kind.fill(NONE);
    out.arc_closing.fill(0.0);
    out.cell_kind.fill(NONE as u8);
    out.cell_intensity.fill(0.0);

    for a in 0..mesh.arc_count() {
        let cell_a = mesh.arc_cells[a * 2];
        let cell_b = mesh.arc_cells[a * 2 + 1];
        if cell_a < 0 || cell_b < 0 {
            continue;
        }
        let (cell_a, cell_b) = (cell_a as usize, cell_b as usize);
        let plate_a = data.plate_id[cell_a];
        let plate_b = data.plate_id[cell_b];
        if plate_a < 0 || plate_b < 0 || plate_a == plate_b {
            continue;
        }

        // The relative surface velocity at the midpoint is
        // `(omegaA - omegaB) x mid`.
        let wa = scratch.omega(plate_a);
        let wb = scratch.omega(plate_b);
        let w = [wa[0] - wb[0], wa[1] - wb[1], wa[2] - wb[2]];
        let r = cross(w, read3(&mesh.arc_mid, a));
        let n = read3(&mesh.arc_normal, a);
        let vn = r[0] * n[0] + r[1] * n[1] + r[2] * n[2];
        let vt = len3(r[0] - vn * n[0], r[1] - vn * n[1], r[2] - vn * n[2]);
        let speed = len3(r[0], r[1], r[2]);

        let mut kind = NONE;
        if speed >= t.active_speed_eps {
            kind = if vn.abs() >= vt {
                if vn > 0.0 { CONVERGENT } else { DIVERGENT }
            } else {
                TRANSFORM
            };
        }
        // A sutured pair is welded along this line, so it cannot read as a
        // spreading centre whatever residual motion is left.
        if kind == DIVERGENT && plates.sutures.contains_key(&suture_key(plate_a, plate_b)) {
            kind = NONE;
        }

        out.arc_kind[a] = kind;
        out.arc_closing[a] = if kind == NONE { 0.0 } else { vn as f32 };
        if kind == NONE {
            continue;
        }
        let intensity = vn.abs();
        if intensity > out.cell_intensity[cell_a] as f64 {
            out.cell_intensity[cell_a] = intensity as f32;
            out.cell_kind[cell_a] = kind as u8;
        }
        if intensity > out.cell_intensity[cell_b] as f64 {
            out.cell_intensity[cell_b] = intensity as f32;
            out.cell_kind[cell_b] = kind as u8;
        }
    }
}
