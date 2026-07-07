//! One fixture per check family, built directly as in-memory `Model`/`MachineSpec`
//! values (no GLB round-trip needed — that's covered by `printer_glb.rs`,
//! which exercises the real committed asset end-to-end).

use rigcheck::checks::{hygiene, quality, structure, sweep, Status};
use rigcheck::evidence::{Evidence, EvidencePart};
use rigcheck::geom::{self, Mat4};
use rigcheck::model::{LoadedNode, Model};
use rigcheck::spec::*;
use std::collections::HashMap;

const IDENTITY_PERM: [usize; 3] = [0, 1, 2];

fn identity_gltf_to_machine() -> [Axis; 3] {
    [Axis::X, Axis::Y, Axis::Z]
}

fn machine(name: &str) -> Machine {
    Machine {
        name: name.to_string(),
        units: "mm".to_string(),
        gltf_to_machine: identity_gltf_to_machine(),
        scene_extent_mm: None,
    }
}

fn envelope() -> Envelope {
    Envelope {
        x: [0.0, 0.0],
        y: [0.0, 0.0],
        z: [0.0, 0.0],
    }
}

fn base_spec(nodes: Vec<NodeSpec>) -> MachineSpec {
    MachineSpec {
        machine: machine("test"),
        envelope: envelope(),
        budgets: Budgets::default(),
        symmetry: None,
        tolerances: Tolerances::default(),
        nodes,
        allow_contact: vec![],
    }
}

fn node_spec(name: &str, role: Role) -> NodeSpec {
    NodeSpec {
        name: name.to_string(),
        role,
        motion: None,
        contract: None,
        surface: None,
        spin: None,
    }
}

/// A closed, outward-winding box (CCW-from-outside, valid in any
/// right-handed frame) — 8 shared vertices, 12 triangles.
fn box_mesh(min: [f64; 3], max: [f64; 3]) -> (Vec<[f64; 3]>, Vec<u32>) {
    let (x0, y0, z0) = (min[0], min[1], min[2]);
    let (x1, y1, z1) = (max[0], max[1], max[2]);
    let positions = vec![
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ];
    let quads: [[u32; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut indices = Vec::new();
    for q in quads {
        indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
    }
    (positions, indices)
}

fn make_node(
    name: &str,
    positions: Vec<[f64; 3]>,
    indices: Vec<u32>,
    uvs: Vec<[f32; 2]>,
    translation: [f64; 3],
) -> LoadedNode {
    let (vertex_weld_id, vertex_component, component_count) =
        geom::weld_and_components(&positions, &indices, 1e-4);
    let mut transform = Mat4::IDENTITY;
    transform.0[3][0] = translation[0];
    transform.0[3][1] = translation[1];
    transform.0[3][2] = translation[2];
    LoadedNode {
        name: name.to_string(),
        positions_local: positions,
        uvs,
        indices,
        transform,
        vertex_weld_id,
        vertex_component,
        component_count,
    }
}

fn model_of(nodes: Vec<LoadedNode>) -> Model {
    let triangle_total = nodes.iter().map(|n| n.indices.len() as u64 / 3).sum();
    Model {
        nodes,
        triangle_total,
        material_total: 1,
        texture_total: 1,
    }
}

#[test]
fn missing_node_fails_structure() {
    let spec = base_spec(vec![
        node_spec("Present", Role::Static),
        node_spec("Absent", Role::Static),
    ]);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    let model = model_of(vec![make_node("Present", p, i, vec![], [0.0; 3])]);

    let results = structure::run(&model, &spec);
    let r = results
        .iter()
        .find(|r| r.id == "structure.required_nodes")
        .unwrap();
    assert_eq!(r.status, Status::Fail);
    assert!(r
        .violations
        .iter()
        .any(|v| v.message.contains("missing") && v.node.as_deref() == Some("Absent")));
}

#[test]
fn open_edge_fails_manifold() {
    let spec = base_spec(vec![node_spec("Holey", Role::Static)]);
    let (p, mut i) = box_mesh([0.0; 3], [10.0; 3]);
    i.truncate(i.len() - 3); // drop the last triangle -> a hole
    let model = model_of(vec![make_node("Holey", p, i, vec![], [0.0; 3])]);

    let r = hygiene::run(&model, &spec);
    let manifold = r.iter().find(|r| r.id == "hygiene.manifold").unwrap();
    assert_eq!(manifold.status, Status::Fail);
    assert!(manifold
        .violations
        .iter()
        .any(|v| v.message.contains("open")));
}

#[test]
fn closed_box_passes_manifold() {
    let spec = base_spec(vec![node_spec("Closed", Role::Static)]);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    let model = model_of(vec![make_node("Closed", p, i, vec![], [0.0; 3])]);

    let r = hygiene::run(&model, &spec);
    let manifold = r.iter().find(|r| r.id == "hygiene.manifold").unwrap();
    assert_eq!(manifold.status, Status::Pass);
}

#[test]
fn flipped_winding_fails() {
    let spec = base_spec(vec![node_spec("Inverted", Role::Static)]);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    // Swap the last two indices of every triangle to invert winding.
    let flipped: Vec<u32> = i.chunks_exact(3).flat_map(|t| [t[0], t[2], t[1]]).collect();
    let model = model_of(vec![make_node("Inverted", p, flipped, vec![], [0.0; 3])]);

    let r = hygiene::run(&model, &spec);
    let winding = r.iter().find(|r| r.id == "hygiene.winding").unwrap();
    assert_eq!(winding.status, Status::Fail);
}

#[test]
fn uv_out_of_bounds_fails() {
    let spec = base_spec(vec![node_spec("Uved", Role::Static)]);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    let mut uvs = vec![[0.0f32, 0.0]; 8];
    uvs[0] = [1.5, 0.0];
    let model = model_of(vec![make_node("Uved", p, i, uvs, [0.0; 3])]);

    let r = hygiene::run(&model, &spec);
    let uv = r.iter().find(|r| r.id == "hygiene.uv_bounds").unwrap();
    assert_eq!(uv.status, Status::Fail);
}

#[test]
fn triangle_budget_exceeded_fails() {
    let mut spec = base_spec(vec![node_spec("Box", Role::Static)]);
    spec.budgets.max_triangles = Some(1);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]); // 12 triangles
    let model = model_of(vec![make_node("Box", p, i, vec![], [0.0; 3])]);

    let r = structure::run(&model, &spec);
    let budgets = r.iter().find(|r| r.id == "structure.budgets").unwrap();
    assert_eq!(budgets.status, Status::Fail);
}

