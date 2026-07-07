# rigcheck

A standalone CLI that validates a rigged machine GLB model against a
hand-authored sidecar spec. It replaces the inline `validate_design`/
`validate_glb` functions that used to live in `tools/gen_printer_assets.py`
with a single tool that works on any rigged machine model, not just this
printer — a spec is all a new model needs.

```
rigcheck check <MODEL.glb> [--spec PATH] [--evidence PATH]
                            [--format human|json|github]
                            [--only ID,...] [--skip ID,...]
```

`--spec` defaults to `<model-stem>.machine.toml`, `--evidence` to
`<model-stem>.evidence.json` (only used if present). Exit code is `0` on a
clean pass, `1` if any check fails, `2` on a usage/parse error.

## Two tiers

A flattened GLB mesh has no sub-part labels — one rig node is one merged
mesh. Most checks work anyway (`tier 1`, black-box: `f(glb, spec)`), because
sub-parts of a single node never share vertices in a typical export (every
face gets its own planar UV projection, which splits every seam) — welding
vertices back together by position and taking connected components recovers
per-part AABBs without needing any labels. This works on *any* GLB, hand
-authored or generated.

`Tier 2` checks (labeled symmetry pairs, exact sub-part contracts) need an
optional **evidence manifest** — a JSON sidecar a generator can emit with
labeled machine-space AABBs per node (see `tools/gen_printer_assets.py`'s
`write_evidence` for a worked example). Without it, tier-2 checks report
`skipped`, never silently pass. A drift guard cross-checks every evidence
AABB against a real mesh component, so a stale manifest fails loudly instead
of quietly validating nothing.

## Writing a `<model>.machine.toml` spec

All spec values are in **machine space** — whatever coordinate convention
your kinematics use (for a 3D printer: gcode space, mm, Z up). The model
itself is glTF space (Y-up). The `gltf_to_machine` field bridges the two:

```toml
[machine]
name = "my-machine"
units = "mm"
# machine.{x,y,z} as which gltf axis each comes from.
# e.g. ["x","z","y"] means machine = (gltf.x, gltf.z, gltf.y) — the common
# case when a Z-up authoring tool's Y-up glTF export just swaps y/z.
gltf_to_machine = ["x", "z", "y"]
scene_extent_mm = { min = 50.0, max = 2000.0 }  # unit-mismatch sanity check
```

> **Note on handedness:** an axis swap like the above is a *reflection*
> (determinant −1), not a rotation. Vector *positions* relabel correctly
> under it with no special handling. Anything computed from a **cross
> product** (a face normal, a signed volume for winding) does **not** —
> compute those in native glTF space first, then relabel the result. This
> bit rigcheck's own `quality.surface` check once; see `checks/quality.rs`.

The gcode/travel envelope the machine must survive, swept at its 8 corners:

```toml
[envelope]
x = [0.0, 220.0]
y = [0.0, 220.0]
z = [0.0, 60.0]
```

Budgets, symmetry, and tolerances (all optional, sensible defaults for
tolerances):

```toml
[budgets]
max_triangles = 5000
max_materials = 1
max_textures = 1

[symmetry]
plane = { axis = "x", at = 110.0 }
labels = ["upright", "bracket"]        # tier-2: needs evidence
node_pairs = [["ScrewL", "ScrewR"]]    # tier-1: works black-box

[tolerances]
contact = 0.01        # mm, sweep interpenetration
coplanar = 0.01        # mm, z-fight plane-distance
mirror = 0.1           # mm, symmetry mirror tolerance
weld = 1e-4            # mm, vertex-welding quantization grid
degenerate_area = 1e-6 # mm^2, minimum triangle area
```

Each node in the rig gets a `[[node]]` entry. `role` drives sweep grouping:
`static` never moves, `moving` gets displaced by its `motion` map at each
envelope corner, `spinner` stays in place but rotates (checked for radial
extent about its spin axis instead of participating in the position sweep).

