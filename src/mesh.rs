//! 三维 (DESIGN.md §1.18): a 3D object is one more kind of instance. This file is the part that needs no
//! GPU: 4 × 4 matrices, meshes generated in code (and a small glTF 2.0 subset behind the `gltf` feature),
//! what a `scene3d` element holds (meshes with transforms, one camera, one light, the pass's numbers), and
//! picking by casting a ray through the same transforms the GPU draws with.
//!
//! Meshes are flat-shaded triangle lists: every triangle has its own three corners, and its normal is
//! the face's. That is what the mesh pass draws and what the edge detection expects.
use crate::el::Paint;
#[cfg(feature = "gltf")]
use crate::error::MeshError;
use std::hash::{Hash, Hasher};

// ── Matrices ─────────────────────────────────────────────────────────────────────────────────────

/// A 4 × 4 matrix, column-major (as WGSL reads it): element (row r, column c) is `0[c * 4 + r]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [f32; 16]);
impl Default for Mat4 { fn default() -> Self { Self::IDENTITY } }
impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    pub fn translate(x: f32, y: f32, z: f32) -> Self { let mut m = Self::IDENTITY; m.0[12] = x; m.0[13] = y; m.0[14] = z; m }
    pub fn scale(x: f32, y: f32, z: f32) -> Self { let mut m = Self::IDENTITY; m.0[0] = x; m.0[5] = y; m.0[10] = z; m }
    pub fn uniform(s: f32) -> Self { Self::scale(s, s, s) }
    /// Rotation about x, y or z by `deg` degrees (right-handed).
    pub fn rotate_x(deg: f32) -> Self { let (s, c) = deg.to_radians().sin_cos(); Mat4([1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    pub fn rotate_y(deg: f32) -> Self { let (s, c) = deg.to_radians().sin_cos(); Mat4([c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    pub fn rotate_z(deg: f32) -> Self { let (s, c) = deg.to_radians().sin_cos(); Mat4([c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]) }
    /// `self × o`: `o` is applied first.
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [0.0; 16];
        for c in 0..4 { for row in 0..4 { r[c * 4 + row] = (0..4).map(|k| self.0[k * 4 + row] * o.0[c * 4 + k]).sum(); } }
        Mat4(r)
    }
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12], m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13], m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]]
    }
    pub fn vector(&self, v: [f32; 3]) -> [f32; 3] {
        let m = &self.0;
        [m[0] * v[0] + m[4] * v[1] + m[8] * v[2], m[1] * v[0] + m[5] * v[1] + m[9] * v[2], m[2] * v[0] + m[6] * v[1] + m[10] * v[2]]
    }
    /// The four columns, as the instance buffer carries them.
    pub fn cols(&self) -> [[f32; 4]; 4] { std::array::from_fn(|c| [self.0[c * 4], self.0[c * 4 + 1], self.0[c * 4 + 2], self.0[c * 4 + 3]]) }
    /// Translation, rotation (a unit quaternion x, y, z, w) and scale, as glTF writes them.
    pub fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Mat4 {
        let [x, y, z, w] = q;
        let r = [
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w),
            2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w),
            2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y),
        ];
        Mat4([r[0] * s[0], r[1] * s[0], r[2] * s[0], 0.0, r[3] * s[1], r[4] * s[1], r[5] * s[1], 0.0, r[6] * s[2], r[7] * s[2], r[8] * s[2], 0.0, t[0], t[1], t[2], 1.0])
    }
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
pub fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
pub fn mulf(a: [f32; 3], k: f32) -> [f32; 3] { [a[0] * k, a[1] * k, a[2] * k] }
pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
pub fn norm(a: [f32; 3]) -> [f32; 3] { let l = dot(a, a).sqrt(); if l > 0.0 { mulf(a, 1.0 / l) } else { [0.0, 0.0, 1.0] } }

// ── Meshes ───────────────────────────────────────────────────────────────────────────────────────

/// A mesh: triangles, counter-clockwise seen from outside. Flat-shaded (each triangle its face normal)
/// unless `normals` gives every corner its own: then curved surfaces shade smoothly and only the creases
/// between them, where the corners' normals differ, are edges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh { pub tris: Vec<[[f32; 3]; 3]>, pub normals: Vec<[[f32; 3]; 3]> }
impl Mesh {
    /// From shared corners and index triples.
    pub fn indexed(pos: &[[f32; 3]], idx: &[[u32; 3]]) -> Self {
        Self { tris: idx.iter().map(|t| [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]]).collect(), normals: vec![] }
    }
    pub fn normal(t: &[[f32; 3]; 3]) -> [f32; 3] { norm(cross(sub(t[1], t[0]), sub(t[2], t[0]))) }
    /// The vertex buffer: position and normal per corner (six floats).
    pub fn vertices(&self) -> Vec<[f32; 6]> {
        let mut v = Vec::with_capacity(self.tris.len() * 3);
        for (i, t) in self.tris.iter().enumerate() {
            let f = Self::normal(t);
            for (j, p) in t.iter().enumerate() {
                let n = self.normals.get(i).map_or(f, |n| n[j]);
                v.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
        }
        v
    }
    /// Smooth normals across edges whose faces meet at less than `crease_deg`; sharper edges stay
    /// creases (each side keeps its own face's normal).
    pub fn creased(mut self, crease_deg: f32) -> Self {
        let cos = crease_deg.to_radians().cos();
        let key = |p: [f32; 3]| [(p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64, (p[2] * 1e4).round() as i64];
        let faces: Vec<[f32; 3]> = self.tris.iter().map(Self::normal).collect();
        let mut at: std::collections::HashMap<[i64; 3], Vec<usize>> = std::collections::HashMap::new();
        for (i, t) in self.tris.iter().enumerate() { for p in t { at.entry(key(*p)).or_default().push(i); } }
        self.normals = self.tris.iter().enumerate().map(|(i, t)| std::array::from_fn(|j| {
            let mut n = [0.0; 3];
            for &k in &at[&key(t[j])] { if dot(faces[k], faces[i]) >= cos { n = add(n, faces[k]); } }
            norm(n)
        })).collect();
        self
    }
    /// The same mesh moved by `m` (positions and normals).
    pub fn transformed(&self, m: &Mat4) -> Self {
        Self { tris: self.tris.iter().map(|t| t.map(|p| m.point(p))).collect(), normals: self.normals.iter().map(|t| t.map(|n| norm(m.vector(n)))).collect() }
    }
    /// Both meshes in one.
    pub fn merged(mut self, o: &Mesh) -> Self {
        let flat = |m: &Mesh| -> Vec<[[f32; 3]; 3]> { if m.normals.len() == m.tris.len() { m.normals.clone() } else { m.tris.iter().map(|t| { let n = Self::normal(t); [n, n, n] }).collect() } };
        let (mut a, b) = (flat(&self), flat(o));
        a.extend(b);
        self.tris.extend(o.tris.iter().copied());
        self.normals = a;
        self
    }
    /// The radius of the smallest origin-centred ball holding it.
    /// Where a ray (origin, unit direction; the mesh's own space) meets the mesh first: the distance,
    /// the point and the face's normal turned toward the ray's origin.
    pub fn ray(&self, o: [f32; 3], d: [f32; 3]) -> Option<(f32, [f32; 3], [f32; 3])> {
        let mut best: Option<(f32, [f32; 3], [f32; 3])> = None;
        for t in &self.tris {
            if let Some(dist) = ray_tri(o, d, t[0], t[1], t[2]) && best.is_none_or(|b| dist < b.0) {
                let mut n = Mesh::normal(t);
                if dot(n, d) > 0.0 { n = mulf(n, -1.0); }
                best = Some((dist, add(o, mulf(d, dist)), n));
            }
        }
        best
    }
    pub fn radius(&self) -> f32 { self.tris.iter().flatten().map(|p| dot(*p, *p)).fold(0.0, f32::max).sqrt() }
    /// Area-weighted points on its surface (for particles): `n` points, deterministic for a seed.
    pub fn sample(&self, n: usize, seed: u32) -> Vec<([f32; 3], [f32; 3])> {
        let areas: Vec<f32> = self.tris.iter().map(|t| 0.5 * dot(cross(sub(t[1], t[0]), sub(t[2], t[0])), cross(sub(t[1], t[0]), sub(t[2], t[0]))).sqrt()).collect();
        let total: f32 = areas.iter().sum();
        if total <= 0.0 || n == 0 { return vec![]; }
        let mut cum = Vec::with_capacity(areas.len());
        let mut acc = 0.0;
        for a in &areas { acc += a / total; cum.push(acc); }
        let unit = |k: u32| (crate::morph::hash(k) >> 8) as f32 / 16_777_216.0;
        (0..n).map(|i| {
            let k = seed.wrapping_mul(0x9e37_79b9) ^ (i as u32).wrapping_mul(3);
            // A stratified pick along the cumulative areas, so even small counts cover every face.
            let r = ((i as f32 + unit(k)) / n as f32).min(0.999_999);
            let ti = cum.partition_point(|&c| c < r).min(self.tris.len() - 1);
            let t = &self.tris[ti];
            let (mut a, mut b) = (unit(k ^ 0x51ed_270b), unit(k ^ 0x2c1b_3c6d));
            if a + b > 1.0 { a = 1.0 - a; b = 1.0 - b; }
            let p = add(t[0], add(mulf(sub(t[1], t[0]), a), mulf(sub(t[2], t[0]), b)));
            (p, Self::normal(t))
        }).collect()
    }
}

/// Turn every triangle so it faces away from `inside(its centre)`.
fn orient(mut m: Mesh, inside: impl Fn([f32; 3]) -> [f32; 3]) -> Mesh {
    for t in &mut m.tris {
        let c = mulf(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
        if dot(Mesh::normal(t), sub(c, inside(c))) < 0.0 { t.swap(1, 2); }
    }
    m
}
/// A unit cube centred at the origin.
pub fn cube() -> Mesh {
    let p = |x: f32, y: f32, z: f32| [x * 0.5, y * 0.5, z * 0.5];
    let mut m = Mesh::default();
    // Each face: an outward axis and two axes along it, so (u × v) points out.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]), ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]), ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    for (n, u, v) in faces {
        let c = |a: f32, b: f32| p(n[0] + u[0] * a + v[0] * b, n[1] + u[1] * a + v[1] * b, n[2] + u[2] * a + v[2] * b);
        let (q0, q1, q2, q3) = (c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0));
        m.tris.push([q0, q1, q2]);
        m.tris.push([q0, q2, q3]);
    }
    orient(m, |_| [0.0; 3])
}

