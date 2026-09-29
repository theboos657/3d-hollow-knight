//! Procedural geometry for the creature and prop models: revolved surfaces
//! (helms, cloaks, bells, shells), tubes (arms, tendrils, chains), ribbons,
//! blades, prisms and slash crescents. Everything here is pure maths returning
//! plain [`MeshData`], so it is unit-tested without a renderer; [`to_mesh`]
//! turns the result into a Bevy `Mesh`.
//!
//! Conventions: right-handed, +Y up. Front faces are counter-clockwise and
//! normals point out of the surface. Vertex colours (`col`) multiply the
//! material's base colour, which is how gradients and trail fades are made.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::math::{Mat3, Mat4, Vec2, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub col: Vec<[f32; 4]>,
    pub idx: Vec<u32>,
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

impl MeshData {
    pub fn vertex_count(&self) -> usize {
        self.pos.len()
    }

    #[cfg(test)]
    pub fn triangle_count(&self) -> usize {
        self.idx.len() / 3
    }

    pub fn push(&mut self, p: Vec3, n: Vec3, uv: Vec2, col: [f32; 4]) -> u32 {
        self.pos.push(p.to_array());
        self.nrm.push(n.to_array());
        self.uv.push(uv.to_array());
        self.col.push(col);
        (self.pos.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.idx.extend([a, b, c]);
    }

    /// Appends one flat quad. `p` are its corners, counter-clockwise seen from
    /// the side `n` points to; `col` are the per-corner vertex colours.
    pub fn add_quad(&mut self, p: [Vec3; 4], n: Vec3, uv: [Vec2; 4], col: [[f32; 4]; 4]) {
        let ids: Vec<u32> = (0..4).map(|i| self.push(p[i], n, uv[i], col[i])).collect();
        self.tri(ids[0], ids[1], ids[2]);
        self.tri(ids[0], ids[2], ids[3]);
    }

    /// Appends `other` (already in this mesh's space).
    pub fn merge(&mut self, other: &MeshData) {
        let base = self.pos.len() as u32;
        self.pos.extend(&other.pos);
        self.nrm.extend(&other.nrm);
        self.uv.extend(&other.uv);
        self.col.extend(&other.col);
        self.idx.extend(other.idx.iter().map(|i| i + base));
    }

    /// Gives every triangle planar texture coordinates from the axis its face
    /// looks along, `scale` repeats per world unit (`offset` shifts the window).
    /// Triangles are unwelded so each keeps its own mapping; normals are kept,
    /// so smooth shapes stay smooth and only the texture seams where the
    /// dominant axis changes. This is what architecture uses in place of
    /// hand-made UVs: the texture density is the same on every surface.
    pub fn box_mapped(&self, scale: f32, offset: Vec2) -> MeshData {
        let mut out = MeshData::default();
        for t in self.idx.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| Vec3::from(self.pos[t[k] as usize]));
            let a = (p[1] - p[0]).cross(p[2] - p[0]).abs();
            let plane = |q: Vec3| {
                let uv = if a.y >= a.x && a.y >= a.z {
                    Vec2::new(q.x, q.z)
                } else if a.x >= a.z {
                    Vec2::new(q.z, q.y)
                } else {
                    Vec2::new(q.x, q.y)
                };
                uv * scale + offset
            };
            let ids = [0, 1, 2].map(|k| {
                let i = t[k] as usize;
                out.push(p[k], Vec3::from(self.nrm[i]), plane(p[k]), self.col[i])
            });
            out.tri(ids[0], ids[1], ids[2]);
        }
        out
    }

    /// Applies `m` to every vertex (normals go through the inverse transpose,
    /// so non-uniform scales stay correct).
    pub fn transformed(mut self, m: Mat4) -> MeshData {
        let nm = Mat3::from_mat4(m).inverse().transpose();
        let flips = Mat3::from_mat4(m).determinant() < 0.0;
        for p in &mut self.pos {
            *p = m.transform_point3(Vec3::from(*p)).to_array();
        }
        for n in &mut self.nrm {
            *n = (nm * Vec3::from(*n)).normalize_or_zero().to_array();
        }
        if flips {
            // A mirroring transform reverses winding.
            for t in self.idx.chunks_exact_mut(3) {
                t.swap(1, 2);
            }
        }
        self
    }

    /// Sets every vertex colour from its position.
    pub fn recolor(mut self, f: impl Fn(Vec3) -> [f32; 4]) -> MeshData {
        for (c, p) in self.col.iter_mut().zip(&self.pos) {
            *c = f(Vec3::from(*p));
        }
        self
    }

    /// Multiplies every vertex colour by `k` (RGBA).
    pub fn tinted(mut self, k: [f32; 4]) -> MeshData {
        for c in &mut self.col {
            for i in 0..4 {
                c[i] *= k[i];
            }
        }
        self
    }

    /// Pushes every vertex along its normal by hash noise in `[-amp, amp]`.
    /// The noise depends only on position, so seams stay closed.
    pub fn jitter(mut self, seed: u32, amp: f32) -> MeshData {
        for (p, n) in self.pos.iter_mut().zip(&self.nrm) {
            let q = Vec3::from(*p);
            let h = hash3(
                seed,
                (q.x * 64.0).round() as i32,
                (q.y * 64.0).round() as i32,
                (q.z * 64.0).round() as i32,
            );
            let d = (h * 2.0 - 1.0) * amp;
            *p = (q + Vec3::from(*n) * d).to_array();
        }
        self
    }

    #[cfg(test)]
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for p in &self.pos {
            lo = lo.min(Vec3::from(*p));
            hi = hi.max(Vec3::from(*p));
        }
        (lo, hi)
    }

    /// Structural checks: sizes agree, indices in range, everything finite,
    /// normals are unit length and no triangle faces *against* its vertex
    /// normals (the sign of a wrong winding).
    #[cfg(test)]
    pub fn validate(&self) -> Result<(), String> {
        let n = self.pos.len();
        if n == 0 || self.idx.is_empty() {
            return Err("empty mesh".into());
        }
        if self.nrm.len() != n || self.uv.len() != n || self.col.len() != n {
            return Err("attribute lengths disagree".into());
        }
        if !self.idx.len().is_multiple_of(3) {
            return Err("index count is not a multiple of 3".into());
        }
        if let Some(i) = self.idx.iter().find(|i| **i as usize >= n) {
            return Err(format!("index {i} out of range ({n} vertices)"));
        }
        for (i, p) in self.pos.iter().enumerate() {
            if p.iter().any(|v| !v.is_finite()) {
                return Err(format!("vertex {i} is not finite"));
            }
        }
        for (i, v) in self.nrm.iter().enumerate() {
            let l = Vec3::from(*v).length();
            if !(0.98..=1.02).contains(&l) {
                return Err(format!("normal {i} has length {l}"));
            }
        }
        let mut against = 0;
        let mut total = 0;
        for t in self.idx.chunks_exact(3) {
            let (a, b, c) = (
                Vec3::from(self.pos[t[0] as usize]),
                Vec3::from(self.pos[t[1] as usize]),
                Vec3::from(self.pos[t[2] as usize]),
            );
            let face = (b - a).cross(c - a);
            if face.length_squared() < 1e-12 {
                continue; // degenerate (poles): no direction to check
            }
            let avg = Vec3::from(self.nrm[t[0] as usize])
                + Vec3::from(self.nrm[t[1] as usize])
                + Vec3::from(self.nrm[t[2] as usize]);
            total += 1;
            if face.dot(avg) < 0.0 {
                against += 1;
            }
        }
        if total > 0 && against * 10 > total {
            return Err(format!(
                "{against} of {total} triangles wind against their normals"
            ));
        }
        Ok(())
    }

    pub fn to_mesh(&self) -> Mesh {
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col.clone());
        m.insert_indices(Indices::U32(self.idx.clone()));
        m
    }

    /// `to_mesh` plus mikktspace tangents, so a material's normal map lights
    /// correctly. Where the UVs are unusable (a mesh with no area in UV space)
    /// the tangents are simply left off and the normal map is ignored.
    pub fn to_mesh_pbr(&self) -> Mesh {
        let mut m = self.to_mesh();
        if self.idx.len() >= 3 {
            let _ = m.generate_tangents();
        }
        m
    }
}

