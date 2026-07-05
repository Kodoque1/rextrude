"""Generate the low-poly PSX-style i3 bedslinger printer model as a GLB.

Run headless from the repo root:

    blender --background --factory-startup --python tools/gen_printer_assets.py -- \
        --out app/assets/models/printer.glb

Requires app/assets/textures/psx_atlas.png (python3 tools/gen_textures.py).

Coordinate convention: everything below is authored in *machine coordinates*,
identical to the app's gcode space: X across the bed, Y along bed travel,
Z up, 1 unit = 1 mm. Blender is Z-up and its glTF exporter (export_yup=True)
maps Blender (x, y, z) -> glTF/Bevy (x, z, -y), so `to_blender` flips Y:
machine (x, y, z) -> Blender (x, -y, z) -> Bevy (x, z, y) = gcode_to_bevy.

Texturing: one material sampling app/assets/textures/psx_atlas.png with
NEAREST filtering. Every face is planar-projected fit-to-region into a named
atlas rect (see tools/texture_regions.py), oriented so that, seen from
outside the part, the texture reads upright and unmirrored.

Node contract with the app (app/src/printer_rig.rs):
  Frame_Static  - static, stays where the scene puts it (identity node)
  Gantry_X      - reparented under GantryRig; authored at the gcode z=0 pose
  Carriage_X    - reparented under CarriageRig; nozzle tip = object origin
  Bed_Y         - reparented under BedRig; origin = gcode (0,0,0), bed top = z0
  LeadScrew_L/R - stay in place, spun around local Y; origin ON the screw
                  axis, position kept as node translation (never baked).
"""

import json
import math
import os
import struct
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from texture_regions import region_uv_rect

REQUIRED_NODES = {
    "Frame_Static",
    "Gantry_X",
    "Carriage_X",
    "Bed_Y",
    "LeadScrew_L",
    "LeadScrew_R",
}

ATLAS_PATH = os.path.abspath(os.path.join("app", "assets", "textures", "psx_atlas.png"))


def to_blender(p):
    x, y, z = p
    return (x, -y, z)


# ------------------------------------------------------- geometry builder --


