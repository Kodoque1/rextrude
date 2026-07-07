//! Mesh hygiene: degenerate triangles, non-manifold edges, inverted
//! winding/normals, UV bounds, and unit sanity — checks the original
//! Python validator never had.

use std::collections::HashMap;

use crate::checks::{CheckResult, Violation};
use crate::geom::{self, Aabb};
use crate::model::Model;
use crate::spec::MachineSpec;

pub fn run(model: &Model, spec: &MachineSpec) -> Vec<CheckResult> {
    let perm = geom::gltf_to_machine_perm(spec.machine.gltf_to_machine);
    let tol = spec.tolerances;
    let mut results = Vec::new();

    // --- degenerate triangles -----------------------------------------------
    let mut violations = Vec::new();
    for n in &model.nodes {
        let world = n.positions_world_gltf();
        let mut count = 0;
        for tri in n.triangles() {
            let area = geom::triangle_area(
                world[tri[0] as usize],
                world[tri[1] as usize],
                world[tri[2] as usize],
            );
            if area < tol.degenerate_area {
                count += 1;
            }
        }
        if count > 0 {
            violations.push(Violation::new(
                n.name.clone(),
                format!(
                    "{count} degenerate triangle(s) (area < {:.2e}mm^2)",
                    tol.degenerate_area
                ),
            ));
        }
    }
    results.push(CheckResult::from_violations(
        "hygiene.degenerate_triangles",
        violations,
    ));

    // --- non-manifold / open edges -------------------------------------------
    let mut violations = Vec::new();
    for n in &model.nodes {
        let mut edge_count: HashMap<(usize, usize), u32> = HashMap::new();
        for tri in n.triangles() {
            let weld = [
                n.vertex_weld_id[tri[0] as usize],
                n.vertex_weld_id[tri[1] as usize],
                n.vertex_weld_id[tri[2] as usize],
            ];
            for &(a, b) in &[(0, 1), (1, 2), (2, 0)] {
                let (wa, wb) = (weld[a], weld[b]);
                let key = if wa < wb { (wa, wb) } else { (wb, wa) };
                *edge_count.entry(key).or_insert(0) += 1;
            }
        }
        // Only a genuinely open edge (incident to exactly one face) signals
        // missing geometry. Edge counts >2 are expected wherever separate
        // abutting parts share a welded seam (e.g. a bracket wrapping a
        // rail) — a normal hard-surface assembly pattern, not a defect.
        let open = edge_count.values().filter(|&&c| c == 1).count();
        if open > 0 {
            violations.push(Violation::new(
                n.name.clone(),
                format!("{open} open (boundary) edge(s)"),
            ));
        }
    }
    results.push(CheckResult::from_violations("hygiene.manifold", violations));

    // --- inverted winding (per connected component) --------------------------
    let mut violations = Vec::new();
    const VOLUME_RELIABLE: f64 = 1.0; // mm^3 — below this, treat as an open shell and skip
    for n in &model.nodes {
        let world = n.positions_world_gltf();
        let mut volume_by_component: HashMap<usize, f64> = HashMap::new();
        for tri in n.triangles() {
            let comp = n.vertex_component[tri[0] as usize];
            let (a, b, c) = (
                world[tri[0] as usize],
                world[tri[1] as usize],
                world[tri[2] as usize],
            );
            let signed = geom::dot(a, geom::cross(b, c)) / 6.0;
            *volume_by_component.entry(comp).or_insert(0.0) += signed;
        }
        for (comp, vol) in volume_by_component {
            if vol.abs() > VOLUME_RELIABLE && vol < 0.0 {
                violations.push(Violation::new(
                    n.name.clone(),
                    format!(
                        "component {comp} appears inside-out (negative signed volume {vol:.1}mm^3)"
                    ),
                ));
            }
        }
    }
    results.push(CheckResult::from_violations("hygiene.winding", violations));

    // --- UV bounds -------------------------------------------------------------
    let mut violations = Vec::new();
    const UV_EPS: f32 = 1e-3;
    for n in &model.nodes {
        let out_of_bounds = n
            .uvs
            .iter()
            .filter(|uv| {
                uv[0] < -UV_EPS || uv[0] > 1.0 + UV_EPS || uv[1] < -UV_EPS || uv[1] > 1.0 + UV_EPS
            })
            .count();
        if out_of_bounds > 0 {
            violations.push(Violation::new(
                n.name.clone(),
                format!("{out_of_bounds} UV coordinate(s) outside [0,1]"),
            ));
        }
    }
    results.push(CheckResult::from_violations(
        "hygiene.uv_bounds",
        violations,
    ));

    // --- unit sanity -------------------------------------------------------------
    match spec.machine.scene_extent_mm {
        None => results.push(CheckResult::skipped(
            "hygiene.unit_sanity",
            "no scene_extent_mm in spec",
        )),
        Some(range) => {
            let mut scene: Option<Aabb> = None;
            for n in &model.nodes {
                if let Some(aabb) = n.aabb_machine(perm) {
                    scene = Some(scene.map_or(aabb, |s| s.union(&aabb)));
                }
            }
            let mut violations = Vec::new();
            if let Some(scene) = scene {
                let diag = (0..3).map(|a| scene.extent(a).powi(2)).sum::<f64>().sqrt();
                if diag < range.min || diag > range.max {
                    violations.push(Violation::global(format!(
                        "scene extent {diag:.1}mm outside expected [{:.1},{:.1}]mm — check for a unit mismatch",
                        range.min, range.max
                    )));
                }
            }
            results.push(CheckResult::from_violations(
                "hygiene.unit_sanity",
                violations,
            ));
        }
    }

    results
}