```toml
[[node]]
name = "Carriage_X"
role = "moving"
# machine-space displacement = offset + gx*per_gx + gy*per_gy + gz*per_gz,
# evaluated at each envelope corner (gx, gy, gz).
motion = { offset = [0.0, 110.0, 0.0], per_gx = [1.0, 0.0, 0.0], per_gz = [0.0, 0.0, 1.0] }
contract = { vertex_at_origin = true, min_local = [nan, nan, 0.0] }  # nan = unconstrained axis

[[node]]
name = "Bed_Y"
role = "moving"
motion = { offset = [0.0, 110.0, 0.0], per_gy = [0.0, -1.0, 0.0] }
surface = { plane = "z", at = 0.0, covers_xy = [[0.0, 220.0], [0.0, 220.0]], max_protrusion = 2.01 }

[[node]]
name = "ScrewL"
role = "spinner"
spin = { axis = "z", max_radial_extent = 4.0, translation_required = true }
```

Legitimate always-touching joints (a bracket that wraps a rail, a nut
threaded on a screw) are declared, not special-cased in code:

```toml
[[allow_contact]]
a = { node = "Carriage_X", part = "origin_component" }  # nozzle tip
b = { node = "Bed_Y" }                                  # whole node

[[allow_contact]]
a = { node = "Gantry_X", label = "nut" }   # tier-2: needs evidence
b = { node = "ScrewL", label = "screw" }
```

`part = "origin_component"` selects just the mesh component containing the
node's local-origin vertex (works black-box). `label = "..."` selects
component(s) whose geometry matches an evidence-labeled part (tier-2).
Omitting both selects the whole node. When a `label` selector can't be
resolved (no evidence provided), it falls back to the whole node rather than
matching nothing — omitting evidence degrades an allowance to more
permissive, never makes the sweep stricter.

See `app/assets/models/printer.machine.toml` for the complete worked
example this schema was designed around.

## Checks

| id | tier | what it catches |
|---|---|---|
| `structure.required_nodes` | 1 | a named node is missing or has no mesh |
| `structure.spinner` | 1 | spinner geometry off its spin axis, or translation baked into geometry |
| `structure.contract` | 1 | per-node contracts (`vertex_at_origin`, `min_local`) |
| `structure.budgets` | 1 | triangle/material/texture budgets |
| `hygiene.degenerate_triangles` | 1 | near-zero-area triangles |
| `hygiene.manifold` | 1 | open (boundary) edges — a real hole in the mesh |
| `hygiene.winding` | 1 | inside-out components (negative signed volume) |
| `hygiene.uv_bounds` | 1 | UVs outside \[0,1\] |
| `hygiene.unit_sanity` | 1 | scene extent outside the expected range (unit mismatch) |
| `sweep.collision` | 1 | interpenetration between moving/static components across the travel envelope |
| `quality.z_fight` | 1 | same-facing coplanar overlapping components |
| `quality.symmetry_nodes` | 1 | node-pair translations not mirrored about the symmetry plane |
| `quality.symmetry_labels` | 2 | labeled sub-parts missing a mirror twin |
| `quality.surface` | 1 | declared surface doesn't cover its expected extent, or protrudes too far |
| `quality.evidence_drift` | 2 | an evidence AABB with no matching mesh component (stale manifest) |

Edge counts greater than 2 (T-junctions) are **not** flagged as non-manifold:
separate abutting parts sharing a welded seam (a bracket wrapping a rail) is
a normal hard-surface assembly pattern, not a defect.

## Regenerating the printer model

```
python3 tools/gen_textures.py
blender --background --factory-startup --python tools/gen_printer_assets.py -- \
    --out app/assets/models/printer.glb
cargo run -p rigcheck --release -- check app/assets/models/printer.glb
```

## Tests

`cargo test -p rigcheck` runs:
- `tests/checks.rs` — one fixture per check family, built as in-memory
  `Model`/`MachineSpec` values (no GLB round-trip needed).
- `tests/printer_glb.rs` — the full suite against the committed
  `printer.glb` + sidecars, and pinned per-node component counts (so an
  exporter behavior change that merges sub-parts is caught).