class Geo:
    """Accumulates verts/faces/per-face atlas region for one mesh object,
    plus labeled machine-space AABBs of every primitive for the design
    validator (`validate_design`)."""

    def __init__(self):
        self.verts = []
        self.faces = []
        self.face_mat = []
        self.parts = []  # (label, (lo_x,lo_y,lo_z), (hi_x,hi_y,hi_z)) machine coords

    def _record(self, label, center, size):
        self.parts.append((
            label,
            tuple(c - s / 2.0 for c, s in zip(center, size)),
            tuple(c + s / 2.0 for c, s in zip(center, size)),
        ))

    def _push(self, verts, faces, mats):
        base = len(self.verts)
        self.verts += verts
        for face, mat in zip(faces, mats):
            self.faces.append(tuple(base + i for i in face))
            self.face_mat.append(mat)

    def add_box(self, center, size, mat, mat_top=None, mat_front=None, label=""):
        """Axis-aligned box in machine coords. `mat_front` is the machine
        -Y face (the one facing the operator/camera)."""
        self._record(label, center, size)
        cx, cy, cz = to_blender(center)
        hx, hy, hz = size[0] / 2.0, size[1] / 2.0, size[2] / 2.0
        verts = [
            (cx - hx, cy - hy, cz - hz), (cx + hx, cy - hy, cz - hz),
            (cx + hx, cy + hy, cz - hz), (cx - hx, cy + hy, cz - hz),
            (cx - hx, cy - hy, cz + hz), (cx + hx, cy - hy, cz + hz),
            (cx + hx, cy + hy, cz + hz), (cx - hx, cy + hy, cz + hz),
        ]
        faces = [
            (0, 3, 2, 1),  # bottom
            (4, 5, 6, 7),  # top
            (0, 1, 5, 4),  # blender -y = machine rear
            (1, 2, 6, 5),  # +x
            (2, 3, 7, 6),  # blender +y = machine front
            (3, 0, 4, 7),  # -x
        ]
        side = mat
        mats = [
            mat,
            mat_top if mat_top is not None else mat,
            side,
            side,
            mat_front if mat_front is not None else side,
            side,
        ]
        self._push(verts, faces, mats)

    def add_chamfered_box(self, center, size, chamfer, mat, mat_top=None, mat_front=None, label=""):
        """Box with the four vertical edges chamfered (octagonal prism).
        10 faces; used on hero parts for the machined-hardware read."""
        self._record(label, center, size)
        cx, cy, cz = to_blender(center)
        hx, hy, hz = size[0] / 2.0, size[1] / 2.0, size[2] / 2.0
        c = min(chamfer, hx * 0.9, hy * 0.9)
        ring = [
            (-hx + c, -hy), (hx - c, -hy), (hx, -hy + c), (hx, hy - c),
            (hx - c, hy), (-hx + c, hy), (-hx, hy - c), (-hx, -hy + c),
        ]
        verts = [(cx + x, cy + y, cz - hz) for (x, y) in ring]
        verts += [(cx + x, cy + y, cz + hz) for (x, y) in ring]
        faces = [tuple(reversed(range(8))), tuple(range(8, 16))]
        mats = [mat, mat_top if mat_top is not None else mat]
        for i in range(8):
            j = (i + 1) % 8
            faces.append((i, j, 8 + j, 8 + i))
            # ring segment 4-5 spans blender +y = machine front
            mats.append(mat_front if (mat_front is not None and i == 4) else mat)
        self._push(verts, faces, mats)

    def add_cylinder(self, center, radius, height, mat, segs=6, label=""):
        """Vertical low-poly cylinder in machine coords."""
        self._record(label, center, (2 * radius, 2 * radius, height))
        cx, cy, cz = to_blender(center)
        bottom, top = cz - height / 2.0, cz + height / 2.0
        verts = []
        for z in (bottom, top):
            for i in range(segs):
                a = i / segs * math.tau
                verts.append((cx + radius * math.cos(a), cy + radius * math.sin(a), z))
        faces = []
        for i in range(segs):
            j = (i + 1) % segs
            faces.append((i, j, segs + j, segs + i))
        faces.append(tuple(reversed(range(segs))))          # bottom cap
        faces.append(tuple(range(segs, 2 * segs)))          # top cap
        self._push(verts, faces, [mat] * len(faces))

    def add_cone(self, tip, radius, height, mat, segs=6, label=""):
        """Cone with the tip at `tip` (machine coords), opening upward."""
        self._record(label, (tip[0], tip[1], tip[2] + height / 2.0), (2 * radius, 2 * radius, height))
        cx, cy, cz = to_blender(tip)
        verts = [(cx, cy, cz)]
        for i in range(segs):
            a = i / segs * math.tau
            verts.append((cx + radius * math.cos(a), cy + radius * math.sin(a), cz + height))
        faces = [(0, (i % segs) + 1, ((i + 1) % segs) + 1) for i in range(segs)]
        faces.append(tuple(range(1, segs + 1)))             # top cap
        self._push(verts, faces, [mat] * len(faces))


def face_uvs(poly, mesh):
    """Fit-to-region planar projection for one polygon.

    Picks the projection plane from the dominant normal axis and orients it
    so the texture reads upright/unmirrored when viewed from outside the
    part: u increases to the viewer's right, v downward (glTF image space).
    Returns (u, v) in glTF space per loop, in [0,1]x[0,1] before region fit.
    """
    n = poly.normal
    coords = [mesh.vertices[vi].co for vi in poly.vertices]
    ax, ay, az = abs(n.x), abs(n.y), abs(n.z)
    if az >= ax and az >= ay:
        # horizontal face; keep machine-front (blender +y) toward image bottom
        proj = [(v.x, v.y) if n.z > 0 else (v.x, -v.y) for v in coords]
    elif ay >= ax:
        # blender +y face = machine front: viewer at +y looks along -y,
        # so world +x appears to the viewer's LEFT -> u = -x.
        proj = [(-v.x, -v.z) if n.y > 0 else (v.x, -v.z) for v in coords]
    else:
        proj = [(v.y, -v.z) if n.x > 0 else (-v.y, -v.z) for v in coords]

    us = [p[0] for p in proj]
    vs = [p[1] for p in proj]
    du = max(max(us) - min(us), 1e-6)
    dv = max(max(vs) - min(vs), 1e-6)
    return [((p[0] - min(us)) / du, (p[1] - min(vs)) / dv) for p in proj]