/// Deterministic hash noise in `[0, 1)`.
pub fn hash3(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B1)
        ^ (x as u32).wrapping_mul(0x85EB_CA6B)
        ^ (y as u32).wrapping_mul(0xC2B2_AE35)
        ^ (z as u32).wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

// ------------------------------------------------------------------ lathe --

/// Revolves a profile of `(radius, y)` points (listed bottom to top, on the
/// outside of the shape) around the Y axis. Smooth normals come from the
/// profile's tangents. A profile point with radius 0 makes a clean pole.
pub fn lathe(profile: &[(f32, f32)], radial: usize) -> MeshData {
    assert!(profile.len() >= 2 && radial >= 3);
    let mut m = MeshData::default();
    let n = profile.len();
    // Profile normals from central differences of the profile curve.
    let mut normals2 = Vec::with_capacity(n);
    for i in 0..n {
        let a = profile[i.saturating_sub(1)];
        let b = profile[(i + 1).min(n - 1)];
        let t = Vec2::new(b.0 - a.0, b.1 - a.1);
        let nn = Vec2::new(t.y, -t.x).normalize_or_zero();
        normals2.push(if nn == Vec2::ZERO { Vec2::X } else { nn });
    }
    let total: f32 = profile
        .windows(2)
        .map(|w| Vec2::new(w[1].0 - w[0].0, w[1].1 - w[0].1).length())
        .sum::<f32>()
        .max(1e-6);
    let mut run = 0.0;
    for (i, &(r, y)) in profile.iter().enumerate() {
        if i > 0 {
            let p = profile[i - 1];
            run += Vec2::new(r - p.0, y - p.1).length();
        }
        for j in 0..=radial {
            let phi = TAU * j as f32 / radial as f32;
            let (s, c) = phi.sin_cos();
            let n2 = normals2[i];
            m.push(
                Vec3::new(r * c, y, r * s),
                Vec3::new(n2.x * c, n2.y, n2.x * s).normalize_or_zero(),
                Vec2::new(j as f32 / radial as f32, run / total),
                WHITE,
            );
        }
    }
    let row = (radial + 1) as u32;
    for i in 0..(n as u32 - 1) {
        for j in 0..radial as u32 {
            let a = i * row + j;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            m.tri(a, c, b);
            m.tri(b, c, d);
        }
    }
    m
}

