//! The `<model>.machine.toml` sidecar contract: the single source of truth
//! for a rigged machine's node names, motion, envelope, and budgets.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MachineSpec {
    pub machine: Machine,
    pub envelope: Envelope,
    #[serde(default)]
    pub budgets: Budgets,
    #[serde(default)]
    pub symmetry: Option<Symmetry>,
    #[serde(default)]
    pub tolerances: Tolerances,
    #[serde(rename = "node", default)]
    pub nodes: Vec<NodeSpec>,
    #[serde(rename = "allow_contact", default)]
    pub allow_contact: Vec<AllowContact>,
}

#[derive(Debug, Deserialize)]
pub struct Machine {
    pub name: String,
    #[serde(default = "default_units")]
    pub units: String,
    /// `machine.{x,y,z}` expressed as which gltf axis each comes from, e.g.
    /// `["x","z","y"]` means `machine = (gltf.x, gltf.z, gltf.y)`.
    pub gltf_to_machine: [Axis; 3],
    #[serde(default)]
    pub scene_extent_mm: Option<Range>,
}

fn default_units() -> String {
    "mm".to_string()
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub z: [f64; 2],
}

impl Envelope {
    /// The 8 corner poses (gx, gy, gz) of the travel envelope.
    pub fn corners(&self) -> [[f64; 3]; 8] {
        let mut out = [[0.0; 3]; 8];
        let mut i = 0;
        for gx in self.x {
            for gy in self.y {
                for gz in self.z {
                    out[i] = [gx, gy, gz];
                    i += 1;
                }
            }
        }
        out
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Budgets {
    pub max_triangles: Option<u64>,
    pub max_materials: Option<u64>,
    pub max_textures: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct Symmetry {
    pub plane: Plane,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub node_pairs: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
pub struct Plane {
    pub axis: Axis,
    pub at: f64,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(default)]
pub struct Tolerances {
    pub contact: f64,
    pub coplanar: f64,
    pub mirror: f64,
    pub weld: f64,
    pub degenerate_area: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            contact: 0.01,
            coplanar: 0.01,
            mirror: 0.1,
            weld: 1e-4,
            degenerate_area: 1e-6,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct NodeSpec {
    pub name: String,
    pub role: Role,
    #[serde(default)]
    pub motion: Option<Motion>,
    #[serde(default)]
    pub contract: Option<Contract>,
    #[serde(default)]
    pub surface: Option<Surface>,
    #[serde(default)]
    pub spin: Option<Spin>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Static,
    Moving,
    Spinner,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Motion {
    pub offset: [f64; 3],
    #[serde(default)]
    pub per_gx: [f64; 3],
    #[serde(default)]
    pub per_gy: [f64; 3],
    #[serde(default)]
    pub per_gz: [f64; 3],
}

impl Motion {
    /// Machine-space displacement for a given (gx, gy, gz) travel pose.
    pub fn displacement(&self, pose: [f64; 3]) -> [f64; 3] {
        let mut out = self.offset;
        for (i, o) in out.iter_mut().enumerate() {
            *o += pose[0] * self.per_gx[i] + pose[1] * self.per_gy[i] + pose[2] * self.per_gz[i];
        }
        out
    }
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct Contract {
    #[serde(default)]
    pub vertex_at_origin: bool,
    /// Per-axis floor on local machine-space vertex coordinates; `nan` means
    /// unconstrained on that axis.
    #[serde(default)]
    pub min_local: Option<[f64; 3]>,
}

#[derive(Debug, Deserialize)]
pub struct Surface {
    pub plane: Axis,
    pub at: f64,
    pub covers_xy: [[f64; 2]; 2],
    pub max_protrusion: f64,
}

#[derive(Debug, Deserialize)]
pub struct Spin {
    pub axis: Axis,
    pub max_radial_extent: f64,
    #[serde(default)]
    pub translation_required: bool,
}

#[derive(Debug, Deserialize)]
pub struct AllowContact {
    pub a: PartRef,
    pub b: PartRef,
}

#[derive(Debug, Deserialize)]
pub struct PartRef {
    pub node: String,
    /// `"origin_component"` selects just the component containing the node's
    /// local-origin vertex; `"surface"` or omitted selects the whole node.
    #[serde(default)]
    pub part: Option<String>,
    /// Tier-2: select by evidence label instead of by component.
    #[serde(default)]
    pub label: Option<String>,
}

pub fn load(path: &std::path::Path) -> anyhow::Result<MachineSpec> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading spec {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing spec {}: {e}", path.display()))
}