def build_object(name, geo, material, location=(0.0, 0.0, 0.0)):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(geo.verts, [], geo.faces)
    mesh.validate()
    assert len(mesh.polygons) == len(geo.face_mat), f"{name}: face count changed in validate()"
    uv_layer = mesh.uv_layers.new()
    for poly, region in zip(mesh.polygons, geo.face_mat):
        u0, v0, u1, v1 = region_uv_rect(region)
        for li, (fu, fv) in zip(poly.loop_indices, face_uvs(poly, mesh)):
            u = u0 + fu * (u1 - u0)
            v = v0 + fv * (v1 - v0)
            # Blender UV space is bottom-left origin; glTF is top-left.
            # The exporter flips V, so author flipped here.
            uv_layer.data[li].uv = (u, 1.0 - v)
    mesh.materials.append(material)
    obj = bpy.data.objects.new(name, mesh)
    obj.location = location
    bpy.context.scene.collection.objects.link(obj)
    return obj


# ----------------------------------------------------------------- assets --


def make_material():
    if not os.path.exists(ATLAS_PATH):
        print(f"ERROR: {ATLAS_PATH} missing - run: python3 tools/gen_textures.py")
        sys.exit(1)
    img = bpy.data.images.load(ATLAS_PATH)

    mat = bpy.data.materials.new("PSX_Atlas")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Roughness"].default_value = 0.9
    bsdf.inputs["Metallic"].default_value = 0.0
    tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"
    mat.node_tree.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    return mat


def build_frame(mat):
    g = Geo()
    # Y rails + end crossmembers + feet (the chassis the bed rides on).
    # Crossmembers sit 0.5mm below the rail plane and feet poke 0.2mm into
    # the rails: attachments overlap in volume instead of sharing render
    # planes (see validate_design's z-fight rule).
    for x in (60, 160):
        g.add_box((x, 110, -14), (12, 470, 6), "steel", label="rail")
    for y in (-90, 315):
        g.add_box((110, y, -14.5), (140, 18, 6), "steel", label="crossmember")
    for x in (60, 160):
        for y in (-100, 320):
            g.add_box((x, y, -20.8), (16, 20, 8), "rubber", label="foot")
    # Y belt + rear Y stepper.
    g.add_box((110, 112, -13), (6, 440, 2), "rubber", label="y_belt")
    g.add_box((110, 358, -4), (30, 30, 30), "olive", label="y_stepper")
    # Uprights + top crossbar, in the gantry plane (machine y = 138).
    for x in (-45, 265):
        g.add_chamfered_box((x, 138, 117), (14, 14, 260), 3, "gunmetal", label="upright")
    g.add_chamfered_box((110, 138, 254), (338, 14, 14), 3, "gunmetal", label="crossbar")
    # Z stepper blocks at the screw bases.
    for x in (-22, 242):
        g.add_box((x, 138, 2.2), (34, 34, 30), "olive", label="z_stepper")
    # PSU box off to the side; unit label faces the operator.
    g.add_chamfered_box((262, 250, 15), (60, 100, 58), 4, "olive", mat_front="psu_decal", label="psu")
    build_object("Frame_Static", g, mat)
    return g


