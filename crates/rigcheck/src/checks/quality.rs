//! Render-quality guards (z-fight, symmetry) and the bed/surface contract.
//! Symmetry and the evidence drift guard are tier-2: they degrade to
//! `skipped` without an evidence manifest, never silently pass.

use crate::checks::{CheckResult, Violation};
use crate::evidence::Evidence;
use crate::geom;
use crate::model::Model;
use crate::spec::MachineSpec;

pub fn run(model: &Model, spec: &MachineSpec, evidence: Option<&Evidence>) -> Vec<CheckResult> {
    let perm = geom::gltf_to_machine_perm(spec.machine.gltf_to_machine);
    let mut results = Vec::new();

    results.push(z_fight(model, spec, perm));
    results.extend(symmetry(model, spec, evidence, perm));
    results.push(surface(model, spec, perm));
    results.push(evidence_drift(model, evidence, perm));

    results
}

fn z_fight(model: &Model, spec: &MachineSpec, perm: [usize; 3]) -> CheckResult {
    let tol = spec.tolerances;
    let mut violations = Vec::new();
    for n in &model.nodes {
        let comps = n.component_aabbs(perm);
        for i in 0..comps.len() {
            for j in (i + 1)..comps.len() {
                let (ci, a) = comps[i];
                let (cj, b) = comps[j];
                for axis in 0..3 {
                    let others: Vec<usize> = (0..3).filter(|&k| k != axis).collect();
                    let flat_overlap = others.iter().all(|&k| {
                        a.min[k] + tol.contact < b.max[k] && b.min[k] + tol.contact < a.max[k]
                    });
                    if !flat_overlap {
                        continue;
                    }
                    if (a.max[axis] - b.max[axis]).abs() < tol.coplanar
                        || (a.min[axis] - b.min[axis]).abs() < tol.coplanar
                    {
                        violations.push(Violation::new(
                            n.name.clone(),
                            format!("z-fight risk: component {ci} and {cj} share a same-facing plane on axis {axis}"),
                        ));
                    }
                }
            }
        }
    }
    CheckResult::from_violations("quality.z_fight", violations)
}

fn symmetry(
    model: &Model,
    spec: &MachineSpec,
    evidence: Option<&Evidence>,
    perm: [usize; 3],
) -> Vec<CheckResult> {
    let Some(sym) = &spec.symmetry else {
        return vec![
            CheckResult::skipped("quality.symmetry_nodes", "no [symmetry] in spec"),
            CheckResult::skipped("quality.symmetry_labels", "no [symmetry] in spec"),
        ];
    };
    let axis = sym.plane.axis.index();

    // tier 1: node-pair translation mirroring.
    let mut violations = Vec::new();
    for [a, b] in &sym.node_pairs {
        let (Some(na), Some(nb)) = (model.node(a), model.node(b)) else {
            continue;
        };
        let ta = na.translation_world_machine(perm)[axis];
        let tb = nb.translation_world_machine(perm)[axis];
        if ((ta + tb) / 2.0 - sym.plane.at).abs() > tolmirror(spec) {
            violations.push(Violation::global(format!(
                "{a} and {b} are not mirrored about axis {axis} at {:.1} (centers {ta:.2}, {tb:.2})",
                sym.plane.at
            )));
        }
    }
    let node_pairs_result = CheckResult::from_violations("quality.symmetry_nodes", violations);

    // tier 2: labeled sub-part mirroring, per node, using evidence.
    let labels_result = match evidence {
        None => CheckResult::skipped("quality.symmetry_labels", "no evidence manifest provided"),
        Some(ev) => {
            let mut violations = Vec::new();
            for (node_name, parts) in &ev.nodes {
                let sym_parts: Vec<&crate::evidence::EvidencePart> = parts
                    .iter()
                    .filter(|p| sym.labels.contains(&p.label))
                    .collect();
                for p in &sym_parts {
                    let cx = (p.min[axis] + p.max[axis]) / 2.0;
                    if (cx - sym.plane.at).abs() < tolmirror(spec) {
                        continue;
                    }
                    let mirror_cx = 2.0 * sym.plane.at - cx;
                    let has_twin = sym_parts.iter().any(|q| {
                        q.label == p.label
                            && ((q.min[axis] + q.max[axis]) / 2.0 - mirror_cx).abs()
                                < tolmirror(spec)
                    });
                    if !has_twin {
                        violations.push(Violation::new(
                            node_name.clone(),
                            format!(
                                "'{}' at axis {axis}={cx:.1} has no mirror twin about {:.1}",
                                p.label, sym.plane.at
                            ),
                        ));
                    }
                }
            }
            CheckResult::from_violations("quality.symmetry_labels", violations)
        }
    };

    vec![node_pairs_result, labels_result]
}

fn tolmirror(spec: &MachineSpec) -> f64 {
    spec.tolerances.mirror
}

