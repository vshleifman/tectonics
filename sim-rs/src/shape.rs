//! How elongated a patch of the sphere is: the spread of its cells in the
//! plane tangent at its centre, as a ratio of the long to the short extent.

use crate::math::{Sym3, Vec3, dominant_axis, len3, normalise};
use crate::mesh::Mesh;

/// Area-weighted moments of a set of cell centres.
#[derive(Clone, Default)]
pub struct Moments {
    area: f64,
    sum: Vec3,
    /// Second moments, `xx, yy, zz, xy, xz, yz`.
    outer: Sym3,
}

impl Moments {
    pub fn add(&mut self, mesh: &Mesh, cell: usize) {
        let a = mesh.area(cell);
        let c = &mesh.centres;
        let (x, y, z) = (
            c[cell * 3] as f64,
            c[cell * 3 + 1] as f64,
            c[cell * 3 + 2] as f64,
        );
        self.area += a;
        self.sum[0] += a * x;
        self.sum[1] += a * y;
        self.sum[2] += a * z;
        self.outer[0] += a * x * x;
        self.outer[1] += a * y * y;
        self.outer[2] += a * z * z;
        self.outer[3] += a * x * y;
        self.outer[4] += a * x * z;
        self.outer[5] += a * y * z;
    }

    pub fn of(mesh: &Mesh, cells: impl IntoIterator<Item = usize>) -> Moments {
        let mut m = Moments::default();
        for cell in cells {
            m.add(mesh, cell);
        }
        m
    }

    /// Unit direction of the patch's centre, if it has one.
    pub fn centre(&self) -> Option<Vec3> {
        let l = len3(self.sum[0], self.sum[1], self.sum[2]);
        (self.area > 0.0 && l > 1e-9 * self.area).then(|| normalise(self.sum))
    }

    /// Spread about the mean, projected into the plane tangent at the centre.
    fn tangent_spread(&self) -> Option<(Sym3, Vec3)> {
        let c = self.centre()?;
        let inv = 1.0 / self.area;
        let m = [self.sum[0] * inv, self.sum[1] * inv, self.sum[2] * inv];
        let o = &self.outer;
        let cov = [
            [
                o[0] * inv - m[0] * m[0],
                o[3] * inv - m[0] * m[1],
                o[4] * inv - m[0] * m[2],
            ],
            [
                o[3] * inv - m[0] * m[1],
                o[1] * inv - m[1] * m[1],
                o[5] * inv - m[1] * m[2],
            ],
            [
                o[4] * inv - m[0] * m[2],
                o[5] * inv - m[1] * m[2],
                o[2] * inv - m[2] * m[2],
            ],
        ];
        // P cov P with P = I - c c^T.
        let p = |i: usize, j: usize| (if i == j { 1.0 } else { 0.0 }) - c[i] * c[j];
        let mut t = [[0.0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let mut v = 0.0;
                for k in 0..3 {
                    for l in 0..3 {
                        v += p(i, k) * cov[k][l] * p(l, j);
                    }
                }
                t[i][j] = v;
            }
        }
        Some(([t[0][0], t[1][1], t[2][2], t[0][1], t[0][2], t[1][2]], c))
    }

    /// Long over short extent across the tangent plane; 1 for a round patch.
    pub fn aspect(&self) -> f64 {
        let Some((s, _)) = self.tangent_spread() else {
            return 1.0;
        };
        let [xx, yy, zz, xy, xz, yz] = s;
        // One eigenvalue is ~0 (along the centre), so the other two follow
        // from the trace and the sum of principal 2x2 minors.
        let tr = xx + yy + zz;
        let minors = xx * yy - xy * xy + xx * zz - xz * xz + yy * zz - yz * yz;
        let disc = (0.25 * tr * tr - minors).max(0.0).sqrt();
        let (hi, lo) = (0.5 * tr + disc, 0.5 * tr - disc);
        if lo <= 1e-12 * hi.max(1e-30) {
            return f64::INFINITY;
        }
        (hi / lo).sqrt()
    }

    /// The tangent direction along which the patch is longest.
    pub fn long_axis(&self) -> Option<Vec3> {
        let (s, c) = self.tangent_spread()?;
        // Seed off the centre so the iteration cannot sit on the null axis.
        let seed = if c[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        Some(dominant_axis(&s, seed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_is_elongated_and_cap_is_round() {
        let mesh = Mesh::icosphere(4);
        let c = &mesh.centres;
        let cap = Moments::of(&mesh, (0..mesh.cell_count).filter(|&i| c[i * 3 + 2] > 0.8));
        let band = Moments::of(
            &mesh,
            (0..mesh.cell_count).filter(|&i| c[i * 3 + 2] > 0.6 && c[i * 3].abs() < 0.1),
        );
        assert!(cap.aspect() < 1.3, "cap {}", cap.aspect());
        assert!(band.aspect() > 3.0, "band {}", band.aspect());
        let axis = band.long_axis().unwrap();
        assert!(axis[1].abs() > 0.9, "band axis {axis:?}");
    }
}