#[test]
fn spinner_off_axis_fails() {
    let mut moving = node_spec("Screw", Role::Spinner);
    moving.spin = Some(Spin {
        axis: Axis::Z,
        max_radial_extent: 1.0,
        translation_required: false,
    });
    let spec = base_spec(vec![moving]);
    // Box centered at origin, 10mm wide in x/y — radial extent 5mm > budget of 1mm.
    let (p, i) = box_mesh([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]);
    let model = model_of(vec![make_node("Screw", p, i, vec![], [0.0; 3])]);

    let r = structure::run(&model, &spec);
    let spinner = r.iter().find(|r| r.id == "structure.spinner").unwrap();
    assert_eq!(spinner.status, Status::Fail);
}

#[test]
fn sweep_flags_uncleared_collision() {
    let mut a = node_spec("A", Role::Moving);
    a.motion = Some(Motion {
        offset: [0.0; 3],
        per_gx: [0.0; 3],
        per_gy: [0.0; 3],
        per_gz: [0.0; 3],
    });
    let b = node_spec("B", Role::Static);
    let mut spec = base_spec(vec![a, b]);
    spec.envelope = Envelope {
        x: [0.0, 0.0],
        y: [0.0, 0.0],
        z: [0.0, 0.0],
    };

    let (pa, ia) = box_mesh([0.0; 3], [10.0; 3]);
    let (pb, ib) = box_mesh([5.0, 5.0, 5.0], [15.0, 15.0, 15.0]); // overlaps A
    let model = model_of(vec![
        make_node("A", pa, ia, vec![], [0.0; 3]),
        make_node("B", pb, ib, vec![], [0.0; 3]),
    ]);

    let r = sweep::run(&model, &spec, None);
    let collision = r.iter().find(|r| r.id == "sweep.collision").unwrap();
    assert_eq!(collision.status, Status::Fail);
}

