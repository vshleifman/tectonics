//! Goldberg-polyhedron cell mesh: subdivide an icosahedron `level` times, treat
//! the triangle vertices as cell centres, and build the dual (hexagons plus
//! exactly twelve pentagons), the neighbour graph and the boundary graph.
//!
//! Built in the same order as the TypeScript builder, so cell, junction and arc
//! indices agree between the two and a crack walked in TypeScript lands on the
//! same arcs here.

use crate::math::{Vec3, add, cross, dot, len3, normalise, scale, sub};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A fast multiplicative hasher for the integer edge keys of mesh building.
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.hash = (self.hash.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub struct Mesh {
    pub cell_count: usize,
    pub centres: Vec<f32>,
    pub areas: Vec<f32>,
    pub nbr_off: Vec<i32>,
    pub nbr: Vec<i32>,
    pub nbr_arc: Vec<i32>,
    pub poly_off: Vec<i32>,
    pub poly_corners: Vec<f32>,
    pub junction_pos: Vec<f32>,
    pub arc_ends: Vec<i32>,
    pub arc_cells: Vec<i32>,
    pub junction_arc_off: Vec<i32>,
    pub junction_arcs: Vec<i32>,
    /// Normalised midpoint of each arc's cell pair.
    pub arc_mid: Vec<f32>,
    /// Unit tangent normal from each arc's first cell toward its second.
    pub arc_normal: Vec<f32>,
    /// Great-circle length of each arc.
    pub arc_length: Vec<f32>,
    /// Unit chord direction from each cell toward each neighbour, per entry.
    pub chord_dir: Vec<f32>,
}

impl Mesh {
    #[inline]
    pub fn arc_count(&self) -> usize {
        self.arc_cells.len() / 2
    }

    #[inline]
    pub fn junction_count(&self) -> usize {
        self.junction_pos.len() / 3
    }

    /// The neighbour-list entries of cell `i`.
    #[inline]
    pub fn nbrs(&self, i: usize) -> std::ops::Range<usize> {
        self.nbr_off[i] as usize..self.nbr_off[i + 1] as usize
    }

    #[inline]
    pub fn area(&self, i: usize) -> f64 {
        self.areas[i] as f64
    }

    pub fn icosphere(level: u32) -> Mesh {
        let (verts, faces) = subdivided(level);
        build_dual(&verts, &faces)
    }
}

fn base_icosahedron() -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let raw: [Vec3; 12] = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    let faces = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    (raw.iter().map(|&v| normalise(v)).collect(), faces)
}

/// Subdivide every triangle into four, `level` times, sharing midpoints.
fn subdivided(level: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (mut verts, mut faces) = base_icosahedron();
    for _ in 0..level {
        let stride = verts.len() as u64;
        let mut cache: FxMap<u64, u32> = FxMap::default();
        cache.reserve(faces.len() * 3 / 2);
        let mut next_faces = Vec::with_capacity(faces.len() * 4);
        let source = verts.clone();
        let mut midpoint = |a: u32, b: u32, verts: &mut Vec<Vec3>| -> u32 {
            let key = if a < b {
                a as u64 * stride + b as u64
            } else {
                b as u64 * stride + a as u64
            };
            *cache.entry(key).or_insert_with(|| {
                let mid = normalise(scale(add(source[a as usize], source[b as usize]), 0.5));
                verts.push(mid);
                (verts.len() - 1) as u32
            })
        };
        for &[a, b, c] in &faces {
            let ab = midpoint(a, b, &mut verts);
            let bc = midpoint(b, c, &mut verts);
            let ca = midpoint(c, a, &mut verts);
            next_faces.push([a, ab, ca]);
            next_faces.push([b, bc, ab]);
            next_faces.push([c, ca, bc]);
            next_faces.push([ab, bc, ca]);
        }
        faces = next_faces;
    }
    (verts, faces)
}

