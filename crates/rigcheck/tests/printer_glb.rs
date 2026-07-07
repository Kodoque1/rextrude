//! Runs the full check suite against the committed `printer.glb` and its
//! sidecars, so `cargo test --workspace` guards the asset even before the
//! dedicated CI step. Also pins per-node component counts, so an exporter
//! behavior change (e.g. Blender starting to merge sub-parts) is caught.

use std::collections::HashMap;
use std::path::PathBuf;

use rigcheck::checks::{hygiene, quality, structure, sweep, Status};

fn model_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/assets/models")
}

#[test]
fn printer_glb_passes_full_suite() {
    let dir = model_dir();
    let glb = dir.join("printer.glb");
    let spec = rigcheck::spec::load(&dir.join("printer.machine.toml")).expect("load spec");
    let evidence =
        rigcheck::evidence::load(&dir.join("printer.evidence.json")).expect("load evidence");
    let model = rigcheck::model::load(&glb, &spec).expect("load model");

    let mut results = Vec::new();
    results.extend(structure::run(&model, &spec));
    results.extend(hygiene::run(&model, &spec));
    results.extend(sweep::run(&model, &spec, Some(&evidence)));
    results.extend(quality::run(&model, &spec, Some(&evidence)));

    let failed: Vec<String> = results
        .iter()
        .filter(|r| r.status == Status::Fail)
        .map(|r| format!("{}: {:?}", r.id, r.violations))
        .collect();
    assert!(
        failed.is_empty(),
        "expected all checks to pass, failed: {failed:#?}"
    );
    assert!(
        !results.iter().any(|r| r.status == Status::Skipped),
        "expected no skipped checks with evidence present"
    );
}

#[test]
fn printer_glb_component_counts_are_pinned() {
    let dir = model_dir();
    let spec = rigcheck::spec::load(&dir.join("printer.machine.toml")).expect("load spec");
    let model = rigcheck::model::load(&dir.join("printer.glb"), &spec).expect("load model");

    let expected: HashMap<&str, usize> = HashMap::from([
        ("Frame_Static", 16),
        ("Gantry_X", 8),
        ("Carriage_X", 10),
        ("Bed_Y", 10),
        ("LeadScrew_L", 1),
        ("LeadScrew_R", 1),
    ]);

    for (name, expected_count) in expected {
        let node = model
            .node(name)
            .unwrap_or_else(|| panic!("node {name} missing from model"));
        assert_eq!(
            node.component_count, expected_count,
            "{name}: component count changed (exporter behavior may have changed) — \
             if this is an intentional model change, update this pinned count"
        );
    }
}