/// A sphere-like ellipsoid (poles on Y), radii `(rx, ry, rz)`.
pub fn ellipsoid(rx: f32, ry: f32, rz: f32, lat: usize, lon: usize) -> MeshData {
    let profile: Vec<(f32, f32)> = (0..=lat)
        .map(|i| {
            let a = -PI / 2.0 + PI * i as f32 / lat as f32;
            (a.cos().max(0.0), a.sin())
        })
        .collect();
    lathe(&profile, lon).transformed(Mat4::from_scale(Vec3::new(rx, ry, rz)))
}

/// A torus in the XZ plane (axis Y).
pub fn ring(major: f32, minor: f32, major_seg: usize, minor_seg: usize) -> MeshData {
    let profile: Vec<(f32, f32)> = (0..=minor_seg)
        .map(|i| {
            let a = TAU * i as f32 / minor_seg as f32;
            (major + minor * a.cos(), minor * a.sin())
        })
        .collect();
    lathe(&profile, major_seg)
}

/// A cone (apex up) from radius `r` at y = 0 to a point at `h`.
pub fn cone(r: f32, h: f32, radial: usize) -> MeshData {
    lathe(&[(r, 0.0), (r * 0.5, h * 0.5), (0.0, h)], radial)
}

// -------------------------------------------------------------------- tube --

