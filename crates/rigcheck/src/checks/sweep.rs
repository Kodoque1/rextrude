//! Kinematic travel-envelope collision sweep: at every corner pose of the
//! declared gcode envelope, displace each moving node by its motion map and
//! flag interpenetration between components that shouldn't touch.
//!
//! Ported from the Python `_overlap`/`_collisions` sweep, at connected
//! -component granularity (whole-node AABBs provably false-positive: the
//! carriage and bed AABBs overlap at the envelope origin because of the
//! bed's corner notch, even though the actual solids never touch).

use std::collections::HashSet;

use crate::checks::{CheckResult, Violation};
use crate::evidence::{aabb_matches, Evidence};
use crate::geom::{self, Aabb};
use crate::model::Model;
use crate::spec::{AllowContact, MachineSpec, Motion, Role};

#[derive(Clone)]
enum Selector {
    Whole,
    Components(HashSet<usize>),
}

impl Selector {
    fn matches(&self, comp: usize) -> bool {
        match self {
            Selector::Whole => true,
            Selector::Components(set) => set.contains(&comp),
        }
    }
}

struct ResolvedRef {
    node: String,
    selector: Selector,
}

/// Resolves a spec `PartRef` to the mesh component(s) it selects.
/// `part = "origin_component"` works black-box (no evidence needed);
/// `label = "..."` needs the evidence manifest to map a label to the
/// component(s) whose geometry matches it. Either falls back to `Whole`
/// (the more permissive option) when it can't be resolved, so omitting
/// evidence degrades allowances rather than making the sweep stricter.
fn resolve_ref(
    model: &Model,
    evidence: Option<&Evidence>,
    perm: [usize; 3],
    r: &crate::spec::PartRef,
) -> ResolvedRef {
    let selector = if r.part.as_deref() == Some("origin_component") {
        model
            .node(&r.node)
            .and_then(|n| n.origin_component(1e-6))
            .map(|c| Selector::Components(HashSet::from([c])))
            .unwrap_or(Selector::Whole)
    } else if let Some(label) = &r.label {
        match (evidence, model.node(&r.node)) {
            (Some(ev), Some(node)) => {
                let comps = node.component_aabbs(perm);
                let matched: HashSet<usize> = ev
                    .nodes
                    .get(&r.node)
                    .into_iter()
                    .flatten()
                    .filter(|p| &p.label == label)
                    .filter_map(|p| {
                        comps
                            .iter()
                            .find(|(_, aabb)| aabb_matches(aabb, p))
                            .map(|(c, _)| *c)
                    })
                    .collect();
                if matched.is_empty() {
                    Selector::Whole
                } else {
                    Selector::Components(matched)
                }
            }
            _ => Selector::Whole,
        }
    } else {
        Selector::Whole
    };
    ResolvedRef {
        node: r.node.clone(),
        selector,
    }
}

fn is_allowed(
    resolved: &[(ResolvedRef, ResolvedRef)],
    node_i: &str,
    comp_i: usize,
    node_j: &str,
    comp_j: usize,
) -> bool {
    resolved.iter().any(|(ra, rb)| {
        (ra.node == node_i
            && ra.selector.matches(comp_i)
            && rb.node == node_j
            && rb.selector.matches(comp_j))
            || (rb.node == node_i
                && rb.selector.matches(comp_i)
                && ra.node == node_j
                && ra.selector.matches(comp_j))
    })
}

fn is_moving_pair(role_a: Role, role_b: Role) -> bool {
    role_a == Role::Moving || role_b == Role::Moving
}

pub fn run(model: &Model, spec: &MachineSpec, evidence: Option<&Evidence>) -> Vec<CheckResult> {
    let perm = geom::gltf_to_machine_perm(spec.machine.gltf_to_machine);

    struct NodeInfo<'a> {
        name: &'a str,
        role: Role,
        motion: Motion,
        components: Vec<(usize, Aabb)>,
    }

    let mut nodes: Vec<NodeInfo> = Vec::new();
    for ns in &spec.nodes {
        let Some(loaded) = model.node(&ns.name) else {
            continue;
        };
        nodes.push(NodeInfo {
            name: &ns.name,
            role: ns.role,
            motion: ns.motion.unwrap_or_default(),
            components: loaded.component_aabbs(perm),
        });
    }

    let resolved_allow: Vec<(ResolvedRef, ResolvedRef)> = spec
        .allow_contact
        .iter()
        .map(|ac: &AllowContact| {
            (
                resolve_ref(model, evidence, perm, &ac.a),
                resolve_ref(model, evidence, perm, &ac.b),
            )
        })
        .collect();

    let mut violations = Vec::new();
    for pose in spec.envelope.corners() {
        let displaced: Vec<(&str, usize, Aabb)> = nodes
            .iter()
            .flat_map(|n| {
                let d = n.motion.displacement(pose);
                n.components
                    .iter()
                    .map(move |(c, aabb)| (n.name, *c, aabb.translated(d)))
            })
            .collect();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                if !is_moving_pair(nodes[i].role, nodes[j].role) {
                    continue;
                }
                let name_i = nodes[i].name;
                let name_j = nodes[j].name;
                for &(node_a, comp_a, aabb_a) in displaced.iter().filter(|(n, ..)| *n == name_i) {
                    for &(node_b, comp_b, aabb_b) in displaced.iter().filter(|(n, ..)| *n == name_j)
                    {
                        if is_allowed(&resolved_allow, node_a, comp_a, node_b, comp_b) {
                            continue;
                        }
                        if aabb_a.overlaps(&aabb_b, spec.tolerances.contact) {
                            violations.push(Violation::new(
                                node_a.to_string(),
                                format!(
                                    "collision at gcode ({:.0},{:.0},{:.0}): {node_a}#{comp_a} vs {node_b}#{comp_b}",
                                    pose[0], pose[1], pose[2]
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    vec![CheckResult::from_violations("sweep.collision", violations)]
}