fn build_dual(verts: &[Vec3], faces: &[[u32; 3]]) -> Mesh {
    let cell_count = verts.len();
    let mut centres = Vec::with_capacity(cell_count * 3);
    for v in verts {
        centres.extend_from_slice(&[v[0] as f32, v[1] as f32, v[2] as f32]);
    }

    // Per-vertex neighbours in first-seen order, and incident faces.
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::with_capacity(6); cell_count];
    let mut incident: Vec<Vec<u32>> = vec![Vec::with_capacity(6); cell_count];
    let link = |from: u32, to: u32, neighbours: &mut Vec<Vec<u32>>| {
        let list = &mut neighbours[from as usize];
        if !list.contains(&to) {
            list.push(to);
        }
    };
    for (f, &[a, b, c]) in faces.iter().enumerate() {
        incident[a as usize].push(f as u32);
        incident[b as usize].push(f as u32);
        incident[c as usize].push(f as u32);
        link(a, b, &mut neighbours);
        link(a, c, &mut neighbours);
        link(b, a, &mut neighbours);
        link(b, c, &mut neighbours);
        link(c, a, &mut neighbours);
        link(c, b, &mut neighbours);
    }

    let centroids: Vec<Vec3> = faces
        .iter()
        .map(|&[a, b, c]| {
            normalise(scale(
                add(add(verts[a as usize], verts[b as usize]), verts[c as usize]),
                1.0 / 3.0,
            ))
        })
        .collect();

    // Boundary graph: junctions are faces, arcs are primal edges, numbered in
    // the order each edge is first seen.
    let n = cell_count as u64;
    let edge_key = |a: u32, b: u32| -> u64 {
        if a < b {
            a as u64 * n + b as u64
        } else {
            b as u64 * n + a as u64
        }
    };
    let mut edge_arc: FxMap<u64, u32> = FxMap::default();
    edge_arc.reserve(faces.len() * 3 / 2);
    let mut arc_ends: Vec<i32> = Vec::with_capacity(faces.len() * 3);
    let mut arc_cells: Vec<i32> = Vec::with_capacity(faces.len() * 3);
    for (f, &[a, b, c]) in faces.iter().enumerate() {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let key = edge_key(u, v);
            match edge_arc.get(&key) {
                Some(&arc) => arc_ends[arc as usize * 2 + 1] = f as i32,
                None => {
                    let arc = (arc_ends.len() / 2) as u32;
                    edge_arc.insert(key, arc);
                    arc_ends.extend_from_slice(&[f as i32, -1]);
                    let (lo, hi) = if u < v { (u, v) } else { (v, u) };
                    arc_cells.extend_from_slice(&[lo as i32, hi as i32]);
                }
            }
        }
    }
    let arc_count = arc_ends.len() / 2;
    let junction_count = faces.len();
    let mut junction_arc_off = vec![0i32; junction_count + 1];
    for a in 0..arc_count {
        junction_arc_off[arc_ends[a * 2] as usize + 1] += 1;
        let f1 = arc_ends[a * 2 + 1];
        if f1 >= 0 {
            junction_arc_off[f1 as usize + 1] += 1;
        }
    }
    for j in 0..junction_count {
        junction_arc_off[j + 1] += junction_arc_off[j];
    }
    let mut junction_arcs = vec![0i32; junction_arc_off[junction_count] as usize];
    let mut write: Vec<i32> = junction_arc_off[..junction_count].to_vec();
    for a in 0..arc_count {
        let f0 = arc_ends[a * 2] as usize;
        junction_arcs[write[f0] as usize] = a as i32;
        write[f0] += 1;
        let f1 = arc_ends[a * 2 + 1];
        if f1 >= 0 {
            junction_arcs[write[f1 as usize] as usize] = a as i32;
            write[f1 as usize] += 1;
        }
    }
    let mut junction_pos = Vec::with_capacity(junction_count * 3);
    for c in &centroids {
        junction_pos.extend_from_slice(&[c[0] as f32, c[1] as f32, c[2] as f32]);
    }

    // Neighbour CSR, recording the arc that severs each link.
    let mut nbr_off = vec![0i32; cell_count + 1];
    for i in 0..cell_count {
        nbr_off[i + 1] = nbr_off[i] + neighbours[i].len() as i32;
    }
    let mut nbr = Vec::with_capacity(nbr_off[cell_count] as usize);
    let mut nbr_arc = Vec::with_capacity(nbr_off[cell_count] as usize);
    for (i, list) in neighbours.iter().enumerate() {
        for &j in list {
            nbr.push(j as i32);
            nbr_arc.push(
                edge_arc
                    .get(&edge_key(i as u32, j))
                    .map_or(-1, |&a| a as i32),
            );
        }
    }

    // Ordered polygon corners and spherical areas.
    let mut poly_off = vec![0i32; cell_count + 1];
    for i in 0..cell_count {
        poly_off[i + 1] = poly_off[i] + incident[i].len() as i32;
    }
    let mut poly_corners = Vec::with_capacity(poly_off[cell_count] as usize * 3);
    let mut areas = Vec::with_capacity(cell_count);
    for i in 0..cell_count {
        let centre = verts[i];
        let corners: Vec<Vec3> = incident[i].iter().map(|&f| centroids[f as usize]).collect();
        let ordered = order_corners(centre, corners);
        for c in &ordered {
            poly_corners.extend_from_slice(&[c[0] as f32, c[1] as f32, c[2] as f32]);
        }
        areas.push(polygon_area(centre, &ordered) as f32);
    }

    let mut mesh = Mesh {
        cell_count,
        centres,
        areas,
        nbr_off,
        nbr,
        nbr_arc,
        poly_off,
        poly_corners,
        junction_pos,
        arc_ends,
        arc_cells,
        junction_arc_off,
        junction_arcs,
        arc_mid: Vec::new(),
        arc_normal: Vec::new(),
        arc_length: Vec::new(),
        chord_dir: Vec::new(),
    };
    derive_geometry(&mut mesh);
    mesh
}