/// A tube along `path` with radius `radius(t)` for `t` in `[0, 1]`, using
/// parallel-transported frames (no twisting). Ends are closed with fans.
pub fn tube(path: &[Vec3], radius: impl Fn(f32) -> f32, sides: usize) -> MeshData {
    assert!(path.len() >= 2 && sides >= 3);
    let n = path.len();
    let mut m = MeshData::default();
    // Tangents.
    let tan: Vec<Vec3> = (0..n)
        .map(|i| {
            let a = path[i.saturating_sub(1)];
            let b = path[(i + 1).min(n - 1)];
            (b - a).normalize_or_zero()
        })
        .collect();
    // An initial normal not parallel to the first tangent.
    let mut up = if tan[0].dot(Vec3::Z).abs() < 0.9 {
        Vec3::Z
    } else {
        Vec3::X
    };
    let mut frames = Vec::with_capacity(n);
    for t in &tan {
        let side = t.cross(up).normalize_or_zero();
        up = side.cross(*t).normalize_or_zero();
        frames.push((side, up));
    }
    for i in 0..n {
        let f = i as f32 / (n - 1) as f32;
        let r = radius(f).max(0.0);
        let (side, upv) = frames[i];
        for j in 0..=sides {
            let a = TAU * j as f32 / sides as f32;
            let dir = side * a.cos() + upv * a.sin();
            m.push(
                path[i] + dir * r,
                dir,
                Vec2::new(j as f32 / sides as f32, f),
                WHITE,
            );
        }
    }
    let row = (sides + 1) as u32;
    for i in 0..(n as u32 - 1) {
        for j in 0..sides as u32 {
            let a = i * row + j;
            let b = a + 1;
            let c = a + row;
            let d = c + 1;
            m.tri(a, c, b);
            m.tri(b, c, d);
        }
    }
    // Caps.
    for (end, flip) in [(0usize, true), (n - 1, false)] {
        let f = end as f32 / (n - 1) as f32;
        let r = radius(f).max(0.0);
        if r <= 1e-5 {
            continue;
        }
        let normal = if flip { -tan[end] } else { tan[end] };
        let centre = m.push(path[end], normal, Vec2::splat(0.5), WHITE);
        let (side, upv) = frames[end];
        let first = m.pos.len() as u32;
        for j in 0..=sides {
            let a = TAU * j as f32 / sides as f32;
            let dir = side * a.cos() + upv * a.sin();
            m.push(
                path[end] + dir * r,
                normal,
                Vec2::new(0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5),
                WHITE,
            );
        }
        for j in 0..sides as u32 {
            if flip {
                m.tri(centre, first + j, first + j + 1);
            } else {
                m.tri(centre, first + j + 1, first + j);
            }
        }
    }
    m
}

/// A straight tube between two points.
pub fn limb(a: Vec3, b: Vec3, r0: f32, r1: f32, sides: usize) -> MeshData {
    tube(&[a, b], move |t| r0 + (r1 - r0) * t, sides)
}

// ----------------------------------------------------------------- ribbons --

/// A flat strip along `path` (a scarf, a fin, a pennant), `width(t)` wide in
/// the direction perpendicular to both the path and `facing`. Double-sided so
/// it shows from both directions with an ordinary culling material.
pub fn ribbon(path: &[Vec3], width: impl Fn(f32) -> f32, facing: Vec3) -> MeshData {
    assert!(path.len() >= 2);
    let n = path.len();
    let mut m = MeshData::default();
    let nrm = facing.normalize_or_zero();
    for i in 0..n {
        let f = i as f32 / (n - 1) as f32;
        let a = path[i.saturating_sub(1)];
        let b = path[(i + 1).min(n - 1)];
        let t = (b - a).normalize_or_zero();
        let side = t.cross(nrm).normalize_or_zero();
        let w = width(f) * 0.5;
        for (s, v) in [(-1.0f32, 0.0f32), (1.0, 1.0)] {
            m.push(path[i] + side * w * s, nrm, Vec2::new(f, v), WHITE);
        }
    }
    let count = m.pos.len();
    for i in 0..(n as u32 - 1) {
        let a = i * 2;
        m.tri(a, a + 1, a + 2);
        m.tri(a + 1, a + 3, a + 2);
    }
    // Back faces.
    for i in 0..count {
        let (p, u, c) = (m.pos[i], m.uv[i], m.col[i]);
        m.pos.push(p);
        m.nrm.push((-nrm).to_array());
        m.uv.push(u);
        m.col.push(c);
    }
    for i in 0..(n as u32 - 1) {
        let a = count as u32 + i * 2;
        m.tri(a, a + 2, a + 1);
        m.tri(a + 1, a + 2, a + 3);
    }
    m
}