def build_gantry(mat):
    g = Geo()
    g.add_chamfered_box((110, 138, 42), (330, 16, 16), 3, "gunmetal", label="beam")
    # hazard trim strip along the beam's lower front edge (0.5mm proud)
    g.add_box((110, 129.2, 35.5), (300, 1.5, 5), "hazard", label="trim")
    for x in (-45, 265):                                  # Z carriage brackets
        g.add_box((x, 138, 42), (30, 30, 44), "gunmetal", label="bracket")
    for x in (-22, 242):                                  # brass lead nuts
        g.add_box((x, 138, 42), (14, 14, 18), "brass", label="nut")
    g.add_box((-45, 138, 74), (28, 28, 24), "olive", label="x_stepper")
    g.add_box((110, 129.5, 46), (300, 2, 5), "rubber", label="x_belt")
    build_object("Gantry_X", g, mat)
    return g


def build_carriage(mat):
    g = Geo()
    g.add_chamfered_box((0, 14, 38), (44, 8, 40), 2, "olive", label="plate")
    for i in range(5):                                    # heatsink fins
        g.add_box((0, 0, 17 + i * 4), (22, 22, 2.6), "alu", label="fin")
    # heater block, warning label toward the operator
    g.add_box((0, 0, 10), (16, 12, 9), "dark_steel", mat_front="caution_decal", label="heater")
    g.add_cone((0, 0, 0), 2.5, 5, "brass", label="nozzle")  # nozzle, tip = origin
    g.add_box((0, -13, 24), (24, 8, 24), "olive", label="shroud")
    g.add_box((0, 14, 62), (18, 10, 8), "cable", label="cable_box")
    build_object("Carriage_X", g, mat)
    return g


def build_bed(mat):
    g = Geo()
    g.add_chamfered_box((110, 110, -2.5), (220, 220, 5), 4, "dark_steel", mat_top="bed_top", label="slab")
    g.add_box((110, 110, -8), (160, 240, 6), "dark_steel", label="bed_carriage")
    # hazard trim along the bed slab's front edge
    g.add_box((110, -1.0, -2.5), (220, 1.5, 4.5), "hazard", label="trim")
    # CAUTION placard hanging off the carriage front
    g.add_box((110, -13, -8), (44, 2, 8), "dark_steel", mat_front="caution_decal", label="placard")
    for x in (25, 195):                                        # leveling knobs
        for y in (25, 195):
            g.add_cylinder((x, y, -11), 7, 5, "rubber", segs=6, label="knob")
    g.add_box((5, 5, 1), (10, 10, 2), "orange", label="notch")
    g.add_box((110, 224, -6), (30, 8, 4), "cable", label="cable_strip")
    build_object("Bed_Y", g, mat)
    return g


SCREW_POSITIONS = {"LeadScrew_L": (-22, 138, 132), "LeadScrew_R": (242, 138, 132)}


def build_lead_screw(name, mat):
    # Bare 6-sided cylinder: the brushed `steel` region rotating with the
    # mesh is what sells the spin (the old fin boxes were silhouette noise).
    g = Geo()
    g.add_cylinder((0, 0, 0), 3.5, 230, "steel", segs=6, label="screw")
    build_object(name, g, mat, location=to_blender(SCREW_POSITIONS[name]))
    return g



# ------------------------------------------------------ design validation --

# gcode travel envelope the machine must survive (mm).
TRAVEL = ((0.0, 220.0), (0.0, 220.0), (0.0, 60.0))
# The nozzle lane: carriage local coords sit at machine y = 110.
NOZZLE_LANE_Y = 110.0
MAX_TRIANGLES = 5000
# Parts expected to come in mirror pairs about the machine center plane.
SYMMETRIC_LABELS = {"upright", "z_stepper", "bracket", "nut", "rail", "foot", "knob"}
SYMMETRY_PLANE_X = 110.0


def _shift(part, d):
    label, lo, hi = part
    return (label, tuple(a + b for a, b in zip(lo, d)), tuple(a + b for a, b in zip(hi, d)))