/// A sphere of radius 0.5 in `seg` longitudes and `rings` latitudes.
pub fn uv_sphere(seg: u32, rings: u32) -> Mesh {
    let (seg, rings) = (seg.max(3), rings.max(2));
    let at = |i: u32, j: u32| {
        let (th, ph) = (i as f32 / seg as f32 * std::f32::consts::TAU, j as f32 / rings as f32 * std::f32::consts::PI);
        [0.5 * ph.sin() * th.cos(), 0.5 * ph.cos(), -0.5 * ph.sin() * th.sin()]
    };
    let mut m = Mesh::default();
    for j in 0..rings { for i in 0..seg {
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
        if j > 0 { m.tris.push([a, d, b]); }
        if j + 1 < rings { m.tris.push([b, d, c]); }
    } }
    orient(m, |_| [0.0; 3])
}

/// An icosahedron of radius 0.5, each face split into four `level` times (20 × 4^level faces).
pub fn icosphere(level: u32) -> Mesh {
    let t = (1.0 + 5.0f32.sqrt()) / 2.0;
    let mut v: Vec<[f32; 3]> = [[-1.0, t, 0.0], [1.0, t, 0.0], [-1.0, -t, 0.0], [1.0, -t, 0.0], [0.0, -1.0, t], [0.0, 1.0, t], [0.0, -1.0, -t], [0.0, 1.0, -t],
        [t, 0.0, -1.0], [t, 0.0, 1.0], [-t, 0.0, -1.0], [-t, 0.0, 1.0]].iter().map(|p| norm(*p)).collect();
    let mut f: Vec<[u32; 3]> = vec![[0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11], [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9], [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1]];
    for _ in 0..level.min(5) {
        let mut mid = std::collections::HashMap::new();
        let mut half = |a: u32, b: u32, v: &mut Vec<[f32; 3]>| *mid.entry((a.min(b), a.max(b))).or_insert_with(|| { v.push(norm(add(v[a as usize], v[b as usize]))); v.len() as u32 - 1 });
        let mut g = Vec::with_capacity(f.len() * 4);
        for [a, b, c] in f {
            let (ab, bc, ca) = (half(a, b, &mut v), half(b, c, &mut v), half(c, a, &mut v));
            g.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        f = g;
    }
    let v: Vec<[f32; 3]> = v.into_iter().map(|p| mulf(p, 0.5)).collect();
    orient(Mesh::indexed(&v, &f), |_| [0.0; 3])
}

/// A torus around y: ring radius `major`, tube radius `minor`.
pub fn torus(major: f32, minor: f32, seg: u32, sides: u32) -> Mesh {
    let (seg, sides) = (seg.max(3), sides.max(3));
    let at = |i: u32, j: u32| {
        let (u, v) = (i as f32 / seg as f32 * std::f32::consts::TAU, j as f32 / sides as f32 * std::f32::consts::TAU);
        let r = major + minor * v.cos();
        [r * u.cos(), minor * v.sin(), -r * u.sin()]
    };
    let mut m = Mesh::default();
    for i in 0..seg { for j in 0..sides {
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
        m.tris.push([a, b, c]);
        m.tris.push([a, c, d]);
    } }
    // Out is away from the tube's centre line.
    orient(m, |c| { let l = (c[0] * c[0] + c[2] * c[2]).sqrt().max(1e-6); [c[0] / l * major, 0.0, c[2] / l * major] })
}

/// A cylinder (`top` = bottom radius) or cone (`top` = 0) around y from −0.5 to 0.5, bottom radius 0.5.
fn lathe(seg: u32, top: f32) -> Mesh {
    let seg = seg.max(3);
    let ring = |i: u32, r: f32, y: f32| { let a = i as f32 / seg as f32 * std::f32::consts::TAU; [r * a.cos(), y, -r * a.sin()] };
    let mut m = Mesh::default();
    for i in 0..seg {
        let (b0, b1) = (ring(i, 0.5, -0.5), ring(i + 1, 0.5, -0.5));
        let (t0, t1) = (ring(i, top, 0.5), ring(i + 1, top, 0.5));
        if top > 0.0 { m.tris.push([b0, b1, t1]); m.tris.push([b0, t1, t0]); m.tris.push([[0.0, 0.5, 0.0], t0, t1]); }
        else { m.tris.push([b0, b1, [0.0, 0.5, 0.0]]); }
        m.tris.push([[0.0, -0.5, 0.0], b1, b0]);
    }
    orient(m, |_| [0.0; 3])
}
pub fn cylinder(seg: u32) -> Mesh { lathe(seg, 0.5) }
pub fn cone(seg: u32) -> Mesh { lathe(seg, 0.0) }
/// A unit square in x–z, facing up (+y).
pub fn plane() -> Mesh {
    let (a, b, c, d) = ([-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [0.5, 0.0, -0.5], [-0.5, 0.0, -0.5]);
    orient(Mesh { tris: vec![[a, b, c], [a, c, d]], normals: vec![] }, |c| [c[0], -1.0, c[2]])
}

/// A closed 2D profile (x, y, counter-clockwise) extruded along z from −depth/2 to depth/2: the two
/// caps and the side walls. The caps are triangulated as a fan from `centre` (the profile must be
/// star-shaped around it, as gears, rings' spokes and cams are). Normals: the caps flat, the walls smooth
/// where consecutive wall faces turn by less than `crease_deg`.
pub fn extrude(profile: &[[f32; 2]], centre: [f32; 2], depth: f32, crease_deg: f32) -> Mesh {
    let n = profile.len();
    let (z0, z1) = (-depth * 0.5, depth * 0.5);
    let mut m = Mesh::default();
    let mut walls = Mesh::default();
    for i in 0..n {
        let (a, b) = (profile[i], profile[(i + 1) % n]);
        // Caps: front (+z) counter-clockwise, back reversed.
        m.tris.push([[centre[0], centre[1], z1], [a[0], a[1], z1], [b[0], b[1], z1]]);
        m.tris.push([[centre[0], centre[1], z0], [b[0], b[1], z0], [a[0], a[1], z0]]);
        walls.tris.push([[a[0], a[1], z0], [b[0], b[1], z0], [b[0], b[1], z1]]);
        walls.tris.push([[a[0], a[1], z0], [b[0], b[1], z1], [a[0], a[1], z1]]);
    }
    let caps = Mesh { normals: m.tris.iter().map(|t| { let f = Mesh::normal(t); [f, f, f] }).collect(), tris: m.tris };
    caps.merged(&walls.creased(crease_deg))
}
/// A ring profile extruded (a washer, a hub, a bearing): outer and inner radius, `seg` sides.
pub fn tube(outer: f32, inner: f32, depth: f32, seg: u32) -> Mesh {
    let seg = seg.max(6);
    let pt = |r: f32, i: u32| { let a = i as f32 / seg as f32 * std::f32::consts::TAU; [r * a.cos(), r * a.sin()] };
    let (z0, z1) = (-depth * 0.5, depth * 0.5);
    let mut caps = Mesh::default();
    let mut outer_w = Mesh::default();
    let mut inner_w = Mesh::default();
    for i in 0..seg {
        let (o0, o1, i0, i1) = (pt(outer, i), pt(outer, i + 1), pt(inner, i), pt(inner, i + 1));
        let v = |p: [f32; 2], z: f32| [p[0], p[1], z];
        caps.tris.push([v(i0, z1), v(o0, z1), v(o1, z1)]); caps.tris.push([v(i0, z1), v(o1, z1), v(i1, z1)]);
        caps.tris.push([v(i0, z0), v(o1, z0), v(o0, z0)]); caps.tris.push([v(i0, z0), v(i1, z0), v(o1, z0)]);
        outer_w.tris.push([v(o0, z0), v(o1, z0), v(o1, z1)]); outer_w.tris.push([v(o0, z0), v(o1, z1), v(o0, z1)]);
        inner_w.tris.push([v(i0, z0), v(i1, z1), v(i1, z0)]); inner_w.tris.push([v(i0, z0), v(i0, z1), v(i1, z1)]);
    }
    let caps = Mesh { normals: caps.tris.iter().map(|t| { let f = Mesh::normal(t); [f, f, f] }).collect(), tris: caps.tris };
    caps.merged(&outer_w.creased(40.0)).merged(&inner_w.creased(40.0))
}
/// A spur gear around z: `teeth` teeth on a pitch circle of radius `r` (module = 2r / teeth), with
/// involute-like flanks (each tooth a tapered trapezoid with rounded corners), extruded `depth`, around a
/// bore of radius `bore`. The tooth tips and roots are creases, the flanks smooth.
pub fn gear(teeth: u32, r: f32, depth: f32, bore: f32) -> Mesh {
    use std::f32::consts::TAU;
    let teeth = teeth.max(6);
    let module = 2.0 * r / teeth as f32;
    let (ra, rf) = (r + 0.9 * module, (r - 1.1 * module).max(bore + module * 0.5));
    let mut prof: Vec<[f32; 2]> = vec![];
    let step = TAU / teeth as f32;
    // One tooth, centred on its angle: the root land, the flank rising (an involute-like curve that is
    // steep near the root and rounds over toward the tip), the tip land, the other flank down. In
    // fractions of the tooth pitch: half a pitch is the tooth at the pitch circle, as cut gears have.
    for k in 0..teeth {
        let a0 = k as f32 * step;
        let at = |frac: f32, rad: f32| { let a = a0 + frac * step; [rad * a.cos(), rad * a.sin()] };
        for j in 0..2 { prof.push(at(-0.5 + j as f32 * 0.15, rf)); }
        for j in 0..=5 { let u = j as f32 / 5.0; let rad = rf + (ra - rf) * u; prof.push(at(-0.22 + 0.11 * u * u, rad)); }
        prof.push(at(0.0, ra));
        for j in (0..=5).rev() { let u = j as f32 / 5.0; let rad = rf + (ra - rf) * u; prof.push(at(0.22 - 0.11 * u * u, rad)); }
        prof.push(at(0.35, rf));
    }
    extrude_bored(&prof, bore, depth, 25.0)
}
/// A profile that is star-shaped around the origin, extruded along z with a round bore of radius
/// `bore` through it: the caps join the outline to the bore along each outline point's own angle.
pub fn extrude_bored(profile: &[[f32; 2]], bore: f32, depth: f32, crease_deg: f32) -> Mesh {
    let n = profile.len();
    let (z0, z1) = (-depth * 0.5, depth * 0.5);
    let inner = |p: [f32; 2]| { let l = (p[0] * p[0] + p[1] * p[1]).sqrt().max(1e-6); [p[0] / l * bore, p[1] / l * bore] };
    let v = |p: [f32; 2], z: f32| [p[0], p[1], z];
    let (mut caps, mut walls, mut hole) = (Mesh::default(), Mesh::default(), Mesh::default());
    for i in 0..n {
        let (a, b) = (profile[i], profile[(i + 1) % n]);
        let (ia, ib) = (inner(a), inner(b));
        caps.tris.push([v(ia, z1), v(a, z1), v(b, z1)]); caps.tris.push([v(ia, z1), v(b, z1), v(ib, z1)]);
        caps.tris.push([v(ia, z0), v(b, z0), v(a, z0)]); caps.tris.push([v(ia, z0), v(ib, z0), v(b, z0)]);
        walls.tris.push([v(a, z0), v(b, z0), v(b, z1)]); walls.tris.push([v(a, z0), v(b, z1), v(a, z1)]);
        hole.tris.push([v(ia, z0), v(ib, z1), v(ib, z0)]); hole.tris.push([v(ia, z0), v(ia, z1), v(ib, z1)]);
    }
    let caps = Mesh { normals: caps.tris.iter().map(|t| { let f = Mesh::normal(t); [f, f, f] }).collect(), tris: caps.tris };
    caps.merged(&walls.creased(crease_deg)).merged(&hole.creased(40.0))
}
/// A box of size (x, y, z) centred at the origin, every edge a crease.
pub fn block(x: f32, y: f32, z: f32) -> Mesh { cube().transformed(&Mat4::scale(x, y, z)).creased(30.0) }
/// A cylinder around y, radius and height, with smooth sides and creased rims.
pub fn rod(r: f32, h: f32, seg: u32) -> Mesh { cylinder(seg).transformed(&Mat4::scale(r * 2.0, h, r * 2.0)).creased(40.0) }

/// Which mesh of a `Library`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MeshId(pub u32);
impl MeshId {
    pub const CUBE: MeshId = MeshId(0);
    pub const SPHERE: MeshId = MeshId(1);
    /// Icosahedra with 0 to 3 subdivisions (20, 80, 320, 1280 faces).
    pub const ICO: [MeshId; 4] = [MeshId(2), MeshId(3), MeshId(4), MeshId(5)];
    pub const TORUS: MeshId = MeshId(6);
    pub const CYLINDER: MeshId = MeshId(7);
    pub const CONE: MeshId = MeshId(8);
    pub const PLANE: MeshId = MeshId(9);
}
/// The built-in meshes' names, in id order (for choices in the gallery).
pub const BUILTIN: [&str; 10] = ["立方体", "经纬球", "二十面体", "二十面体 1", "二十面体 2", "二十面体 3", "环面", "圆柱", "圆锥", "平面"];

/// Every mesh the runtime knows. The built-ins have fixed ids; `add` gives the next id. `generation`
/// changes when a mesh is added, so the renderer knows to upload again.
#[derive(Clone, Debug)]
pub struct Library { meshes: Vec<Mesh>, pub generation: u32 }
impl Default for Library {
    fn default() -> Self {
        let meshes = vec![cube(), uv_sphere(24, 14), icosphere(0), icosphere(1), icosphere(2), icosphere(3), torus(0.34, 0.15, 28, 12), cylinder(20), cone(20), plane()];
        Self { meshes, generation: 1 }
    }
}
impl Library {
    pub fn add(&mut self, m: Mesh) -> MeshId { self.meshes.push(m); self.generation += 1; MeshId(self.meshes.len() as u32 - 1) }
    pub fn get(&self, id: MeshId) -> Option<&Mesh> { self.meshes.get(id.0 as usize) }
    pub fn all(&self) -> &[Mesh] { &self.meshes }
}

// ── What a scene3d element holds ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Vertical field of view, degrees.
    Perspective { fov: f32 },
    /// Height of the view, world units.
    Orthographic { height: f32 },
}

/// One camera that orbits a target: `yaw` around y and `pitch` above the horizon (degrees), `distance`
/// from the target; `near` and `far` clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera { pub projection: Projection, pub yaw: f32, pub pitch: f32, pub distance: f32, pub target: [f32; 3], pub near: f32, pub far: f32 }
impl Default for Camera {
    fn default() -> Self { Self { projection: Projection::Perspective { fov: 35.0 }, yaw: 30.0, pitch: 22.0, distance: 3.2, target: [0.0; 3], near: 0.1, far: 60.0 } }
}
impl Camera {
    pub fn eye(&self) -> [f32; 3] {
        let (y, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        add(self.target, mulf([p.cos() * y.sin(), p.sin(), p.cos() * y.cos()], self.distance))
    }
    /// Right, up and back (the camera looks along −back), world space.
    pub fn basis(&self) -> [[f32; 3]; 3] {
        let back = norm(sub(self.eye(), self.target));
        let up0 = if back[1].abs() > 0.999 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
        let right = norm(cross(up0, back));
        [right, cross(back, right), back]
    }
    pub fn view(&self) -> Mat4 {
        let [r, u, b] = self.basis();
        let e = self.eye();
        Mat4([r[0], u[0], b[0], 0.0, r[1], u[1], b[1], 0.0, r[2], u[2], b[2], 0.0, -dot(r, e), -dot(u, e), -dot(b, e), 1.0])
    }
    /// Projection to wgpu's clip space (z from 0 to 1) for a view `aspect` wide per unit of height.
    pub fn proj(&self, aspect: f32) -> Mat4 {
        let (n, f) = (self.near.max(1e-3), self.far.max(self.near + 1e-3));
        match self.projection {
            Projection::Perspective { fov } => {
                let t = 1.0 / (fov.clamp(1.0, 170.0).to_radians() * 0.5).tan();
                Mat4([t / aspect, 0.0, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, 0.0, f / (n - f), -1.0, 0.0, 0.0, n * f / (n - f), 0.0])
            }
            Projection::Orthographic { height } => {
                let (h, w) = (height.max(1e-3) * 0.5, height.max(1e-3) * 0.5 * aspect);
                Mat4([1.0 / w, 0.0, 0.0, 0.0, 0.0, 1.0 / h, 0.0, 0.0, 0.0, 0.0, 1.0 / (n - f), 0.0, 0.0, 0.0, n / (n - f), 1.0])
            }
        }
    }
    /// The ray through a point of the view (fractions 0..1 from the top left): origin and direction.
    pub fn ray(&self, fx: f32, fy: f32, aspect: f32) -> ([f32; 3], [f32; 3]) {
        let [r, u, b] = self.basis();
        let (nx, ny) = (fx * 2.0 - 1.0, 1.0 - fy * 2.0);
        match self.projection {
            Projection::Perspective { fov } => {
                let t = (fov.clamp(1.0, 170.0).to_radians() * 0.5).tan();
                (self.eye(), norm(add(add(mulf(r, nx * t * aspect), mulf(u, ny * t)), mulf(b, -1.0))))
            }
            Projection::Orthographic { height } => {
                let (h, w) = (height * 0.5, height * 0.5 * aspect);
                (add(self.eye(), add(mulf(r, nx * w), mulf(u, ny * h))), mulf(b, -1.0))
            }
        }
    }
}

/// One directional light: the direction towards it (world space) and how bright faces turned away
/// still are (0..1), so the darkest face is never the page ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light { pub dir: [f32; 3], pub ambient: f32 }
impl Default for Light { fn default() -> Self { Self { dir: [-0.45, 0.8, 0.55], ambient: 0.28 } } }
impl Light {
    /// From two angles (degrees): around y, and above the horizon.
    pub fn from_angles(yaw: f32, pitch: f32, ambient: f32) -> Self {
        let (y, p) = (yaw.to_radians(), pitch.to_radians());
        Self { dir: [p.cos() * y.sin(), p.sin(), p.cos() * y.cos()], ambient }
    }
}

/// The mesh pass's numbers (DESIGN.md §1.18).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mesh3d {
    /// The off-screen target's size as a fraction of the element's device size (nearest upsample for
    /// colour; the nearness is filtered). 0.25–2: above 1 the silhouette and the drawn outline are
    /// supersampled, an edge on a pixel's share of it (its fill stays one texel's colour).
    pub resolution_scale: f32,
    /// A depth step between neighbouring texels larger than this fraction of the depth is an edge.
    pub edge_depth: f32,
    /// Normals of neighbouring texels further apart than this many degrees are an edge.
    pub edge_normal: f32,
    /// 0: shaded by the light (ambient + lambert); 2: two flat tones, lit and shadow, with a hard
    /// terminator where the light falls below `terminator` (0..1 of the lambert term).
    pub tones: f32,
    pub terminator: f32,
    /// A drawn outline on the silhouette and the creases, logical px (0: none; the materials still see
    /// the edges in the nearness).
    pub outline: f32,
    /// A rectangle of the plane z = 0 (world units, `[x0, y0, x1, y1]`) that an orthographic view puts on
    /// whole device pixels, at whatever size and scale the element is drawn: each of its edges moves to
    /// the nearest (under half a pixel; the view is scaled and shifted that little along each axis), so a
    /// flat face there has its outline on one whole pixel on every side. `None`: as the camera puts it.
    pub snap: Option<[f32; 4]>,
}
impl Default for Mesh3d { fn default() -> Self { Self { resolution_scale: 0.5, edge_depth: 0.04, edge_normal: 15.0, tones: 0.0, terminator: 0.3, outline: 0.0, snap: None } } }

/// Copies of one mesh, drawn in one call: every transform is one copy. `id` names the group in picks.
/// `outline`: the grey its drawn outline (`Mesh3d::outline`) takes, by its tone (`None`: the ink).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshInstances { pub mesh: MeshId, pub transforms: Vec<Mat4>, pub paint: Paint, pub id: u32, pub outline: Option<Paint> }
impl MeshInstances {
    pub fn one(mesh: MeshId, transform: Mat4, paint: Paint, id: u32) -> Self { Self { mesh, transforms: vec![transform], paint, id, outline: None } }
    /// Its outline in `p`'s grey (its tone; the accent is not drawn in an outline).
    pub fn outlined(mut self, p: Paint) -> Self { self.outline = Some(p); self }
}

/// What a `scene3d` element draws: meshes with transforms, one camera, one light (DESIGN.md §1.18).
/// `picture`: draw it as a picture with this `Markr` slot (its marks, its own ground) instead of
/// through the material.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene3d { pub items: Vec<MeshInstances>, pub camera: Camera, pub light: Light, pub spec: Mesh3d, pub picture: Option<u8> }
impl Scene3d {
    /// The nearest and farthest view depth any copy can reach (its centre ± its bounding radius), for
    /// marks that follow depth (`style::Marks`).
    pub fn depth_range(&self, lib: &Library) -> (f32, f32) {
        let eye = self.camera.eye();
        let fwd = mulf(self.camera.basis()[2], -1.0);
        let (mut a, mut b) = (f32::INFINITY, f32::NEG_INFINITY);
        for it in &self.items {
            let r0 = lib.get(it.mesh).map_or(1.0, Mesh::radius);
            for m in &it.transforms {
                let c = [m.0[12], m.0[13], m.0[14]];
                let s = [m.0[0], m.0[1], m.0[2]].iter().map(|v| v * v).sum::<f32>().sqrt().max([m.0[4], m.0[5], m.0[6]].iter().map(|v| v * v).sum::<f32>().sqrt()).max([m.0[8], m.0[9], m.0[10]].iter().map(|v| v * v).sum::<f32>().sqrt());
                let z = dot(sub(c, eye), fwd);
                a = a.min(z - r0 * s);
                b = b.max(z + r0 * s);
            }
        }
        if a.is_finite() && b > a { (a.max(self.camera.near), b) } else { (self.camera.near, self.camera.far) }
    }
    /// One mesh at the origin, in ink, with the default camera and light.
    pub fn single(mesh: MeshId) -> Self {
        Self { items: vec![MeshInstances::one(mesh, Mat4::IDENTITY, Paint::INK, 1)], camera: Camera::default(), light: Light::default(), spec: Mesh3d::default(), picture: None }
    }
    /// How many copies are drawn in all.
    pub fn count(&self) -> usize { self.items.iter().map(|i| i.transforms.len()).sum() }
    /// A hash of everything that changes the picture (for the renderer's cache).
    pub fn fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut f = |v: f32| v.to_bits().hash(&mut h);
        for it in &self.items {
            f(it.mesh.0 as f32); f(it.paint.tone); f(it.paint.accent); f(it.paint.hue as f32); f(it.id as f32); f(it.outline.map_or(-1.0, |p| p.tone));
            for t in &it.transforms { for v in t.0 { f(v); } }
        }
        let c = &self.camera;
        match c.projection { Projection::Perspective { fov } => { f(0.0); f(fov) } Projection::Orthographic { height } => { f(1.0); f(height) } }
        for v in [c.yaw, c.pitch, c.distance, c.target[0], c.target[1], c.target[2], c.near, c.far, self.light.dir[0], self.light.dir[1], self.light.dir[2], self.light.ambient,
                  self.spec.resolution_scale, self.spec.edge_depth, self.spec.edge_normal, self.picture.map_or(-1.0, |p| p as f32)] { f(v); }
        for v in self.spec.snap.unwrap_or([f32::NAN; 4]) { f(v); }
        h.finish()
    }
    /// The camera's projection for a view `rw` × `rh` device px, with `Mesh3d::snap`'s rectangle on
    /// whole device pixels (an orthographic camera; any other as it is).
    pub fn proj_in(&self, rw: f32, rh: f32) -> Mat4 {
        let p = self.camera.proj(rw / rh);
        let (Some([x0, y0, x1, y1]), Projection::Orthographic { .. }) = (self.spec.snap, self.camera.projection) else { return p };
        let m = p.mul(&self.camera.view()).0;
        let ndc = |x: f32, y: f32| (m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]);
        let ((ax, ay), (bx, by)) = (ndc(x0, y0), ndc(x1, y1));
        // Along one axis (`u` from -1 to 1 across `n` px): the scale and shift that move both edges to
        // the nearest whole px.
        let axis = |u0: f32, u1: f32, n: f32| {
            let (d0, d1) = ((u0 + 1.0) * 0.5 * n, (u1 + 1.0) * 0.5 * n);
            let (r0, r1) = (d0.round(), d1.round());
            if (d1 - d0).abs() < 1e-3 || r0 == r1 { return (1.0, 0.0); }
            let s = (r1 - r0) / (d1 - d0);
            (s, s - 1.0 + 2.0 * (r0 - s * d0) / n)
        };
        let (sx, ox) = axis(ax, bx, rw);
        // Down the view, from its top: y flipped.
        let (sy, oy) = axis(-ay, -by, rh);
        Mat4([sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, ox, -oy, 0.0, 1.0]).mul(&p)
    }
    /// The copy under a point of the view (fractions 0..1 of the element from its top left), nearest
    /// first: the same transforms the GPU draws, intersected on the CPU (Möller–Trumbore).
    pub fn pick(&self, lib: &Library, fx: f32, fy: f32, aspect: f32) -> Option<MeshHit> {
        let (o, d) = self.camera.ray(fx, fy, aspect);
        self.cast(lib, o, d).map(|(_, mut h)| { h.uv = [fx, fy]; h })
    }
    /// `n` points on the surfaces of every copy, area-weighted over the whole scene (world areas, so a
    /// copy drawn twice as large gets four times the points); deterministic for a seed. Particles
    /// emitted by a mesh start (or land) on these (DESIGN.md §1.10).
    pub fn surface(&self, lib: &Library, n: usize, seed: u32) -> Vec<SurfacePoint> {
        let mut copies: Vec<(u32, u32, f32)> = vec![];
        for (gi, it) in self.items.iter().enumerate() {
            let Some(mesh) = lib.get(it.mesh) else { continue };
            for (ci, m) in it.transforms.iter().enumerate() {
                let area: f32 = mesh.tris.iter().map(|t| { let (a, b, c) = (m.point(t[0]), m.point(t[1]), m.point(t[2])); 0.5 * dot(cross(sub(b, a), sub(c, a)), cross(sub(b, a), sub(c, a))).sqrt() }).sum();
                if area > 0.0 { copies.push((gi as u32, ci as u32, area)); }
            }
        }
        let total: f32 = copies.iter().map(|c| c.2).sum();
        if total <= 0.0 { return vec![]; }
        let mut out = Vec::with_capacity(n);
        let mut acc = 0.0f32;
        for (gi, ci, area) in copies {
            let from = (acc / total * n as f32).round() as usize;
            acc += area;
            let to = (acc / total * n as f32).round() as usize;
            let Some(mesh) = lib.get(self.items[gi as usize].mesh) else { continue };
            out.extend(mesh.sample(to - from, seed ^ gi.wrapping_mul(0x85eb_ca6b) ^ ci.wrapping_mul(0xc2b2_ae35)).into_iter().map(|(p, _)| SurfacePoint { item: gi, copy: ci, p }));
        }
        out
    }
    /// Where a surface point is in the view now (fractions 0..1 of the element from its top left), and
    /// whether it is seen: the nearest surface along the ray through it is the point itself (a depth
    /// test against the mesh, on the CPU). `None` when the copy no longer exists or the point is
    /// behind the camera.
    pub fn project(&self, lib: &Library, sp: &SurfacePoint, aspect: f32) -> Option<(f32, f32, bool)> {
        let m = self.items.get(sp.item as usize)?.transforms.get(sp.copy as usize)?;
        let w = m.point(sp.p);
        let c = self.camera.proj(aspect).mul(&self.camera.view());
        let q = &c.0;
        let clip = [q[0] * w[0] + q[4] * w[1] + q[8] * w[2] + q[12], q[1] * w[0] + q[5] * w[1] + q[9] * w[2] + q[13], q[3] * w[0] + q[7] * w[1] + q[11] * w[2] + q[15]];
        if clip[2] <= 1e-6 { return None; }
        let (fx, fy) = ((clip[0] / clip[2] + 1.0) * 0.5, (1.0 - clip[1] / clip[2]) * 0.5);
        let (o, d) = self.camera.ray(fx, fy, aspect);
        let along = dot(sub(w, o), d);
        let seen = match self.cast(lib, o, d) { Some((t, _)) => t >= along - 1e-3 * along.abs().max(1.0), None => true };
        Some((fx, fy, seen))
    }
    /// The nearest copy along a ray: its distance and which copy.
    fn cast(&self, lib: &Library, o: [f32; 3], d: [f32; 3]) -> Option<(f32, MeshHit)> {
        let mut best: Option<(f32, MeshHit)> = None;
        for it in &self.items {
            let Some(mesh) = lib.get(it.mesh) else { continue };
            let r = mesh.radius();
            for (k, m) in it.transforms.iter().enumerate() {
                // A ball around the copy first: most rays miss most copies.
                let c = m.point([0.0; 3]);
                let s = dot(m.vector([1.0, 0.0, 0.0]), m.vector([1.0, 0.0, 0.0])).max(dot(m.vector([0.0, 1.0, 0.0]), m.vector([0.0, 1.0, 0.0]))).max(dot(m.vector([0.0, 0.0, 1.0]), m.vector([0.0, 0.0, 1.0]))).sqrt();
                let oc = sub(o, c);
                let (b, cc) = (dot(oc, d), dot(oc, oc) - (r * s) * (r * s));
                if b * b - cc < 0.0 { continue; }
                for t in &mesh.tris {
                    let (a, b2, c2) = (m.point(t[0]), m.point(t[1]), m.point(t[2]));
                    if let Some(dist) = ray_tri(o, d, a, b2, c2) && best.is_none_or(|(bd, _)| dist < bd) {
                        let mut n = Mesh::normal(&[a, b2, c2]);
                        if dot(n, d) > 0.0 { n = mulf(n, -1.0); }
                        best = Some((dist, MeshHit { object: it.id, instance: k as u32, point: add(o, mulf(d, dist)), normal: n, t: dist, uv: [0.0; 2] }));
                    }
                }
            }
        }
        best
    }
}