/// Per-arc midpoints, normals and lengths, and per-neighbour chord directions,
/// computed from the stored (single-precision) positions as the TypeScript does.
fn derive_geometry(mesh: &mut Mesh) {
    let c = &mesh.centres;
    let arc_count = mesh.arc_count();
    let mut mid = Vec::with_capacity(arc_count * 3);
    let mut normal = Vec::with_capacity(arc_count * 3);
    let mut length = Vec::with_capacity(arc_count);
    for a in 0..arc_count {
        let p = mesh.arc_cells[a * 2] as usize * 3;
        let q = mesh.arc_cells[a * 2 + 1] as usize * 3;
        let (px, py, pz) = (c[p] as f64, c[p + 1] as f64, c[p + 2] as f64);
        let (qx, qy, qz) = (c[q] as f64, c[q + 1] as f64, c[q + 2] as f64);
        let m = normalise([px + qx, py + qy, pz + qz]);
        let d = [qx - px, qy - py, qz - pz];
        let radial = dot(d, m);
        let t = normalise([
            d[0] - radial * m[0],
            d[1] - radial * m[1],
            d[2] - radial * m[2],
        ]);
        mid.extend_from_slice(&[m[0] as f32, m[1] as f32, m[2] as f32]);
        normal.extend_from_slice(&[t[0] as f32, t[1] as f32, t[2] as f32]);

        let j0 = mesh.arc_ends[a * 2] as usize * 3;
        let j1 = mesh.arc_ends[a * 2 + 1] as usize * 3;
        let j = &mesh.junction_pos;
        let cos = j[j0] as f64 * j[j1] as f64
            + j[j0 + 1] as f64 * j[j1 + 1] as f64
            + j[j0 + 2] as f64 * j[j1 + 2] as f64;
        length.push(cos.clamp(-1.0, 1.0).acos() as f32);
    }
    let mut chord = Vec::with_capacity(mesh.nbr.len() * 3);
    for i in 0..mesh.cell_count {
        for k in mesh.nbrs(i) {
            let n = mesh.nbr[k] as usize;
            let dx = c[n * 3] as f64 - c[i * 3] as f64;
            let dy = c[n * 3 + 1] as f64 - c[i * 3 + 1] as f64;
            let dz = c[n * 3 + 2] as f64 - c[i * 3 + 2] as f64;
            let l = len3(dx, dy, dz);
            let l = if l == 0.0 { 1.0 } else { l };
            chord.extend_from_slice(&[(dx / l) as f32, (dy / l) as f32, (dz / l) as f32]);
        }
    }
    mesh.arc_mid = mid;
    mesh.arc_normal = normal;
    mesh.arc_length = length;
    mesh.chord_dir = chord;
}