// ------------------------------------------------------------------ blades --

/// A double-edged blade along +X: a flat diamond cross-section of width `w`
/// (in Y) and thickness `t` (in Z) tapering to a point at `len`. Flat shaded.
pub fn blade(len: f32, w: f32, t: f32, taper_start: f32) -> MeshData {
    let mut m = MeshData::default();
    let x0 = 0.0;
    let x1 = len * taper_start.clamp(0.0, 0.95);
    let (hy, hz) = (w * 0.5, t * 0.5);
    // Two cross-sections (base and where the taper starts) and the tip.
    let ring = |x: f32| {
        [
            Vec3::new(x, hy, 0.0),
            Vec3::new(x, 0.0, hz),
            Vec3::new(x, -hy, 0.0),
            Vec3::new(x, 0.0, -hz),
        ]
    };
    let base = ring(x0);
    let mid = ring(x1);
    let tip = Vec3::new(len, 0.0, 0.0);
    let quad = |m: &mut MeshData, p: [Vec3; 4]| {
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        let ids: Vec<u32> = p
            .iter()
            .enumerate()
            .map(|(i, v)| m.push(*v, n, Vec2::new((i % 2) as f32, (i / 2) as f32), WHITE))
            .collect();
        m.tri(ids[0], ids[1], ids[2]);
        m.tri(ids[0], ids[2], ids[3]);
    };
    // Shaft: four flat faces between the base and the taper start.
    for k in 0..4 {
        let (a, b) = (k, (k + 1) % 4);
        // Outward facing: (base[a], mid[a], mid[b], base[b]) or reversed; pick by normal direction.
        let centre = (base[a] + base[b]) * 0.5;
        let mut p = [base[a], base[b], mid[b], mid[a]];
        let nrm = (p[1] - p[0]).cross(p[3] - p[0]);
        if nrm.dot(centre - Vec3::new(centre.x, 0.0, 0.0)) < 0.0 {
            p = [base[a], mid[a], mid[b], base[b]];
        }
        quad(&mut m, p);
    }
    // Point: four triangles from the taper ring to the tip.
    for k in 0..4 {
        let (a, b) = (mid[k], mid[(k + 1) % 4]);
        let centre = (a + b + tip) / 3.0 - Vec3::new((a.x + b.x + tip.x) / 3.0, 0.0, 0.0);
        let mut tri = [a, b, tip];
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
        if n.dot(centre) < 0.0 {
            tri = [b, a, tip];
        }
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        let ids: Vec<u32> = tri
            .iter()
            .map(|v| m.push(*v, n, Vec2::ZERO, WHITE))
            .collect();
        m.tri(ids[0], ids[1], ids[2]);
    }
    m
}

/// A prism from a convex or star-shaped polygon (in XY, around the origin),
/// extruded `depth` along Z (centred). Flat shaded walls, fan-triangulated caps.
pub fn extrude(poly: &[Vec2], depth: f32) -> MeshData {
    assert!(poly.len() >= 3);
    let mut m = MeshData::default();
    let hz = depth * 0.5;
    let centre = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    // Make the polygon counter-clockwise.
    let area: f32 = (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let pts: Vec<Vec2> = if area < 0.0 {
        poly.iter().rev().copied().collect()
    } else {
        poly.to_vec()
    };
    // Caps.
    for (z, n) in [(hz, Vec3::Z), (-hz, Vec3::NEG_Z)] {
        let c = m.push(Vec3::new(centre.x, centre.y, z), n, Vec2::splat(0.5), WHITE);
        let first = m.pos.len() as u32;
        for p in &pts {
            m.push(Vec3::new(p.x, p.y, z), n, Vec2::new(p.x, p.y), WHITE);
        }
        for i in 0..pts.len() as u32 {
            let (a, b) = (first + i, first + (i + 1) % pts.len() as u32);
            if z > 0.0 {
                m.tri(c, a, b);
            } else {
                m.tri(c, b, a);
            }
        }
    }
    // Walls.
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let edge = Vec3::new(b.x - a.x, b.y - a.y, 0.0);
        let n = Vec3::new(edge.y, -edge.x, 0.0).normalize_or_zero();
        let ids = [
            m.push(Vec3::new(a.x, a.y, hz), n, Vec2::new(0.0, 0.0), WHITE),
            m.push(Vec3::new(b.x, b.y, hz), n, Vec2::new(1.0, 0.0), WHITE),
            m.push(Vec3::new(b.x, b.y, -hz), n, Vec2::new(1.0, 1.0), WHITE),
            m.push(Vec3::new(a.x, a.y, -hz), n, Vec2::new(0.0, 1.0), WHITE),
        ];
        m.tri(ids[0], ids[3], ids[1]);
        m.tri(ids[1], ids[3], ids[2]);
    }
    m
}

