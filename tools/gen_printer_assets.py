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
    """Accumulates verts/faces/per-face atlas region for one mesh object."""

    def __init__(self):
        self.verts = []
        self.faces = []
        self.face_mat = []

    def _push(self, verts, faces, mats):
        base = len(self.verts)
        self.verts += verts
        for face, mat in zip(faces, mats):
            self.faces.append(tuple(base + i for i in face))
            self.face_mat.append(mat)

    def add_box(self, center, size, mat, mat_top=None, mat_front=None):
        """Axis-aligned box in machine coords. `mat_front` is the machine
        -Y face (the one facing the operator/camera)."""
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

    def add_chamfered_box(self, center, size, chamfer, mat, mat_top=None, mat_front=None):
        """Box with the four vertical edges chamfered (octagonal prism).
        10 faces; used on hero parts for the machined-hardware read."""
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

    def add_cylinder(self, center, radius, height, mat, segs=6):
        """Vertical low-poly cylinder in machine coords."""
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

    def add_cone(self, tip, radius, height, mat, segs=6):
        """Cone with the tip at `tip` (machine coords), opening upward."""
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
    for x in (60, 160):
        g.add_box((x, 110, -10), (12, 470, 6), "steel")
    for y in (-90, 315):
        g.add_box((110, y, -10), (140, 18, 6), "steel")
    for x in (60, 160):
        for y in (-100, 320):
            g.add_box((x, y, -17), (16, 20, 8), "rubber")
    # Y belt + rear Y stepper.
    g.add_box((110, 112, -9), (6, 440, 2), "rubber")
    g.add_box((110, 338, -4), (42, 42, 34), "olive")
    # Uprights + top crossbar, in the gantry plane (machine y = 138).
    for x in (-45, 265):
        g.add_chamfered_box((x, 138, 117), (14, 14, 260), 3, "gunmetal")
    g.add_chamfered_box((110, 138, 254), (338, 14, 14), 3, "gunmetal")
    # Z stepper blocks at the screw bases.
    for x in (-22, 242):
        g.add_box((x, 138, 2), (34, 34, 30), "olive")
    # PSU box off to the side; unit label faces the operator.
    g.add_chamfered_box((262, 250, 15), (60, 100, 58), 4, "olive", mat_front="psu_decal")
    return build_object("Frame_Static", g, mat)


def build_gantry(mat):
    g = Geo()
    g.add_chamfered_box((110, 138, 42), (330, 16, 16), 3, "gunmetal")  # X beam
    # hazard trim strip along the beam's lower front edge (0.5mm proud)
    g.add_box((110, 129.2, 35.5), (300, 1.5, 5), "hazard")
    for x in (-45, 265):                                  # Z carriage brackets
        g.add_box((x, 138, 42), (30, 30, 44), "gunmetal")
    for x in (-22, 242):                                  # brass lead nuts
        g.add_box((x, 138, 42), (14, 14, 18), "brass")
    g.add_box((-45, 138, 74), (28, 28, 24), "olive")      # X stepper
    g.add_box((110, 129.5, 46), (300, 2, 5), "rubber")    # X belt
    return build_object("Gantry_X", g, mat)


def build_carriage(mat):
    g = Geo()
    g.add_chamfered_box((0, 16, 38), (44, 8, 40), 2, "olive")  # carriage plate
    for i in range(5):                                    # heatsink fins
        g.add_box((0, 0, 17 + i * 4), (22, 22, 2.6), "alu")
    # heater block, warning label toward the operator
    g.add_box((0, 0, 10), (16, 12, 9), "dark_steel", mat_front="caution_decal")
    g.add_cone((0, 0, 0), 2.5, 5, "brass")                # nozzle, tip = origin
    g.add_box((0, -13, 24), (24, 8, 24), "olive")         # fan shroud
    g.add_box((0, 16, 62), (18, 10, 8), "cable")          # cable box
    return build_object("Carriage_X", g, mat)


def build_bed(mat):
    g = Geo()
    g.add_chamfered_box((110, 110, -2.5), (220, 220, 5), 4, "dark_steel", mat_top="bed_top")
    g.add_box((110, 110, -8), (160, 240, 6), "dark_steel")     # Y carriage
    # hazard trim along the bed slab's front edge
    g.add_box((110, -1.0, -2.5), (220, 1.5, 4.5), "hazard")
    # CAUTION placard hanging off the carriage front
    g.add_box((110, -13, -13), (44, 2, 14), "dark_steel", mat_front="caution_decal")
    for x in (25, 195):                                        # leveling knobs
        for y in (25, 195):
            g.add_cylinder((x, y, -11), 7, 5, "rubber", segs=6)
    g.add_box((5, 5, 1), (10, 10, 2), "orange")   # orange notch = gcode origin
    g.add_box((110, 224, -6), (30, 8, 4), "cable")             # cable strip
    return build_object("Bed_Y", g, mat)


def build_lead_screw(name, x, mat):
    # Bare 6-sided cylinder: the brushed `steel` region rotating with the
    # mesh is what sells the spin (the old fin boxes were silhouette noise).
    g = Geo()
    g.add_cylinder((0, 0, 0), 3.5, 230, "steel", segs=6)
    return build_object(name, g, mat, location=to_blender((x, 138, 132)))


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
    build_frame(mat)
    build_gantry(mat)
    build_carriage(mat)
    build_bed(mat)
    build_lead_screw("LeadScrew_L", -22, mat)
    build_lead_screw("LeadScrew_R", 242, mat)

    bpy.ops.export_scene.gltf(filepath=out, export_format="GLB", export_yup=True)
    validate_glb(out)


main()