fn surface(model: &Model, spec: &MachineSpec, perm: [usize; 3]) -> CheckResult {
    let mut surface_nodes: Vec<(&str, &crate::spec::Surface)> = Vec::new();
    for n in &spec.nodes {
        if let Some(s) = &n.surface {
            surface_nodes.push((&n.name, s));
        }
    }
    if surface_nodes.is_empty() {
        return CheckResult::skipped("quality.surface", "no node declares a [surface] contract");
    }

    let mut violations = Vec::new();
    for (name, surf) in surface_nodes {
        let Some(loaded) = model.node(name) else {
            continue;
        };
        let axis = surf.plane.index();
        let mut unit = [0.0; 3];
        unit[axis] = 1.0;

        let world_gltf = loaded.positions_world_gltf();
        let world_machine = loaded.positions_world_machine(perm);
        let mut top_min = [f64::INFINITY; 3];
        let mut top_max = [f64::NEG_INFINITY; 3];
        let mut found_top = false;
        for tri in loaded.triangles() {
            // `gltf_to_machine` is a reflection (det -1): the true outward
            // normal must be computed in native glTF space (winding is
            // faithful there) and then relabeled into machine space —
            // recomputing the cross product directly from machine-space
            // positions would silently flip its sign.
            let (ga, gb, gc) = (
                world_gltf[tri[0] as usize],
                world_gltf[tri[1] as usize],
                world_gltf[tri[2] as usize],
            );
            let normal_gltf = geom::cross(geom::sub(gb, ga), geom::sub(gc, ga));
            let Some(normal_gltf) = geom::normalize(normal_gltf) else {
                continue;
            };
            let normal = geom::to_machine(normal_gltf, perm);
            let (a, b, c) = (
                world_machine[tri[0] as usize],
                world_machine[tri[1] as usize],
                world_machine[tri[2] as usize],
            );
            if geom::dot(normal, unit) < 0.7 {
                continue;
            }
            let plane_coord = (a[axis] + b[axis] + c[axis]) / 3.0;
            if (plane_coord - surf.at).abs() > spec.tolerances.coplanar.max(0.5) {
                continue;
            }
            found_top = true;
            for p in [a, b, c] {
                for k in 0..3 {
                    top_min[k] = top_min[k].min(p[k]);
                    top_max[k] = top_max[k].max(p[k]);
                }
            }
        }
        if !found_top {
            violations.push(Violation::new(
                name.to_string(),
                format!(
                    "no top-facing geometry found at axis {axis} = {:.1}",
                    surf.at
                ),
            ));
        } else {
            let others: Vec<usize> = (0..3).filter(|&k| k != axis).collect();
            for (idx, &k) in others.iter().enumerate() {
                let (want_lo, want_hi) = (surf.covers_xy[idx][0], surf.covers_xy[idx][1]);
                if top_min[k] > want_lo + 1e-6 || top_max[k] < want_hi - 1e-6 {
                    violations.push(Violation::new(
                        name.to_string(),
                        format!(
                            "surface covers [{:.1},{:.1}] on axis {k}, expected [{:.1},{:.1}]",
                            top_min[k], top_max[k], want_lo, want_hi
                        ),
                    ));
                }
            }
        }

        if let Some(node_aabb) = loaded.aabb_machine(perm) {
            let protrusion = node_aabb.max[axis] - surf.at;
            if protrusion > surf.max_protrusion {
                violations.push(Violation::new(
                    name.to_string(),
                    format!(
                        "geometry rises {protrusion:.2}mm above the surface (budget {:.2}mm)",
                        surf.max_protrusion
                    ),
                ));
            }
        }
    }
    CheckResult::from_violations("quality.surface", violations)
}

fn evidence_drift(model: &Model, evidence: Option<&Evidence>, perm: [usize; 3]) -> CheckResult {
    let Some(ev) = evidence else {
        return CheckResult::skipped("quality.evidence_drift", "no evidence manifest provided");
    };
    let mut violations = Vec::new();
    for (node_name, parts) in &ev.nodes {
        let Some(loaded) = model.node(node_name) else {
            violations.push(Violation::new(
                node_name.clone(),
                "evidence references a node not present in the spec/model".to_string(),
            ));
            continue;
        };
        let comps = loaded.component_aabbs(perm);
        for p in parts {
            let matches = comps
                .iter()
                .any(|(_, aabb)| crate::evidence::aabb_matches(aabb, p));
            if !matches {
                violations.push(Violation::new(
                    node_name.clone(),
                    format!(
                        "evidence part '{}' ({:?}..{:?}) has no matching mesh component — manifest may be stale",
                        p.label, p.min, p.max
                    ),
                ));
            }
        }
    }
    CheckResult::from_violations("quality.evidence_drift", violations)
}
