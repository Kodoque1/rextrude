//! Geometry primitives: axis-aligned boxes, coordinate mapping between glTF
//! space and machine space, vertex welding, and connected-component
//! decomposition (recovers per-sub-part identity from a flattened mesh).

use std::collections::HashMap;

use crate::spec::Axis;

#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb {
    pub fn from_points<'a>(pts: impl Iterator<Item = &'a [f64; 3]>) -> Option<Self> {
        let mut it = pts;
        let first = it.next()?;
        let mut min = *first;
        let mut max = *first;
        for p in it {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        Some(Aabb { min, max })
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        let mut min = self.min;
        let mut max = self.max;
        for i in 0..3 {
            min[i] = min[i].min(other.min[i]);
            max[i] = max[i].max(other.max[i]);
        }
        Aabb { min, max }
    }

    pub fn translated(&self, d: [f64; 3]) -> Aabb {
        let mut min = self.min;
        let mut max = self.max;
        for i in 0..3 {
            min[i] += d[i];
            max[i] += d[i];
        }
        Aabb { min, max }
    }

    /// Strict interior overlap on all three axes (a shared boundary alone
    /// does not count as a collision).
    pub fn overlaps(&self, other: &Aabb, tol: f64) -> bool {
        (0..3).all(|i| self.min[i] + tol < other.max[i] && other.min[i] + tol < self.max[i])
    }

    pub fn extent(&self, axis: usize) -> f64 {
        self.max[axis] - self.min[axis]
    }

    pub fn center(&self, axis: usize) -> f64 {
        (self.min[axis] + self.max[axis]) / 2.0
    }
}

/// A 4x4 column-major transform matrix (glTF convention), stored as f64.
#[derive(Debug, Clone, Copy)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    pub fn from_f32_cols(m: [[f32; 4]; 4]) -> Self {
        let mut out = [[0.0; 4]; 4];
        for (c, col) in m.iter().enumerate() {
            for (r, v) in col.iter().enumerate() {
                out[c][r] = *v as f64;
            }
        }
        Mat4(out)
    }

    pub fn mul(&self, rhs: &Mat4) -> Mat4 {
        let a = &self.0;
        let b = &rhs.0;
        let mut out = [[0.0; 4]; 4];
        for c in 0..4 {
            for r in 0..4 {
                let mut s = 0.0;
                for k in 0..4 {
                    s += a[k][r] * b[c][k];
                }
                out[c][r] = s;
            }
        }
        Mat4(out)
    }

    pub fn transform_point(&self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.0;
        [
            m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
            m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
            m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
        ]
    }

    pub fn translation(&self) -> [f64; 3] {
        [self.0[3][0], self.0[3][1], self.0[3][2]]
    }
}

/// Which gltf axis index (0=x,1=y,2=z) feeds each machine axis.
pub fn gltf_to_machine_perm(gltf_to_machine: [Axis; 3]) -> [usize; 3] {
    [
        gltf_to_machine[0].index(),
        gltf_to_machine[1].index(),
        gltf_to_machine[2].index(),
    ]
}

pub fn to_machine(p: [f64; 3], perm: [usize; 3]) -> [f64; 3] {
    [p[perm[0]], p[perm[1]], p[perm[2]]]
}

/// Quantize-and-weld vertex positions, then union-find over triangle edges
/// to recover connected components.
///
/// Sub-parts of a flattened mesh never share vertices in this pipeline (each
/// face gets its own planar UV projection, so the exporter splits vertices
/// along every seam) — welding by position recovers per-part identity
/// without needing any label metadata.
///
/// Returns `(vertex_weld_id, vertex_component, component_count)`:
/// `vertex_weld_id` is the fine-grained per-position id (distinct for every
/// distinct welded position — this is what edge/manifold checks need, since
/// a whole closed component would otherwise collapse every edge into one
/// bucket); `vertex_component` is the coarser id after union-find over
/// triangle adjacency (what AABB/sweep grouping needs).
pub fn weld_and_components(
    positions: &[[f64; 3]],
    indices: &[u32],
    weld_tol: f64,
) -> (Vec<usize>, Vec<usize>, usize) {
    let key_of = |p: &[f64; 3]| -> (i64, i64, i64) {
        let q = |v: f64| (v / weld_tol).round() as i64;
        (q(p[0]), q(p[1]), q(p[2]))
    };

    let mut weld_id_of_key: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut weld_id = vec![0usize; positions.len()];
    for (i, p) in positions.iter().enumerate() {
        let key = key_of(p);
        let next_id = weld_id_of_key.len();
        let id = *weld_id_of_key.entry(key).or_insert(next_id);
        weld_id[i] = id;
    }

    let n_weld = weld_id_of_key.len();
    let mut parent: Vec<usize> = (0..n_weld).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    for tri in indices.chunks_exact(3) {
        let wa = weld_id[tri[0] as usize];
        let wb = weld_id[tri[1] as usize];
        let wc = weld_id[tri[2] as usize];
        union(&mut parent, wa, wb);
        union(&mut parent, wb, wc);
    }

    let mut root_to_component: HashMap<usize, usize> = HashMap::new();
    let mut vertex_component = vec![0usize; positions.len()];
    for (i, &w) in weld_id.iter().enumerate() {
        let root = find(&mut parent, w);
        let next_id = root_to_component.len();
        let comp = *root_to_component.entry(root).or_insert(next_id);
        vertex_component[i] = comp;
    }

    let count = root_to_component.len();
    (weld_id, vertex_component, count)
}

pub fn triangle_area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let cr = cross(ab, ac);
    0.5 * norm(cr)
}

pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

pub fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = norm(a);
    if n < 1e-12 {
        None
    } else {
        Some([a[0] / n, a[1] / n, a[2] / n])
    }
}