def _overlap(a, b, tol=0.01):
    _, alo, ahi = a
    _, blo, bhi = b
    return all(alo[i] + tol < bhi[i] and blo[i] + tol < ahi[i] for i in range(3))


def _collisions(errors, when, group_a, group_b):
    for pa in group_a:
        for pb in group_b:
            if _overlap(pa, pb):
                errors.append(f"collision {when}: {pa[0]} {pa[1]}..{pa[2]} vs {pb[0]} {pb[1]}..{pb[2]}")


def validate_design(geos):
    """Design-by-contract checks over the authored parts; exits on failure.

    Guards the invariants the app's kinematics rely on (nozzle-at-origin,
    bed-top-at-zero, screws spinning about their own axis), sweeps the
    travel envelope for collisions, and rejects same-facing coplanar
    render planes (z-fighting) and budget overruns.
    """
    errors = []
    frame = geos["Frame_Static"].parts
    gantry = geos["Gantry_X"].parts
    carriage = geos["Carriage_X"].parts
    bed = geos["Bed_Y"].parts
    screws = [
        _shift(part, SCREW_POSITIONS[name])
        for name in ("LeadScrew_L", "LeadScrew_R")
        for part in geos[name].parts
    ]

    # --- kinematic contract ---------------------------------------------
    cgeo = geos["Carriage_X"]
    if not any(all(abs(c) < 1e-6 for c in v) for v in cgeo.verts):
        errors.append("carriage: nozzle tip vertex is not at the local origin")
    if min(v[2] for v in cgeo.verts) < -1e-6:
        errors.append("carriage: geometry extends below the nozzle tip")

    slab = next(p for p in bed if p[0] == "slab")
    if any(abs(a - b) > 1e-3 for a, b in zip(slab[1] + slab[2], (0, 0, -5, 220, 220, 0))):
        errors.append(f"bed: slab must span (0,0,-5)..(220,220,0), got {slab[1]}..{slab[2]}")
    bed_top_overhang = max(hi[2] for _, _, hi in bed)
    if bed_top_overhang > 2.01:
        errors.append(f"bed: parts rise {bed_top_overhang}mm above the print surface")

    for name in ("LeadScrew_L", "LeadScrew_R"):
        sgeo = geos[name]
        r = max(max(abs(v[0]), abs(v[1])) for v in sgeo.verts)
        if r > 4.0:
            errors.append(f"{name}: geometry {r:.1f}mm off its spin axis (must be centered)")
        if all(abs(c) < 1e-6 for c in SCREW_POSITIONS[name][:2]):
            errors.append(f"{name}: node translation looks baked into geometry")

    lx, rx = SCREW_POSITIONS["LeadScrew_L"][0], SCREW_POSITIONS["LeadScrew_R"][0]
    if abs((lx + rx) / 2.0 - SYMMETRY_PLANE_X) > 0.1:
        errors.append("lead screws are not mirrored about the machine center plane")

    gantry_low = min(lo[2] for _, lo, _ in gantry)
    if gantry_low < 5.0:
        errors.append(f"gantry: reaches z={gantry_low}mm at the gcode z=0 pose (bed clearance)")

    # --- travel-envelope collision sweep ---------------------------------
    static = frame + screws
    for gx in TRAVEL[0]:
        for gy in TRAVEL[1]:
            for gz in TRAVEL[2]:
                when = f"at gcode ({gx:.0f},{gy:.0f},{gz:.0f})"
                carr_w = [_shift(p, (gx, NOZZLE_LANE_Y, gz)) for p in carriage]
                bed_w = [_shift(p, (0.0, NOZZLE_LANE_Y - gy, 0.0)) for p in bed]
                gantry_w = [_shift(p, (0.0, 0.0, gz)) for p in gantry]
                _collisions(errors, when, carr_w, static)
                _collisions(errors, when, bed_w, static)
                _collisions(errors, when, bed_w, gantry_w)
                _collisions(errors, when, carr_w, gantry_w)
                # the nozzle is allowed to touch the print surface
                _collisions(errors, when, [p for p in carr_w if p[0] != "nozzle"], bed_w)

    # --- z-fight guard: same-facing coplanar planes with overlap ---------
    for obj_name, geo in geos.items():
        parts = geo.parts
        for i in range(len(parts)):
            for j in range(i + 1, len(parts)):
                (la, alo, ahi), (lb, blo, bhi) = parts[i], parts[j]
                for axis in range(3):
                    others = [k for k in range(3) if k != axis]
                    flat_overlap = all(
                        alo[k] + 0.01 < bhi[k] and blo[k] + 0.01 < ahi[k] for k in others
                    )
                    if not flat_overlap:
                        continue
                    if abs(ahi[axis] - bhi[axis]) < 0.01 or abs(alo[axis] - blo[axis]) < 0.01:
                        errors.append(
                            f"z-fight risk in {obj_name}: {la} and {lb} share a same-facing plane on axis {axis}"
                        )

    # --- symmetry about the machine center plane --------------------------
    for obj_name, geo in geos.items():
        sym = [p for p in geo.parts if p[0] in SYMMETRIC_LABELS]
        for label, lo, hi in sym:
            cx = (lo[0] + hi[0]) / 2.0
            if abs(cx - SYMMETRY_PLANE_X) < 0.1:
                continue
            mirror_cx = 2.0 * SYMMETRY_PLANE_X - cx
            if not any(
                p[0] == label and abs((p[1][0] + p[2][0]) / 2.0 - mirror_cx) < 0.1
                for p in sym
            ):
                errors.append(f"{obj_name}: {label} at x={cx:.1f} has no mirror twin about x={SYMMETRY_PLANE_X}")

    # --- budgets ----------------------------------------------------------
    tris = sum(len(f) - 2 for g in geos.values() for f in g.faces)
    if tris > MAX_TRIANGLES:
        errors.append(f"triangle budget exceeded: {tris} > {MAX_TRIANGLES}")

    if errors:
        print(f"DESIGN VALIDATION FAILED ({len(errors)} violations):")
        for e in errors:
            print(f"  - {e}")
        sys.exit(1)
    print(f"design validation OK ({tris} tris, {sum(len(g.parts) for g in geos.values())} parts)")


