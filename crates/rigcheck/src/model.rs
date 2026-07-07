//! Loads a GLB and, for each node named in the spec, extracts local mesh
//! data plus the node's accumulated world transform (glTF space).

use std::collections::HashMap;

use crate::geom::{self, Mat4};
use crate::spec::MachineSpec;

pub struct LoadedNode {
    pub name: String,
    /// Untransformed (local) positions, glTF space.
    pub positions_local: Vec<[f64; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Flat triangle list (indices into `positions_local`).
    pub indices: Vec<u32>,
    /// Accumulated world transform (glTF space) from the scene root.
    pub transform: Mat4,
    /// Fine-grained per-position id (distinct per welded position) — what
    /// edge/manifold checks need.
    pub vertex_weld_id: Vec<usize>,
    /// Connected-component id per vertex, from welding local positions and
    /// unioning across triangle adjacency — what AABB/sweep grouping needs.
    pub vertex_component: Vec<usize>,
    pub component_count: usize,
}

impl LoadedNode {
    pub fn positions_world_gltf(&self) -> Vec<[f64; 3]> {
        self.positions_local
            .iter()
            .map(|p| self.transform.transform_point(*p))
            .collect()
    }

    pub fn positions_local_machine(&self, perm: [usize; 3]) -> Vec<[f64; 3]> {
        self.positions_local
            .iter()
            .map(|p| geom::to_machine(*p, perm))
            .collect()
    }

    pub fn positions_world_machine(&self, perm: [usize; 3]) -> Vec<[f64; 3]> {
        self.positions_world_gltf()
            .iter()
            .map(|p| geom::to_machine(*p, perm))
            .collect()
    }

    pub fn translation_world_machine(&self, perm: [usize; 3]) -> [f64; 3] {
        geom::to_machine(self.transform.translation(), perm)
    }

    /// Component id containing a vertex within `eps` of the local origin —
    /// the black-box "nozzle tip" / anchor-point selector.
    pub fn origin_component(&self, eps: f64) -> Option<usize> {
        self.positions_local
            .iter()
            .position(|p| p[0].abs() < eps && p[1].abs() < eps && p[2].abs() < eps)
            .map(|vi| self.vertex_component[vi])
    }

    /// Machine-space AABB of a specific connected component (world pose).
    pub fn component_aabb_machine(&self, perm: [usize; 3], component: usize) -> Option<geom::Aabb> {
        let world = self.positions_world_machine(perm);
        geom::Aabb::from_points(
            world
                .iter()
                .enumerate()
                .filter(|(vi, _)| self.vertex_component[*vi] == component)
                .map(|(_, p)| p),
        )
    }

    pub fn aabb_machine(&self, perm: [usize; 3]) -> Option<geom::Aabb> {
        let world = self.positions_world_machine(perm);
        geom::Aabb::from_points(world.iter())
    }

    pub fn component_aabbs(&self, perm: [usize; 3]) -> Vec<(usize, geom::Aabb)> {
        (0..self.component_count)
            .filter_map(|c| self.component_aabb_machine(perm, c).map(|a| (c, a)))
            .collect()
    }

    pub fn triangle_component(&self, tri_index: usize) -> usize {
        let vi = self.indices[tri_index * 3] as usize;
        self.vertex_component[vi]
    }

    pub fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        self.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]])
    }
}

pub struct Model {
    pub nodes: Vec<LoadedNode>,
    pub triangle_total: u64,
    pub material_total: u64,
    pub texture_total: u64,
}

impl Model {
    pub fn node(&self, name: &str) -> Option<&LoadedNode> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

fn global_transforms(doc: &gltf::Document) -> HashMap<usize, Mat4> {
    fn walk(node: gltf::Node, parent: Mat4, out: &mut HashMap<usize, Mat4>) {
        let local = Mat4::from_f32_cols(node.transform().matrix());
        let world = parent.mul(&local);
        out.insert(node.index(), world);
        for child in node.children() {
            walk(child, world, out);
        }
    }
    let mut out = HashMap::new();
    for scene in doc.scenes() {
        for node in scene.nodes() {
            walk(node, Mat4::IDENTITY, &mut out);
        }
    }
    out
}

pub fn load(glb_path: &std::path::Path, spec: &MachineSpec) -> anyhow::Result<Model> {
    let (document, buffers, _images) = gltf::import(glb_path)
        .map_err(|e| anyhow::anyhow!("loading {}: {e}", glb_path.display()))?;

    let transforms = global_transforms(&document);
    let weld_tol = spec.tolerances.weld;

    let mut nodes = Vec::new();
    for node_spec in &spec.nodes {
        let gnode = document
            .nodes()
            .find(|n| n.name() == Some(node_spec.name.as_str()));
        let Some(gnode) = gnode else {
            // Recorded as a missing node in the structure check; skip loading.
            continue;
        };
        let transform = *transforms.get(&gnode.index()).unwrap_or(&Mat4::IDENTITY);
        let mut positions_local = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();
        if let Some(mesh) = gnode.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|b| Some(&buffers[b.index()]));
                let base = positions_local.len() as u32;
                if let Some(iter) = reader.read_positions() {
                    positions_local.extend(iter.map(|p| [p[0] as f64, p[1] as f64, p[2] as f64]));
                }
                if let Some(iter) = reader.read_tex_coords(0) {
                    uvs.extend(iter.into_f32());
                }
                if let Some(iter) = reader.read_indices() {
                    indices.extend(iter.into_u32().map(|i| i + base));
                }
            }
        }
        let (vertex_weld_id, vertex_component, component_count) =
            geom::weld_and_components(&positions_local, &indices, weld_tol);
        nodes.push(LoadedNode {
            name: node_spec.name.clone(),
            positions_local,
            uvs,
            indices,
            transform,
            vertex_weld_id,
            vertex_component,
            component_count,
        });
    }

    let triangle_total: u64 = document
        .meshes()
        .flat_map(|m| m.primitives())
        .map(|p| p.indices().map(|a| a.count() as u64 / 3).unwrap_or(0))
        .sum();
    let material_total = document.materials().len() as u64;
    let texture_total = document.textures().len() as u64;

    Ok(Model {
        nodes,
        triangle_total,
        material_total,
        texture_total,
    })
}
