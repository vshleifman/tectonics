//! Small vector helpers and the symmetric 3x3 algebra of the plate force
//! balance. Everything is `f64`; per-cell fields are stored as `f32` and widened
//! on read, exactly as the TypeScript typed arrays are.

pub type Vec3 = [f64; 3];

/// Symmetric 3x3 matrix as `xx, yy, zz, xy, xz, yz`.
pub type Sym3 = [f64; 6];

#[inline]
pub fn len3(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

#[inline]
pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
pub fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

#[inline]
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Unit vector along `v`; the zero vector is returned unchanged.
#[inline]
pub fn normalise(v: Vec3) -> Vec3 {
    let l = len3(v[0], v[1], v[2]);
    let l = if l == 0.0 { 1.0 } else { l };
    [v[0] / l, v[1] / l, v[2] / l]
}

/// `v` turned by the rotation `omega` (rad/Myr, about its own axis) carries
/// a point through in `dt` Myr.
pub fn rotate(v: Vec3, omega: Vec3, dt: f64) -> Vec3 {
    let rate = len3(omega[0], omega[1], omega[2]);
    let angle = rate * dt;
    if angle < 1e-12 {
        return v;
    }
    let k = scale(omega, 1.0 / rate);
    let (sin, cos) = angle.sin_cos();
    let kv = cross(k, v);
    let along = dot(k, v) * (1.0 - cos);
    [
        v[0] * cos + kv[0] * sin + k[0] * along,
        v[1] * cos + kv[1] * sin + k[1] * along,
        v[2] * cos + kv[2] * sin + k[2] * along,
    ]
}

#[inline]
pub fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

/// Read the xyz triple at `i` of a flat `f32` array, widened.
#[inline]
pub fn read3(arr: &[f32], i: usize) -> Vec3 {
    [
        arr[i * 3] as f64,
        arr[i * 3 + 1] as f64,
        arr[i * 3 + 2] as f64,
    ]
}

/// Solve `m x = b` by Cramer's rule, or zero where `m` is too near singular to
/// trust (a plate small enough to be effectively one point).
pub fn solve_sym3(m: &Sym3, b: Vec3) -> Vec3 {
    let [xx, yy, zz, xy, xz, yz] = *m;
    let c00 = yy * zz - yz * yz;
    let c01 = xz * yz - xy * zz;
    let c02 = xy * yz - xz * yy;
    let det = xx * c00 + xy * c01 + xz * c02;
    let s = xx.abs() + yy.abs() + zz.abs();
    if !det.is_finite() || det.abs() < 1e-12 * (s * s * s + 1.0) {
        return [0.0, 0.0, 0.0];
    }
    let c11 = xx * zz - xz * xz;
    let c12 = xy * xz - xx * yz;
    let c22 = xx * yy - xy * xy;
    [
        (c00 * b[0] + c01 * b[1] + c02 * b[2]) / det,
        (c01 * b[0] + c11 * b[1] + c12 * b[2]) / det,
        (c02 * b[0] + c12 * b[1] + c22 * b[2]) / det,
    ]
}

/// The axis along which `m` is largest, by ten rounds of power iteration.
pub fn dominant_axis(m: &Sym3, seed: Vec3) -> Vec3 {
    let [xx, yy, zz, xy, xz, yz] = *m;
    let mut v = seed;
    for _ in 0..10 {
        let x = xx * v[0] + xy * v[1] + xz * v[2];
        let y = xy * v[0] + yy * v[1] + yz * v[2];
        let z = xz * v[0] + yz * v[1] + zz * v[2];
        let l = len3(x, y, z);
        if l < 1e-12 {
            return v;
        }
        v = [x / l, y / l, z / l];
    }
    v
}