# ------------------------------------------------------------------ main --


def parse_out_path():
    argv = sys.argv
    if "--" in argv:
        argv = argv[argv.index("--") + 1:]
        if "--out" in argv:
            return argv[argv.index("--out") + 1]
    return "app/assets/models/printer.glb"


def validate_glb(path):
    with open(path, "rb") as f:
        data = f.read()
    assert data[:4] == b"glTF", "not a GLB file"
    (json_len,) = struct.unpack("<I", data[12:16])
    assert data[16:20] == b"JSON"
    doc = json.loads(data[20:20 + json_len])
    names = {n.get("name") for n in doc.get("nodes", [])}
    missing = REQUIRED_NODES - names
    if missing:
        print(f"ERROR: exported GLB is missing nodes: {sorted(missing)}")
        sys.exit(1)
    tris = sum(
        acc.get("count", 0) // 3
        for m in doc.get("meshes", [])
        for p in m.get("primitives", [])
        for acc in [doc["accessors"][p["indices"]]]
        if "indices" in p
    )
    print(f"GLB OK: nodes={sorted(names)} triangles={tris}")


def main():
    out = parse_out_path()
    os.makedirs(os.path.dirname(out), exist_ok=True)

    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)

    mat = make_material()
    geos = {
        "Frame_Static": build_frame(mat),
        "Gantry_X": build_gantry(mat),
        "Carriage_X": build_carriage(mat),
        "Bed_Y": build_bed(mat),
        "LeadScrew_L": build_lead_screw("LeadScrew_L", mat),
        "LeadScrew_R": build_lead_screw("LeadScrew_R", mat),
    }
    validate_design(geos)

    bpy.ops.export_scene.gltf(filepath=out, export_format="GLB", export_yup=True)
    validate_glb(out)


main()
