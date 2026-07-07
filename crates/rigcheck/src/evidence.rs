//! Optional tier-2 evidence manifest: labeled machine-space AABBs emitted by
//! a generator alongside the GLB (e.g. `tools/gen_printer_assets.py`),
//! enabling checks that need sub-part semantics a flattened mesh can't carry
//! on its own (symmetry-by-label, exact sub-part contracts).

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Evidence {
    pub version: u32,
    pub space: String,
    pub nodes: HashMap<String, Vec<EvidencePart>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvidencePart {
    pub label: String,
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// Whether a mesh component's AABB matches an evidence-recorded AABB
/// closely enough to be considered "the same part" (drift guard + label
/// selector resolution share this tolerance).
pub fn aabb_matches(aabb: &crate::geom::Aabb, part: &EvidencePart) -> bool {
    (0..3)
        .all(|k| (aabb.min[k] - part.min[k]).abs() < 1.0 && (aabb.max[k] - part.max[k]).abs() < 1.0)
}

pub fn load(path: &std::path::Path) -> anyhow::Result<Evidence> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading evidence {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parsing evidence {}: {e}", path.display()))
}