/// Order dual-cell corners by angle in the tangent plane at `centre`.
fn order_corners(centre: Vec3, corners: Vec<Vec3>) -> Vec<Vec3> {
    if corners.is_empty() {
        return corners;
    }
    let u = normalise(sub(corners[0], scale(centre, dot(corners[0], centre))));
    let v = cross(centre, u);
    let mut with_angle: Vec<(f64, Vec3)> = corners
        .into_iter()
        .map(|corner| {
            let d = sub(corner, scale(centre, dot(corner, centre)));
            (dot(d, v).atan2(dot(d, u)), corner)
        })
        .collect();
    with_angle.sort_by(|p, q| p.0.total_cmp(&q.0));
    with_angle.into_iter().map(|(_, c)| c).collect()
}

/// Solid angle of a spherical triangle of unit vectors.
fn triangle_area(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let numerator = dot(a, cross(b, c)).abs();
    let denominator = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
    2.0 * numerator.atan2(denominator)
}

fn polygon_area(centre: Vec3, corners: &[Vec3]) -> f64 {
    let n = corners.len();
    (0..n)
        .map(|k| triangle_area(centre, corners[k], corners[(k + 1) % n]))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_twelve_pentagons() {
        let mesh = Mesh::icosphere(4);
        let pentagons = (0..mesh.cell_count)
            .filter(|&i| mesh.nbrs(i).len() == 5)
            .count();
        assert_eq!(pentagons, 12);
        assert!((0..mesh.cell_count).all(|i| {
            let n = mesh.nbrs(i).len();
            n == 5 || n == 6
        }));
    }

    #[test]
    fn total_area_is_four_pi() {
        let mesh = Mesh::icosphere(5);
        let total: f64 = mesh.areas.iter().map(|&a| a as f64).sum();
        assert!((total - 4.0 * std::f64::consts::PI).abs() < 1e-4, "{total}");
    }

    #[test]
    fn neighbour_lists_are_symmetric() {
        let mesh = Mesh::icosphere(4);
        for i in 0..mesh.cell_count {
            for k in mesh.nbrs(i) {
                let j = mesh.nbr[k] as usize;
                assert!(mesh.nbrs(j).any(|m| mesh.nbr[m] as usize == i));
            }
        }
    }

    #[test]
    fn every_link_has_its_arc() {
        let mesh = Mesh::icosphere(3);
        for i in 0..mesh.cell_count {
            for k in mesh.nbrs(i) {
                let arc = mesh.nbr_arc[k];
                assert!(arc >= 0);
                let a = arc as usize;
                let (p, q) = (
                    mesh.arc_cells[a * 2] as usize,
                    mesh.arc_cells[a * 2 + 1] as usize,
                );
                let j = mesh.nbr[k] as usize;
                assert!((p == i && q == j) || (p == j && q == i));
            }
        }
        // Euler: V - E + F = 2 for the primal triangulation.
        assert_eq!(
            mesh.cell_count as i64 - mesh.arc_count() as i64 + mesh.junction_count() as i64,
            2
        );
    }
}
