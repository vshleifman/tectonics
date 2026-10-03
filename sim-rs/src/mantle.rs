//! Mantle flow toward downwellings: what gathers continents into
//! supercontinents.
//!
//! Slabs sinking at trenches drag the mantle down with them, and cold slab
//! piles beneath long-lived subduction zones keep it going down for a hundred
//! million years and more after. The upper mantle flows in toward these
//! downwellings from all around, and plates feel that flow as traction on
//! their bases: weakly under ocean, where a soft asthenosphere decouples
//! them, strongly under the deep keels of continents. So continents drift
//! toward where the mantle goes down and pile up there, which is how
//! supercontinents assemble; the heat that then builds beneath them (see
//! `rifting.rs`) breaks them up again.
//!
//! The flow is worked out on a coarse grid of mantle sites (the vertices of a
//! level-3 icosphere, which are the first cells of any finer mesh), so its cost
//! does not grow with the mesh. Each site's downwelling is the rate at which
//! plate has been subducting there, remembered over `downwelling_memory`. Only
//! continental keels are carried by it (see `plate_forces.rs`). The
//! flow is that of point sinks in a thin spherical shell, balanced by gentle
//! upwelling everywhere else: a sink taking in area `Q` per Myr draws the
//! surface toward it at `Q cot(d/2) / 4π` at angular distance `d`, strong near
//! it, weak far off and nothing at its antipode.

use crate::boundaries::{BoundaryField, CONVERGENT};
use crate::cell_data::CellData;
use crate::math::Vec3;
use crate::mesh::Mesh;
use crate::slabs::subduction;
use crate::tuning::Tuning;

/// Mantle sites: the vertices of a level-3 icosphere.
const SITES: usize = 642;
/// Closest a sink's pull is evaluated, as a multiple of the site spacing, so
/// the flow right over a downwelling stays finite.
const NEAR_SITES: f64 = 1.5;

pub struct MantleFlow {
    /// The mantle site each cell lies over.
    site_of: Vec<u32>,
    sites: Vec<Vec3>,
    /// Remembered subduction at each site, as area of plate per Myr.
    sink: Vec<f64>,
    flux: Vec<f64>,
    /// Flow at the top of the mantle over each site, as a tangent velocity
    /// (rad/Myr).
    wind: Vec<Vec3>,
}

impl MantleFlow {
    pub fn new(mesh: &Mesh) -> MantleFlow {
        let n = mesh.cell_count;
        let count = SITES.min(n);
        let sites: Vec<Vec3> = (0..count)
            .map(|i| {
                [
                    mesh.centres[i * 3] as f64,
                    mesh.centres[i * 3 + 1] as f64,
                    mesh.centres[i * 3 + 2] as f64,
                ]
            })
            .collect();
        // Each cell to the site nearest it through the mesh, by a breadth-first
        // search out from every site at once.
        let mut site_of = vec![u32::MAX; n];
        let mut queue: Vec<usize> = (0..count).collect();
        for (i, s) in site_of.iter_mut().enumerate().take(count) {
            *s = i as u32;
        }
        let mut head = 0;
        while head < queue.len() {
            let cell = queue[head];
            head += 1;
            for k in mesh.nbrs(cell) {
                let m = mesh.nbr[k] as usize;
                if site_of[m] == u32::MAX {
                    site_of[m] = site_of[cell];
                    queue.push(m);
                }
            }
        }
        MantleFlow {
            site_of,
            sites,
            sink: vec![0.0; count],
            flux: vec![0.0; count],
            wind: vec![[0.0; 3]; count],
        }
    }

    /// Forget every downwelling, as for a world rebuilt from scratch.
    pub fn reset(&mut self) {
        self.sink.fill(0.0);
        self.wind.fill([0.0; 3]);
    }

    /// The mantle flow beneath cell `i`.
    #[inline]
    pub fn wind_at(&self, i: usize) -> Vec3 {
        self.wind[self.site_of[i] as usize]
    }