// ---------------------------------------------------------------- crescent --

/// A slash crescent in the XY plane around the origin: an arc from angle
/// `a0` to `a1` (radians, measured from +X, either direction) at outer radius
/// `r_outer`. Its width grows from a thin tail (`w_tail`) to `w_head` at the
/// head (the `a1` end), and vertex alpha ramps from 0 at the tail to 1 at the
/// head. Double-sided.
pub fn crescent(r_outer: f32, w_tail: f32, w_head: f32, a0: f32, a1: f32, segs: usize) -> MeshData {
    let mut m = MeshData::default();
    for i in 0..=segs {
        let s = i as f32 / segs as f32;
        let a = a0 + (a1 - a0) * s;
        let w = w_tail + (w_head - w_tail) * s.powf(1.5);
        let alpha = s.powf(1.2);
        let dir = Vec3::new(a.cos(), a.sin(), 0.0);
        for (r, v) in [(r_outer - w, 0.0f32), (r_outer, 1.0)] {
            m.push(
                dir * r,
                Vec3::Z,
                Vec2::new(s, v),
                [1.0, 1.0, 1.0, alpha * if v > 0.5 { 0.6 } else { 1.0 }],
            );
        }
    }
    let count = m.pos.len();
    let flip = a1 < a0; // decreasing angle reverses the winding
    for i in 0..segs as u32 {
        let a = i * 2;
        if flip {
            m.tri(a, a + 2, a + 1);
            m.tri(a + 1, a + 2, a + 3);
        } else {
            m.tri(a, a + 1, a + 2);
            m.tri(a + 1, a + 3, a + 2);
        }
    }
    for i in 0..count {
        let (p, u, c) = (m.pos[i], m.uv[i], m.col[i]);
        m.pos.push(p);
        m.nrm.push(Vec3::NEG_Z.to_array());
        m.uv.push(u);
        m.col.push(c);
    }
    for i in 0..segs as u32 {
        let a = count as u32 + i * 2;
        if flip {
            m.tri(a, a + 1, a + 2);
            m.tri(a + 1, a + 3, a + 2);
        } else {
            m.tri(a, a + 2, a + 1);
            m.tri(a + 1, a + 2, a + 3);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(name: &str, m: &MeshData) {
        m.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    #[test]
    fn every_builder_makes_a_valid_mesh() {
        ok(
            "lathe",
            &lathe(&[(0.5, 0.0), (0.5, 1.0), (0.2, 1.4), (0.0, 1.6)], 12),
        );
        ok("ellipsoid", &ellipsoid(0.5, 0.8, 0.4, 8, 12));
        ok("ring", &ring(1.0, 0.1, 16, 6));
        ok("cone", &cone(0.4, 1.0, 10));
        ok(
            "tube",
            &tube(
                &[
                    Vec3::ZERO,
                    Vec3::new(0.5, 0.5, 0.0),
                    Vec3::new(1.0, 0.6, 0.2),
                ],
                |t| 0.1 * (1.0 - t * 0.5),
                8,
            ),
        );
        ok(
            "limb",
            &limb(Vec3::ZERO, Vec3::new(0.0, -1.0, 0.0), 0.1, 0.06, 8),
        );
        ok(
            "ribbon",
            &ribbon(
                &[
                    Vec3::ZERO,
                    Vec3::new(-0.5, -0.2, 0.0),
                    Vec3::new(-1.0, -0.6, 0.0),
                ],
                |t| 0.3 * (1.0 - t),
                Vec3::Z,
            ),
        );
        ok("blade", &blade(1.5, 0.12, 0.05, 0.8));
        ok(
            "extrude",
            &extrude(
                &[
                    Vec2::new(-0.5, 0.0),
                    Vec2::new(0.5, 0.0),
                    Vec2::new(0.6, 0.9),
                    Vec2::new(0.0, 1.2),
                    Vec2::new(-0.6, 0.9),
                ],
                0.2,
            ),
        );
        ok("crescent", &crescent(2.0, 0.1, 0.9, 2.0, -1.0, 12));
        ok("crescent ccw", &crescent(2.0, 0.1, 0.9, -1.0, 2.0, 12));
    }

    #[test]
    fn pbr_meshes_get_finite_tangents_for_their_normal_maps() {
        use bevy::mesh::VertexAttributeValues;
        let shapes = [
            (
                "lathe",
                lathe(&[(0.5, 0.0), (0.5, 1.0), (0.2, 1.4), (0.0, 1.6)], 12),
            ),
            ("ellipsoid", ellipsoid(0.5, 0.8, 0.4, 8, 12)),
            ("ring", ring(1.0, 0.1, 16, 6)),
            (
                "limb",
                limb(Vec3::ZERO, Vec3::new(0.0, -1.0, 0.0), 0.1, 0.06, 8),
            ),
            ("blade", blade(1.5, 0.12, 0.05, 0.8)),
        ];
        for (name, data) in shapes {
            let mesh = data.to_mesh_pbr();
            let Some(VertexAttributeValues::Float32x4(t)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT)
            else {
                panic!("{name}: no tangents");
            };
            assert_eq!(t.len(), data.vertex_count(), "{name}");
            for v in t {
                assert!(v.iter().all(|c| c.is_finite()), "{name}: {v:?}");
                assert!((v[3].abs() - 1.0).abs() < 1e-3, "{name}: handedness {v:?}");
            }
        }
    }

    #[test]
    fn box_mapping_gives_every_surface_the_same_texture_density() {
        // A unit cube: each face maps one world unit to `scale` texture units,
        // whichever way the face looks.
        let mut cube = MeshData::default();
        let (h, w) = (0.5, [1.0; 4]);
        let uv0 = [Vec2::ZERO; 4];
        for (n, up, right) in [
            (Vec3::Z, Vec3::Y, Vec3::X),
            (Vec3::Y, Vec3::NEG_Z, Vec3::X),
            (Vec3::X, Vec3::Y, Vec3::NEG_Z),
        ] {
            let c = n * h;
            cube.add_quad(
                [
                    c - right * h - up * h,
                    c + right * h - up * h,
                    c + right * h + up * h,
                    c - right * h + up * h,
                ],
                n,
                uv0,
                [w; 4],
            );
        }
        let mapped = cube.box_mapped(0.25, Vec2::new(0.1, 0.2));
        ok("box mapped", &mapped);
        assert_eq!(mapped.triangle_count(), cube.triangle_count());
        for t in mapped.idx.chunks(3) {
            let p = [0, 1, 2].map(|k| Vec3::from(mapped.pos[t[k] as usize]));
            let u = [0, 1, 2].map(|k| Vec2::from(mapped.uv[t[k] as usize]));
            let (dp, du) = ((p[1] - p[0]).length(), (u[1] - u[0]).length());
            assert!((du - dp * 0.25).abs() < 1e-5, "{du} vs {dp}");
        }
    }

    #[test]
    fn a_lathe_of_a_cylinder_is_a_cylinder() {
        let m = lathe(&[(0.5, 0.0), (0.5, 2.0)], 16);
        let (lo, hi) = m.bounds();
        assert!((hi.y - 2.0).abs() < 1e-5 && lo.y.abs() < 1e-5);
        assert!((hi.x - 0.5).abs() < 1e-4 && (lo.x + 0.5).abs() < 1e-4);
        // Every normal points straight out from the axis.
        for (p, n) in m.pos.iter().zip(&m.nrm) {
            let radial = Vec3::new(p[0], 0.0, p[2]).normalize();
            assert!(Vec3::from(*n).dot(radial) > 0.999);
        }
    }

    #[test]
    fn ellipsoid_bounds_match_its_radii() {
        let m = ellipsoid(0.5, 0.8, 0.4, 12, 16);
        let (lo, hi) = m.bounds();
        assert!((hi.y - 0.8).abs() < 1e-4 && (lo.y + 0.8).abs() < 1e-4);
        assert!((hi.x - 0.5).abs() < 1e-3);
        assert!((hi.z - 0.4).abs() < 1e-3);
    }

    #[test]
    fn a_tube_follows_its_path_and_radius() {
        let m = tube(&[Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0)], |_| 0.25, 8);
        let (lo, hi) = m.bounds();
        assert!((hi.y - 2.0).abs() < 1e-5 && lo.y.abs() < 1e-5);
        assert!(hi.x <= 0.2501 && hi.z <= 0.2501);
    }

    #[test]
    fn a_blade_reaches_its_length_and_points_along_x() {
        let m = blade(1.75, 0.14, 0.05, 0.85);
        let (lo, hi) = m.bounds();
        assert!((hi.x - 1.75).abs() < 1e-5 && lo.x.abs() < 1e-5);
        assert!(hi.y <= 0.0701 && hi.z <= 0.0251);
    }

    #[test]
    fn a_crescent_fades_from_tail_to_head_and_sits_on_its_arc() {
        let m = crescent(2.0, 0.1, 0.8, 2.0, -1.0, 20);
        let alphas: Vec<f32> = m.col.iter().map(|c| c[3]).collect();
        assert!(alphas[0] < 0.01, "the tail is transparent");
        assert!(
            alphas.iter().cloned().fold(0.0, f32::max) > 0.95,
            "the head is opaque"
        );
        for p in &m.pos {
            let r = Vec2::new(p[0], p[1]).length();
            assert!((1.15..=2.0001).contains(&r), "radius {r} off the arc");
        }
    }

    #[test]
    fn transforms_move_normals_correctly_even_when_squashed_and_mirrored() {
        let m =
            ellipsoid(1.0, 1.0, 1.0, 8, 12).transformed(Mat4::from_scale(Vec3::new(2.0, 0.5, 1.0)));
        ok("scaled", &m);
        let mirrored = ellipsoid(0.5, 0.5, 0.5, 8, 12)
            .transformed(Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        ok("mirrored", &mirrored);
    }

    #[test]
    fn merge_offsets_indices() {
        let mut a = cone(0.3, 1.0, 8);
        let b = cone(0.3, 1.0, 8).transformed(Mat4::from_translation(Vec3::X * 2.0));
        let (va, ta) = (a.vertex_count(), a.triangle_count());
        a.merge(&b);
        assert_eq!(a.vertex_count(), va * 2);
        assert_eq!(a.triangle_count(), ta * 2);
        ok("merged", &a);
    }

    #[test]
    fn jitter_is_deterministic_and_bounded() {
        let base = ellipsoid(0.5, 0.5, 0.5, 8, 12);
        let a = base.clone().jitter(7, 0.05);
        let b = base.clone().jitter(7, 0.05);
        let c = base.clone().jitter(8, 0.05);
        assert_eq!(a.pos, b.pos);
        assert_ne!(a.pos, c.pos);
        for (p, q) in a.pos.iter().zip(&base.pos) {
            assert!((Vec3::from(*p) - Vec3::from(*q)).length() <= 0.0501);
        }
    }

    #[test]
    fn validate_catches_broken_meshes() {
        let mut m = cone(0.3, 1.0, 8);
        m.idx.push(9999);
        m.idx.push(0);
        m.idx.push(1);
        assert!(m.validate().is_err(), "index out of range");
        let mut m = cone(0.3, 1.0, 8);
        m.pos[0][0] = f32::NAN;
        assert!(m.validate().is_err(), "NaN");
        let mut m = cone(0.3, 1.0, 8);
        for t in m.idx.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        assert!(m.validate().is_err(), "inside-out winding");
    }
}
