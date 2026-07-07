//! Required nodes, spinner/contract node contracts, and budgets — the
//! cheapest checks to generalize, ported directly from the Python
//! `validate_design`/`validate_glb` pair.

use crate::checks::{CheckResult, Violation};
use crate::geom::{self};
use crate::model::Model;
use crate::spec::{MachineSpec, Role};

const EPS: f64 = 1e-6;

pub fn run(model: &Model, spec: &MachineSpec) -> Vec<CheckResult> {
    let perm = geom::gltf_to_machine_perm(spec.machine.gltf_to_machine);
    let mut results = Vec::new();

    // --- required nodes ----------------------------------------------------
    let mut missing = Vec::new();
    let mut empty = Vec::new();
    for n in &spec.nodes {
        match model.node(&n.name) {
            None => missing.push(n.name.clone()),
            Some(loaded) if loaded.positions_local.is_empty() => empty.push(n.name.clone()),
            Some(_) => {}
        }
    }
    let mut violations: Vec<Violation> = missing
        .iter()
        .map(|n| Violation::new(n.clone(), "required node missing from GLB"))
        .collect();
    violations.extend(
        empty
            .iter()
            .map(|n| Violation::new(n.clone(), "node present but has no mesh geometry")),
    );
    results.push(CheckResult::from_violations(
        "structure.required_nodes",
        violations,
    ));

    // --- spinner contracts ---------------------------------------------------
    let mut violations = Vec::new();
    for n in spec.nodes.iter().filter(|n| n.role == Role::Spinner) {
        let Some(loaded) = model.node(&n.name) else {
            continue;
        };
        let Some(spin) = &n.spin else { continue };
        let gltf_axis = perm[spin.axis.index()];
        let (i, j) = match gltf_axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let radial = loaded
            .positions_local
            .iter()
            .map(|p| p[i].abs().max(p[j].abs()))
            .fold(0.0_f64, f64::max);
        if radial > spin.max_radial_extent {
            violations.push(Violation::new(
                n.name.clone(),
                format!(
                    "geometry {radial:.2}mm off its spin axis (must be within {:.2}mm)",
                    spin.max_radial_extent
                ),
            ));
        }
        if spin.translation_required {
            let world = loaded.translation_world_machine(perm);
            let others: Vec<f64> = (0..3)
                .filter(|&a| a != spin.axis.index())
                .map(|a| world[a])
                .collect();
            if others.iter().all(|c| c.abs() < EPS) {
                violations.push(Violation::new(
                    n.name.clone(),
                    "node translation looks baked into geometry (position collapsed to the spin axis)",
                ));
            }
        }
    }
    results.push(CheckResult::from_violations(
        "structure.spinner",
        violations,
    ));

    // --- node contracts (e.g. carriage nozzle-tip-at-origin) ----------------
    let mut violations = Vec::new();
    for n in &spec.nodes {
        let Some(contract) = &n.contract else {
            continue;
        };
        let Some(loaded) = model.node(&n.name) else {
            continue;
        };
        if contract.vertex_at_origin {
            let has_origin = loaded
                .positions_local
                .iter()
                .any(|p| p[0].abs() < EPS && p[1].abs() < EPS && p[2].abs() < EPS);
            if !has_origin {
                violations.push(Violation::new(
                    n.name.clone(),
                    "no vertex at the local origin",
                ));
            }
        }
        if let Some(min_local) = contract.min_local {
            let local_machine = loaded.positions_local_machine(perm);
            for axis in 0..3 {
                let floor = min_local[axis];
                if floor.is_nan() {
                    continue;
                }
                if let Some(lo) = local_machine
                    .iter()
                    .map(|p| p[axis])
                    .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.min(v))))
                {
                    if lo < floor - EPS {
                        violations.push(Violation::new(
                            n.name.clone(),
                            format!("geometry extends to {lo:.2} on machine axis {axis}, below floor {floor:.2}"),
                        ));
                    }
                }
            }
        }
    }
    results.push(CheckResult::from_violations(
        "structure.contract",
        violations,
    ));

    // --- budgets -------------------------------------------------------------
    let mut violations = Vec::new();
    if let Some(max_tris) = spec.budgets.max_triangles {
        if model.triangle_total > max_tris {
            violations.push(Violation::global(format!(
                "triangle budget exceeded: {} > {max_tris}",
                model.triangle_total
            )));
        }
    }
    if let Some(max_mats) = spec.budgets.max_materials {
        if model.material_total > max_mats {
            violations.push(Violation::global(format!(
                "material budget exceeded: {} > {max_mats}",
                model.material_total
            )));
        }
    }
    if let Some(max_tex) = spec.budgets.max_textures {
        if model.texture_total > max_tex {
            violations.push(Violation::global(format!(
                "texture budget exceeded: {} > {max_tex}",
                model.texture_total
            )));
        }
    }
    results.push(CheckResult::from_violations(
        "structure.budgets",
        violations,
    ));

    results
}