    /// Fold this step's subduction into the downwellings and work out the flow
    /// they drive.
    pub fn update(
        &mut self,
        mesh: &Mesh,
        data: &CellData,
        boundaries: &BoundaryField,
        dt_myr: f64,
        t: &Tuning,
    ) {
        self.flux.fill(0.0);
        for a in 0..mesh.arc_count() {
            if boundaries.arc_kind[a] != CONVERGENT {
                continue;
            }
            let (ca, cb) = (mesh.arc_cells[a * 2], mesh.arc_cells[a * 2 + 1]);
            if ca < 0 || cb < 0 {
                continue;
            }
            let closing = (boundaries.arc_closing[a] as f64).max(0.0);
            if closing <= 0.0 {
                continue;
            }
            let Some(sub) = subduction(mesh, data, ca as usize, cb as usize, t) else {
                continue;
            };
            self.flux[self.site_of[sub.down] as usize] += closing * mesh.arc_length[a] as f64;
        }
        let blend = if t.downwelling_memory > 0.0 {
            (dt_myr / t.downwelling_memory).min(1.0)
        } else {
            1.0
        };
        for (sink, &flux) in self.sink.iter_mut().zip(&self.flux) {
            *sink += (flux - *sink) * blend;
        }
        self.flow();
    }

    /// The flow at every site from the downwellings as they stand.
    fn flow(&mut self) {
        let count = self.sites.len();
        let spacing = (4.0 * std::f64::consts::PI / count as f64).sqrt();
        let near = (NEAR_SITES * spacing).min(std::f64::consts::PI);
        let floor = near.sin().powi(2);
        let per_sink = 1.0 / (4.0 * std::f64::consts::PI);
        for i in 0..count {
            let p = self.sites[i];
            let mut u = [0.0f64; 3];
            for (j, &q) in self.sink.iter().enumerate() {
                if q <= 0.0 || j == i {
                    continue;
                }
                let s = self.sites[j];
                let cos = p[0] * s[0] + p[1] * s[1] + p[2] * s[2];
                // Toward the sink along the surface, unnormalised: its length
                // is sin d.
                let toward = [s[0] - cos * p[0], s[1] - cos * p[1], s[2] - cos * p[2]];
                let sin2 = (1.0 - cos * cos).max(floor);
                // cot(d/2) = (1 + cos d) / sin d, along a unit vector.
                let w = q * per_sink * (1.0 + cos) / sin2;
                u[0] += w * toward[0];
                u[1] += w * toward[1];
                u[2] += w * toward[2];
            }
            self.wind[i] = u;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{dot, len3};

    #[test]
    fn the_mantle_flows_toward_a_downwelling_and_fades_with_distance() {
        let mesh = Mesh::icosphere(4);
        let mut mantle = MantleFlow::new(&mesh);
        mantle.sink[0] = 1.0;
        mantle.flow();
        let sink = mantle.sites[0];
        let mut by_distance: Vec<(f64, f64)> = Vec::new();
        for (i, &p) in mantle.sites.iter().enumerate().skip(1) {
            let u = mantle.wind[i];
            let speed = len3(u[0], u[1], u[2]);
            let cos = dot(p, sink);
            let toward = [
                sink[0] - cos * p[0],
                sink[1] - cos * p[1],
                sink[2] - cos * p[2],
            ];
            if cos > -0.99 {
                assert!(dot(u, toward) > 0.0, "site {i} flows away from the sink");
                assert!(
                    dot(u, p).abs() < 1e-5 * speed,
                    "site {i} flows off the surface"
                );
            }
            by_distance.push((cos.acos(), speed));
        }
        by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
        let near = by_distance[by_distance.len() / 10].1;
        let far = by_distance[by_distance.len() * 9 / 10].1;
        assert!(
            near > 4.0 * far,
            "flow barely fades: {near} near, {far} far"
        );
        let antipode = by_distance.last().unwrap();
        assert!(
            antipode.1 < 0.05 * near,
            "flow at the antipode: {}",
            antipode.1
        );
    }

    #[test]
    fn every_cell_lies_over_a_nearby_site() {
        let mesh = Mesh::icosphere(5);
        let mantle = MantleFlow::new(&mesh);
        let spacing = (4.0 * std::f64::consts::PI / mantle.sites.len() as f64).sqrt();
        for i in 0..mesh.cell_count {
            let c = [
                mesh.centres[i * 3] as f64,
                mesh.centres[i * 3 + 1] as f64,
                mesh.centres[i * 3 + 2] as f64,
            ];
            let s = mantle.sites[mantle.site_of[i] as usize];
            assert!(
                dot(c, s).clamp(-1.0, 1.0).acos() < spacing,
                "cell {i} far from its site"
            );
        }
    }
}
