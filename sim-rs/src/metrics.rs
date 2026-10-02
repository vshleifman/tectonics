//! How Earth-like the world is, mirroring `metrics.ts`, and the per-plate
//! summaries for the UI's plate list. Serialised field-for-field into the same
//! shapes as the TypeScript `WorldMetrics` and `PlateSummary`.

use crate::advect::AdvectionField;
use crate::boundaries::{BoundaryField, CONVERGENT, DIVERGENT, TRANSFORM};
use crate::cell_data::{CONTINENTAL, CellData};
use crate::math::{Vec3, cross, dot, len3, normalise};
use crate::mesh::Mesh;
use crate::plates::PlateData;
use crate::tuning::Tuning;
use serde::Serialize;
use std::collections::HashMap;

pub const AGE_BIN_MYR: f64 = 20.0;
pub const AGE_BINS: usize = 10;
const MIN_CORRELATION_AREA: f64 = 0.01;
const MAJOR_PLATE_AREA: f64 = 0.02;
const EARTH_RADIUS_KM: f64 = 6371.0;
const CM_PER_YR_PER_RAD_MYR: f64 = EARTH_RADIUS_KM * 1e5 / 1e6;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlateDrive {
    pub id: i32,
    pub speed: f64,
    pub trench_fraction: f64,
    pub area_share: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldMetrics {
    pub elapsed_myr: f64,
    pub plate_count: usize,
    pub major_plate_count: usize,
    pub continental_area: f64,
    pub continental_mass_share: f64,
    pub mean_continental_thickness: f64,
    pub thin_continental_area: f64,
    pub continentalised_by_accretion: f64,
    pub continentalised_by_spreading: f64,
    pub continental_eroded: f64,
    pub delaminated: f64,
    pub foundered: f64,
    pub decontinentalised: f64,
    pub mean_ocean_age: f64,
    pub max_ocean_age: f64,
    pub ocean_recycling_myr: Option<f64>,
    pub age_histogram: Vec<f64>,
    pub drives: Vec<PlateDrive>,
    pub boundary_share: [f64; 4],
    pub speed_trench_correlation: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlateSummary {
    pub id: i32,
    pub cell_count: usize,
    pub area_share: f64,
    pub centroid: Vec3,
    pub speed: f64,
    pub heading: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
pub fn compute_metrics(
    mesh: &Mesh,
    data: &CellData,
    plates: &PlateData,
    boundaries: &BoundaryField,
    advection: &AdvectionField,
    t: &Tuning,
    elapsed_myr: f64,
) -> WorldMetrics {
    let dt_myr = t.dt_myr;
    let mut total_area = 0.0;
    let mut continental_area = 0.0;
    let mut continental_mass = 0.0;
    let mut thin_continental_area = 0.0;
    let mut ocean_area = 0.0;
    let mut ocean_age_area = 0.0;
    let mut max_ocean_age = 0.0f64;
    let mut ocean_mass = 0.0;
    let mut age_area = [0.0f64; AGE_BINS];
    // Plates in order of first appearance, as the TypeScript Map iterates.
    let mut slot: HashMap<i32, usize> = HashMap::new();
    let mut plate_ids: Vec<i32> = Vec::new();
    let mut plate_area: Vec<f64> = Vec::new();
    let mut centroid_sum: Vec<Vec3> = Vec::new();
    let c = &mesh.centres;

    for i in 0..mesh.cell_count {
        let area = mesh.area(i);
        total_area += area;
        if data.crust_type[i] == CONTINENTAL {
            continental_area += area;
            continental_mass += area * data.thickness[i] as f64;
            if (data.thickness[i] as f64) < t.continental_thickness_threshold {
                thin_continental_area += area;
            }
        } else {
            let age = data.age[i] as f64;
            ocean_area += area;
            ocean_age_area += age * area;
            max_ocean_age = max_ocean_age.max(age);
            let bin = ((age / AGE_BIN_MYR).floor() as usize).min(AGE_BINS - 1);
            age_area[bin] += area;
            ocean_mass += area * data.thickness[i] as f64;
        }
        let id = data.plate_id[i];
        if id < 0 {
            continue;
        }
        let k = *slot.entry(id).or_insert_with(|| {
            plate_ids.push(id);
            plate_area.push(0.0);
            centroid_sum.push([0.0, 0.0, 0.0]);
            plate_ids.len() - 1
        });
        plate_area[k] += area;
        centroid_sum[k][0] += c[i * 3] as f64;
        centroid_sum[k][1] += c[i * 3 + 1] as f64;
        centroid_sum[k][2] += c[i * 3 + 2] as f64;
    }

    let mut perimeter: HashMap<i32, f64> = HashMap::new();
    let mut trench: HashMap<i32, f64> = HashMap::new();
    let mut by_kind = [0.0f64; 4];
    let mut boundary_length = 0.0;
    for a in 0..mesh.arc_count() {
        let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
        if ca < 0 || cb < 0 {
            continue;
        }
        let (ca, cb) = (ca as usize, cb as usize);
        let (pa, pb) = (data.plate_id[ca], data.plate_id[cb]);
        if pa < 0 || pb < 0 || pa == pb {
            continue;
        }
        let len = mesh.arc_length[a] as f64;
        *perimeter.entry(pa).or_insert(0.0) += len;
        *perimeter.entry(pb).or_insert(0.0) += len;
        boundary_length += len;
        let kind = boundaries.arc_kind[a];
        let slot = match kind {
            CONVERGENT => 0,
            DIVERGENT => 1,
            TRANSFORM => 2,
            _ => 3,
        };
        by_kind[slot] += len;
        if kind != CONVERGENT {
            continue;
        }
        let cont_a = data.crust_type[ca] == CONTINENTAL;
        let cont_b = data.crust_type[cb] == CONTINENTAL;
        if cont_a && cont_b {
            continue;
        }
        // Continental crust never subducts; between two oceanic cells the
        // denser one goes down.
        let down = if cont_a {
            pb
        } else if cont_b || data.density[ca] >= data.density[cb] {
            pa
        } else {
            pb
        };
        *trench.entry(down).or_insert(0.0) += len;
    }

    let mut drives: Vec<PlateDrive> = Vec::with_capacity(plate_ids.len());
    for (k, &id) in plate_ids.iter().enumerate() {
        let sum = centroid_sum[k];
        let norm = len3(sum[0], sum[1], sum[2]);
        let norm = if norm == 0.0 { 1.0 } else { norm };
        let centroid = [sum[0] / norm, sum[1] / norm, sum[2] / norm];
        let v = plates.linear_velocity(id, centroid);
        let edge = perimeter.get(&id).copied().unwrap_or(0.0);
        drives.push(PlateDrive {
            id,
            speed: len3(v[0], v[1], v[2]) * CM_PER_YR_PER_RAD_MYR,
            trench_fraction: if edge > 0.0 {
                trench.get(&id).copied().unwrap_or(0.0) / edge
            } else {
                0.0
            },
            area_share: if total_area > 0.0 {
                plate_area[k] / total_area
            } else {
                0.0
            },
        });
    }
    drives.sort_by(|a, b| b.area_share.total_cmp(&a.area_share));

    let dt = dt_myr.max(1e-9);
    let per_myr = advection.subducted_this_step / dt;
    let globe_share_per_myr = |a: f64| {
        if total_area > 0.0 {
            a / total_area / dt
        } else {
            0.0
        }
    };
    let continental_share_per_myr = |m: f64| {
        if continental_mass > 0.0 {
            m / continental_mass / dt
        } else {
            0.0
        }
    };
    let correlated: Vec<PlateDrive> = drives
        .iter()
        .filter(|d| d.area_share >= MIN_CORRELATION_AREA)
        .cloned()
        .collect();

    WorldMetrics {
        elapsed_myr,
        plate_count: plates.by_id.len(),
        major_plate_count: drives
            .iter()
            .filter(|d| d.area_share >= MAJOR_PLATE_AREA)
            .count(),
        continental_area: if total_area > 0.0 {
            continental_area / total_area
        } else {
            0.0
        },
        continental_mass_share: if continental_mass + ocean_mass > 0.0 {
            continental_mass / (continental_mass + ocean_mass)
        } else {
            0.0
        },
        mean_continental_thickness: if continental_area > 0.0 {
            continental_mass / continental_area
        } else {
            0.0
        },
        thin_continental_area: if total_area > 0.0 {
            thin_continental_area / total_area
        } else {
            0.0
        },
        continentalised_by_accretion: globe_share_per_myr(advection.continentalised_by_accretion),
        continentalised_by_spreading: globe_share_per_myr(advection.continentalised_by_spreading),
        continental_eroded: continental_share_per_myr(advection.eroded_this_step),
        delaminated: continental_share_per_myr(advection.delaminated_this_step),
        foundered: continental_share_per_myr(advection.foundered_this_step),
        decontinentalised: globe_share_per_myr(advection.decontinentalised),
        mean_ocean_age: if ocean_area > 0.0 {
            ocean_age_area / ocean_area
        } else {
            0.0
        },
        max_ocean_age,
        ocean_recycling_myr: if per_myr > 0.0 && ocean_mass > 0.0 {
            Some(ocean_mass / per_myr)
        } else {
            None
        },
        age_histogram: age_area
            .iter()
            .map(|&a| {
                if ocean_area > 0.0 {
                    a / ocean_area
                } else {
                    0.0
                }
            })
            .collect(),
        boundary_share: if boundary_length > 0.0 {
            by_kind.map(|v| v / boundary_length)
        } else {
            by_kind
        },
        speed_trench_correlation: correlate(&correlated),
        drives,
    }
}

/// Pearson correlation of speed against trench fraction, or `None` where it
/// would be noise rather than a reading.
fn correlate(drives: &[PlateDrive]) -> Option<f64> {
    let n = drives.len();
    if n < 3 {
        return None;
    }
    let mean_x = drives.iter().map(|d| d.trench_fraction).sum::<f64>() / n as f64;
    let mean_y = drives.iter().map(|d| d.speed).sum::<f64>() / n as f64;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for d in drives {
        let dx = d.trench_fraction - mean_x;
        let dy = d.speed - mean_y;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    let denom = (sxx * syy).sqrt();
    if denom < 1e-12 {
        None
    } else {
        Some(sxy / denom)
    }
}

/// One summary per live plate, ordered by stable id.
pub fn summaries(mesh: &Mesh, data: &CellData, plates: &PlateData) -> Vec<PlateSummary> {
    let mut acc: HashMap<i32, (Vec3, usize, f64)> = HashMap::new();
    let mut total_area = 0.0;
    let c = &mesh.centres;
    for i in 0..mesh.cell_count {
        let area = mesh.area(i);
        total_area += area;
        let id = data.plate_id[i];
        if id < 0 {
            continue;
        }
        let e = acc.entry(id).or_insert(([0.0; 3], 0, 0.0));
        e.0[0] += c[i * 3] as f64;
        e.0[1] += c[i * 3 + 1] as f64;
        e.0[2] += c[i * 3 + 2] as f64;
        e.1 += 1;
        e.2 += area;
    }
    let mut out: Vec<PlateSummary> = acc
        .into_iter()
        .map(|(id, (sum, cell_count, area))| {
            let centroid = normalise(sum);
            let v = plates.linear_velocity(id, centroid);
            let speed = len3(v[0], v[1], v[2]);
            PlateSummary {
                id,
                cell_count,
                area_share: if total_area > 0.0 {
                    area / total_area
                } else {
                    0.0
                },
                centroid,
                speed,
                heading: if speed > 1e-9 {
                    bearing_at(centroid, v)
                } else {
                    None
                },
            }
        })
        .collect();
    out.sort_by_key(|s| s.id);
    out
}

/// Compass bearing of tangent `v` at `p`, degrees clockwise from north.
fn bearing_at(p: Vec3, v: Vec3) -> Option<f64> {
    let up = [0.0, 1.0, 0.0];
    let radial = dot(up, p);
    let north = [
        up[0] - radial * p[0],
        up[1] - radial * p[1],
        up[2] - radial * p[2],
    ];
    if len3(north[0], north[1], north[2]) < 1e-6 {
        return None;
    }
    let n = normalise(north);
    let east = normalise(cross(p, up));
    let degrees = dot(v, east).atan2(dot(v, n)).to_degrees();
    Some((degrees + 360.0) % 360.0)
}