/// A point on one copy's surface: the group (index in `items`), the copy, and the point in the mesh's
/// own space, so it follows the copy as it turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePoint { pub item: u32, pub copy: u32, pub p: [f32; 3] }

/// Which copy a pick found: the group's `id` and the copy's index in its transforms; where the ray met
/// it (world space), the surface's normal there (facing the ray), how far along the ray (world units),
/// and the point of the view it was cast through (fractions 0..1 of the element from its top left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshHit { pub object: u32, pub instance: u32, pub point: [f32; 3], pub normal: [f32; 3], pub t: f32, pub uv: [f32; 2] }
impl MeshHit {
    /// The same copy (ignoring where on it).
    pub fn same(&self, o: &MeshHit) -> bool { self.object == o.object && self.instance == o.instance }
}

/// What a `scene3d` element hears: the pointer came onto (or off) a copy, or pressed one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick { Hover(Option<MeshHit>), Press(Option<MeshHit>) }

pub(crate) fn ray_tri(o: [f32; 3], d: [f32; 3], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Option<f32> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-9 { return None; }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 { return None; }
    let t = dot(e2, q) * inv;
    (t > 1e-5).then_some(t)
}

// ── glTF 2.0, a small subset ─────────────────────────────────────────────────────────────────────

/// Read a glTF 2.0 file (`.gltf` with its buffers inline as `data:` URIs, or `.glb`) into one mesh:
/// every triangle primitive of every node of the default scene, in world space (node transforms,
/// matrix or TRS, applied down the hierarchy). Positions and indices are read; normals, where given,
/// decide which way a triangle faces. No skins, morph targets, materials or textures.
#[cfg(feature = "gltf")]
pub fn load_gltf(bytes: &[u8]) -> Result<Mesh, MeshError> {
    let (json, bin): (serde_json::Value, Option<&[u8]>) = if bytes.starts_with(b"glTF") {
        let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(MeshError::Truncated);
        let jlen = u32_at(12)? as usize;
        let js = bytes.get(20..20 + jlen).ok_or(MeshError::Truncated)?;
        let json = serde_json::from_slice(js).map_err(|e| MeshError::Json(e.to_string()))?;
        let at = 20 + jlen;
        let bin = if bytes.len() >= at + 8 { let blen = u32_at(at)? as usize; Some(bytes.get(at + 8..at + 8 + blen).ok_or(MeshError::Truncated)?) } else { None };
        (json, bin)
    } else {
        (serde_json::from_slice(bytes).map_err(|e| MeshError::Json(e.to_string()))?, None)
    };
    let arr = |v: &serde_json::Value, k: &str| v.get(k).and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let num = |v: &serde_json::Value, k: &str| v.get(k).and_then(|a| a.as_u64()).map(|n| n as usize);
    let buffers: Vec<Vec<u8>> = arr(&json, "buffers").iter().map(|b| match b.get("uri").and_then(|u| u.as_str()) {
        Some(uri) => {
            let data = uri.split_once(";base64,").map(|(_, d)| d).ok_or_else(|| MeshError::ExternalBuffer(uri.chars().take(60).collect()))?;
            base64(data)
        }
        None => bin.map(<[u8]>::to_vec).ok_or(MeshError::Truncated),
    }).collect::<Result<_, _>>()?;
    let views = arr(&json, "bufferViews");
    let accessors = arr(&json, "accessors");
    // The floats or integers of one accessor, `width` per element.
    let read = |ai: usize| -> Result<(Vec<f64>, usize), MeshError> {
        let a = accessors.get(ai).ok_or(MeshError::Missing("accessor"))?;
        let count = num(a, "count").ok_or(MeshError::Missing("accessor count"))?;
        let width = match a.get("type").and_then(|t| t.as_str()) { Some("SCALAR") => 1, Some("VEC2") => 2, Some("VEC3") => 3, Some("VEC4") => 4, _ => return Err(MeshError::Unsupported("accessor type")) };
        let ct = num(a, "componentType").ok_or(MeshError::Missing("componentType"))?;
        let size = match ct { 5121 => 1, 5123 => 2, 5125 | 5126 => 4, _ => return Err(MeshError::Unsupported("component type")) };
        let v = views.get(num(a, "bufferView").ok_or(MeshError::Missing("bufferView"))?).ok_or(MeshError::Missing("bufferView"))?;
        let buf = buffers.get(num(v, "buffer").unwrap_or(0)).ok_or(MeshError::Missing("buffer"))?;
        let start = num(v, "byteOffset").unwrap_or(0) + num(a, "byteOffset").unwrap_or(0);
        let stride = num(v, "byteStride").unwrap_or(size * width);
        let mut out = Vec::with_capacity(count * width);
        for e in 0..count { for c in 0..width {
            let o = start + e * stride + c * size;
            let b = buf.get(o..o + size).ok_or(MeshError::Truncated)?;
            out.push(match ct { 5121 => b[0] as f64, 5123 => u16::from_le_bytes([b[0], b[1]]) as f64, 5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64, _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 });
        } }
        Ok((out, width))
    };
    let meshes = arr(&json, "meshes");
    let nodes = arr(&json, "nodes");
    let scene = json.get("scene").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
    let roots: Vec<usize> = arr(&json, "scenes").get(scene).map(|s| arr(s, "nodes").iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect())
        .unwrap_or_else(|| (0..nodes.len()).collect());
    let f3 = |v: &serde_json::Value, k: &str, d: [f32; 3]| v.get(k).and_then(|a| a.as_array()).filter(|a| a.len() == 3).map_or(d, |a| std::array::from_fn(|i| a[i].as_f64().unwrap_or(0.0) as f32));
    let mut out = Mesh::default();
    let mut stack: Vec<(usize, Mat4, u32)> = roots.into_iter().map(|r| (r, Mat4::IDENTITY, 0)).collect();
    while let Some((ni, parent, depth)) = stack.pop() {
        if depth > 64 { return Err(MeshError::Unsupported("node hierarchy deeper than 64")); }
        let n = nodes.get(ni).ok_or(MeshError::Missing("node"))?;
        let local = match n.get("matrix").and_then(|m| m.as_array()).filter(|m| m.len() == 16) {
            Some(m) => Mat4(std::array::from_fn(|i| m[i].as_f64().unwrap_or(0.0) as f32)),
            None => {
                let q = n.get("rotation").and_then(|a| a.as_array()).filter(|a| a.len() == 4).map_or([0.0, 0.0, 0.0, 1.0], |a| std::array::from_fn(|i| a[i].as_f64().unwrap_or(0.0) as f32));
                Mat4::trs(f3(n, "translation", [0.0; 3]), q, f3(n, "scale", [1.0; 3]))
            }
        };
        let world = parent.mul(&local);
        if let Some(mi) = num(n, "mesh") {
            let m = meshes.get(mi).ok_or(MeshError::Missing("mesh"))?;
            for p in arr(m, "primitives") {
                if num(&p, "mode").unwrap_or(4) != 4 { continue; }
                let attrs = p.get("attributes").ok_or(MeshError::Missing("attributes"))?;
                let (pos, w) = read(num(attrs, "POSITION").ok_or(MeshError::Missing("POSITION"))?)?;
                if w != 3 { return Err(MeshError::Unsupported("POSITION that is not VEC3")); }
                let nrm = match num(attrs, "NORMAL") { Some(a) => Some(read(a)?.0), None => None };
                let count = pos.len() / 3;
                let idx: Vec<usize> = match num(&p, "indices") { Some(a) => read(a)?.0.into_iter().map(|v| v as usize).collect(), None => (0..count).collect() };
                let at = |k: usize| [pos[k * 3] as f32, pos[k * 3 + 1] as f32, pos[k * 3 + 2] as f32];
                for t in idx.chunks_exact(3) {
                    if t.iter().any(|&k| k >= count) { return Err(MeshError::Truncated); }
                    let mut tri = [world.point(at(t[0])), world.point(at(t[1])), world.point(at(t[2]))];
                    // Given normals say which side is out.
                    if let Some(nv) = &nrm {
                        let avg = t.iter().fold([0.0; 3], |s, &k| add(s, [nv[k * 3] as f32, nv[k * 3 + 1] as f32, nv[k * 3 + 2] as f32]));
                        if dot(world.vector(avg), Mesh::normal(&tri)) < 0.0 { tri.swap(1, 2); }
                    }
                    out.tris.push(tri);
                }
            }
        }
        for c in arr(n, "children") { if let Some(c) = c.as_u64() { stack.push((c as usize, world, depth + 1)); } }
    }
    if out.tris.is_empty() { return Err(MeshError::Missing("triangles")); }
    Ok(out)
}

