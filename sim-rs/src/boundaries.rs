//! Boundary classification, mirroring `boundaries.ts`: every arc between two
//! plates is convergent, divergent or transform by the relative velocity of the
//! plates across it.

use crate::cell_data::CellData;
use crate::math::{cross, len3, read3};
use crate::mesh::Mesh;
use crate::par;
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
    let packed = scratch.plates();

    // Each arc's kind and its normal closing speed.
    let motion = |a: usize| -> (i8, f64) {
        let cell_a = mesh.arc_cells[a * 2];
        let cell_b = mesh.arc_cells[a * 2 + 1];
        if cell_a < 0 || cell_b < 0 {
            return (NONE, 0.0);
        }
        let plate_a = data.plate_id[cell_a as usize];
        let plate_b = data.plate_id[cell_b as usize];
        if plate_a < 0 || plate_b < 0 || plate_a == plate_b {
            return (NONE, 0.0);
        }

        // The relative surface velocity at the midpoint is
        // `(omegaA - omegaB) x mid`.
        let wa = packed.omega(plate_a);
        let wb = packed.omega(plate_b);
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
        (kind, vn)
    };
    // Only arcs between plates are active, a few in a hundred, so they come
    // back in arc order and the cells are marked from them alone.
    let active = par::filter_map(mesh.arc_count(), |a| match motion(a) {
        (NONE, _) => None,
        (kind, vn) => Some((a, kind, vn)),
    });
    out.arc_kind.fill(NONE);
    out.arc_closing.fill(0.0);
    out.cell_kind.fill(NONE as u8);
    out.cell_intensity.fill(0.0);
    for (a, kind, vn) in active {
        out.arc_kind[a] = kind;
        out.arc_closing[a] = vn as f32;
        let intensity = vn.abs();
        for cell in [mesh.arc_cells[a * 2] as usize, mesh.arc_cells[a * 2 + 1] as usize] {
            if intensity > out.cell_intensity[cell] as f64 {
                out.cell_intensity[cell] = intensity as f32;
                out.cell_kind[cell] = kind as u8;
            }
        }
    }
}
