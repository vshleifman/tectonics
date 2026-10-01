//! Per-cell storage as flat arrays indexed by cell, mirroring `CellData.ts`.
//! Single precision for storage, as the TypeScript typed arrays are; all
//! arithmetic widens to `f64` and rounds back on store.

pub const OCEANIC: u8 = 0;
pub const CONTINENTAL: u8 = 1;

#[derive(Clone)]
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
}

impl CellData {
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
        }
    }
}

/// Total live crust mass: the sum of `area * thickness` over every cell.
pub fn total_crust_mass(areas: &[f32], data: &CellData) -> f64 {
    areas
        .iter()
        .zip(&data.thickness)
        .map(|(&a, &t)| a as f64 * t as f64)
        .sum()
}