#[test]
fn sweep_respects_allow_contact() {
    let mut a = node_spec("A", Role::Moving);
    a.motion = Some(Motion::default());
    let b = node_spec("B", Role::Static);
    let mut spec = base_spec(vec![a, b]);
    spec.allow_contact = vec![AllowContact {
        a: PartRef {
            node: "A".to_string(),
            part: None,
            label: None,
        },
        b: PartRef {
            node: "B".to_string(),
            part: None,
            label: None,
        },
    }];

    let (pa, ia) = box_mesh([0.0; 3], [10.0; 3]);
    let (pb, ib) = box_mesh([5.0, 5.0, 5.0], [15.0, 15.0, 15.0]);
    let model = model_of(vec![
        make_node("A", pa, ia, vec![], [0.0; 3]),
        make_node("B", pb, ib, vec![], [0.0; 3]),
    ]);

    let r = sweep::run(&model, &spec, None);
    let collision = r.iter().find(|r| r.id == "sweep.collision").unwrap();
    assert_eq!(collision.status, Status::Pass);
}

#[test]
fn coplanar_same_facing_components_fail_zfight() {
    let spec = base_spec(vec![node_spec("Pair", Role::Static)]);
    let (mut p, mut i) = box_mesh([0.0; 3], [10.0; 3]);
    let (p2, i2) = box_mesh([5.0, 5.0, 0.0], [15.0, 15.0, 10.0]); // same z range, offset in x/y, overlapping
    let base = p.len() as u32;
    p.extend(p2);
    i.extend(i2.iter().map(|idx| idx + base));
    let model = model_of(vec![make_node("Pair", p, i, vec![], [0.0; 3])]);

    let r = quality::run(&model, &spec, None);
    let zfight = r.iter().find(|r| r.id == "quality.z_fight").unwrap();
    assert_eq!(zfight.status, Status::Fail);
}

#[test]
fn symmetry_label_without_twin_fails() {
    let mut spec = base_spec(vec![node_spec("Frame", Role::Static)]);
    spec.symmetry = Some(Symmetry {
        plane: Plane {
            axis: Axis::X,
            at: 100.0,
        },
        labels: vec!["post".to_string()],
        node_pairs: vec![],
    });
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    let model = model_of(vec![make_node("Frame", p, i, vec![], [0.0; 3])]);

    let mut nodes = HashMap::new();
    nodes.insert(
        "Frame".to_string(),
        vec![EvidencePart {
            label: "post".to_string(),
            min: [0.0, 0.0, 0.0],
            max: [10.0, 10.0, 10.0],
        }],
    );
    let evidence = Evidence {
        version: 1,
        space: "machine".to_string(),
        nodes,
    };

    let r = quality::run(&model, &spec, Some(&evidence));
    let sym = r
        .iter()
        .find(|r| r.id == "quality.symmetry_labels")
        .unwrap();
    assert_eq!(sym.status, Status::Fail);
}

#[test]
fn stale_evidence_fails_drift_guard() {
    let spec = base_spec(vec![node_spec("Frame", Role::Static)]);
    let (p, i) = box_mesh([0.0; 3], [10.0; 3]);
    let model = model_of(vec![make_node("Frame", p, i, vec![], [0.0; 3])]);

    let mut nodes = HashMap::new();
    nodes.insert(
        "Frame".to_string(),
        vec![EvidencePart {
            label: "ghost".to_string(),
            min: [100.0, 100.0, 100.0],
            max: [110.0, 110.0, 110.0],
        }],
    );
    let evidence = Evidence {
        version: 1,
        space: "machine".to_string(),
        nodes,
    };

    let r = quality::run(&model, &spec, Some(&evidence));
    let drift = r.iter().find(|r| r.id == "quality.evidence_drift").unwrap();
    assert_eq!(drift.status, Status::Fail);
}

#[test]
fn identity_perm_is_used_consistently() {
    // Sanity check that IDENTITY_PERM matches the identity gltf_to_machine spec.
    assert_eq!(
        geom::gltf_to_machine_perm(identity_gltf_to_machine()),
        IDENTITY_PERM
    );
}