/// Standard base64 (with or without padding).
#[cfg(feature = "gltf")]
fn base64(s: &str) -> Result<Vec<u8>, MeshError> {
    let val = |c: u8| -> Result<u32, MeshError> { Ok(match c {
        b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' | b'-' => 62, b'/' | b'_' => 63,
        _ => return Err(MeshError::Base64(c as char)),
    } as u32) };
    let b: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').collect();
    let mut out = Vec::with_capacity(b.len() * 3 / 4);
    for ch in b.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in ch.iter().enumerate() { n |= val(c)? << (18 - 6 * i); }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..ch.len().saturating_sub(1)]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(m: &Mesh) -> f32 {
        // For a closed surface the area-weighted normals sum to zero.
        let s = m.tris.iter().fold([0.0; 3], |s, t| add(s, mulf(cross(sub(t[1], t[0]), sub(t[2], t[0])), 0.5)));
        dot(s, s).sqrt()
    }

    #[test]
    fn primitives_are_closed_and_face_out() {
        for (name, m) in [("cube", cube()), ("sphere", uv_sphere(16, 10)), ("ico2", icosphere(2)), ("cylinder", cylinder(16)), ("cone", cone(16))] {
            assert!(closed(&m) < 1e-4, "{name} is not closed: {}", closed(&m));
            // Convex and centred: every face points away from the centre.
            for t in &m.tris {
                let c = mulf(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
                assert!(dot(Mesh::normal(t), c) > 0.0, "{name}: a face points in");
            }
        }
        assert!(closed(&torus(0.34, 0.15, 20, 10)) < 1e-4);
        for l in 0..4 { assert_eq!(icosphere(l).tris.len(), 20 * 4usize.pow(l)); }
    }

    #[test]
    fn camera_looks_at_its_target() {
        let c = Camera::default();
        let v = c.view();
        let t = v.point(c.target);
        assert!(t[0].abs() < 1e-4 && t[1].abs() < 1e-4 && (t[2] + c.distance).abs() < 1e-4, "{t:?}");
        let (o, d) = c.ray(0.5, 0.5, 1.5);
        let toward = norm(sub(c.target, o));
        assert!(dot(d, toward) > 0.9999);
    }

    #[test]
    fn picks_the_nearest_copy() {
        let lib = Library::default();
        let mut s = Scene3d::single(MeshId::CUBE);
        s.items.push(MeshInstances { mesh: MeshId::CUBE, transforms: vec![Mat4::translate(-0.6, 0.0, -2.0), Mat4::translate(5.0, 0.0, 0.0)], paint: Paint::INK, id: 7, outline: None });
        s.camera = Camera { yaw: 0.0, pitch: 0.0, ..Camera::default() };
        assert_eq!(s.pick(&lib, 0.5, 0.5, 1.0).map(|h| (h.object, h.instance)), Some((1, 0)));
        assert_eq!(s.pick(&lib, 0.02, 0.02, 1.0), None);
        // A copy behind the first, seen past its edge.
        let left = (0..50).map(|i| i as f32 / 100.0).find_map(|fx| s.pick(&lib, fx, 0.5, 1.0).filter(|h| h.object == 7));
        assert_eq!(left.map(|h| (h.object, h.instance)), Some((7, 0)));
    }

    #[test]
    fn a_pick_says_where_on_the_surface() {
        let mut lib = Library::default();
        let ball = lib.add(icosphere(4));
        let mut s = Scene3d::single(ball);
        // Icospheres have radius 0.5: twice the size is the unit sphere.
        s.items[0].transforms = vec![Mat4::uniform(2.0)];
        s.camera = Camera { yaw: 25.0, pitch: 10.0, ..Camera::default() };
        let mut n = 0;
        for (fx, fy) in [(0.5, 0.5), (0.42, 0.55), (0.6, 0.4), (0.5, 0.36)] {
            let Some(h) = s.pick(&lib, fx, fy, 1.0) else { continue };
            n += 1;
            // An icosphere is a little inside the unit sphere between its vertices.
            let r = dot(h.point, h.point).sqrt();
            assert!((r - 1.0).abs() < 2e-3, "|point| = {r}");
            // The face normal of a fine icosphere is the radial direction, within the facet's angle.
            assert!(dot(h.normal, norm(h.point)) > 0.998, "normal {:?} point {:?}", h.normal, h.point);
            let (o, d) = s.camera.ray(fx, fy, 1.0);
            let back = add(o, mulf(d, h.t));
            assert!(dot(sub(back, h.point), sub(back, h.point)).sqrt() < 1e-4);
            assert_eq!(h.uv, [fx, fy]);
        }
        assert_eq!(n, 4);
        // A unit sphere solved exactly: the ray's point is on it to 1e-4.
        let (o, d) = s.camera.ray(0.5, 0.5, 1.0);
        let b = dot(o, d);
        let t = -b - (b * b - (dot(o, o) - 1.0)).sqrt();
        let p = add(o, mulf(d, t));
        assert!((dot(p, p).sqrt() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn samples_lie_on_the_surface() {
        let m = cube();
        let pts = m.sample(500, 3);
        assert_eq!(pts.len(), 500);
        for (p, n) in &pts { assert!((p.iter().map(|v| v.abs()).fold(0.0, f32::max) - 0.5).abs() < 1e-4); assert!((dot(*n, *n) - 1.0).abs() < 1e-4); }
        assert_eq!(pts, m.sample(500, 3));
    }

    #[test]
    fn surface_points_project_and_hide_behind_the_mesh() {
        let lib = Library::default();
        let mut sc = Scene3d::single(MeshId::ICO[2]);
        sc.items[0].transforms = vec![Mat4::uniform(1.4)];
        let pts = sc.surface(&lib, 2000, 9);
        assert_eq!(pts.len(), 2000);
        let eye = sc.camera.eye();
        let (mut seen, mut agree) = (0, 0);
        for sp in &pts {
            let (fx, fy, vis) = sc.project(&lib, sp, 1.3).expect("in front of the camera");
            assert!((-0.2..1.2).contains(&fx) && (-0.2..1.2).contains(&fy));
            // On a sphere the seen half is the one facing the eye.
            let w = sc.items[0].transforms[0].point(sp.p);
            let facing = dot(norm(w), norm(sub(eye, w))) > 0.0;
            seen += vis as usize;
            agree += (vis == facing) as usize;
        }
        assert!((700..1300).contains(&seen), "{seen} of 2000 seen");
        assert!(agree > 1900, "{agree} agree with facing");
        // Turning the copy moves its points with it.
        let a = sc.project(&lib, &pts[0], 1.3).expect("a");
        sc.items[0].transforms = vec![Mat4::rotate_y(40.0).mul(&Mat4::uniform(1.4))];
        let b = sc.project(&lib, &pts[0], 1.3).expect("b");
        assert!((a.0 - b.0).abs() + (a.1 - b.1).abs() > 0.01);
    }

    #[cfg(feature = "gltf")]
    #[test]
    fn reads_a_small_gltf() {
        // One triangle (3 floats × 3) and its indices, inline, moved by its node.
        let mut buf = vec![];
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] { buf.extend_from_slice(&v.to_le_bytes()); }
        for i in [0u16, 1, 2] { buf.extend_from_slice(&i.to_le_bytes()); }
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut b64 = String::new();
        for c in buf.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 { if k <= c.len() { b64.push(T[(n >> (18 - 6 * k) & 63) as usize] as char); } else { b64.push('='); } }
        }
        let json = format!(r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"translation":[0,0,2]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
            "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{b64}"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#, buf.len());
        let m = load_gltf(json.as_bytes()).expect("loads");
        assert_eq!(m.tris, vec![[[0.0, 0.0, 2.0], [1.0, 0.0, 2.0], [0.0, 1.0, 2.0]]]);
        assert!(matches!(load_gltf(b"{\"buffers\":[{\"uri\":\"mesh.bin\"}]}"), Err(MeshError::ExternalBuffer(_))));
    }
}
