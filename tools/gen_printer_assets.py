"""Generate the low-poly PSX-style i3 bedslinger printer model as a GLB.

Run headless from the repo root:

    blender --background --factory-startup --python tools/gen_printer_assets.py -- \
        --out app/assets/models/printer.glb

Coordinate convention: everything below is authored in *machine coordinates*,
identical to the app's gcode space: X across the bed, Y along bed travel,
Z up, 1 unit = 1 mm. Blender is Z-up and its glTF exporter (export_yup=True)
maps Blender (x, y, z) -> glTF/Bevy (x, z, -y), so `to_blender` flips Y:
machine (x, y, z) -> Blender (x, -y, z) -> Bevy (x, z, y) = gcode_to_bevy.

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
import struct
import sys

import bpy

# ---------------------------------------------------------------- palette --

PALETTE = [
    (0.16, 0.17, 0.20),  # 0 gunmetal: frame, beam, brackets
    (0.42, 0.44, 0.48),  # 1 steel: rails, crossbars, screws
    (0.24, 0.27, 0.19),  # 2 dark olive: steppers, PSU, carriage plate
    (0.06, 0.10, 0.08),  # 3 near-black green: bed top surface
    (0.85, 0.42, 0.10),  # 4 safety orange: origin notch marker
    (0.09, 0.09, 0.10),  # 5 rubber: belts, feet, knobs, cables
    (0.20, 0.22, 0.24),  # 6 dark steel: bed slab sides, bed carriage
    (0.62, 0.50, 0.22),  # 7 brass: nozzle, lead nuts
    (0.50, 0.14, 0.12),  # 8 red accent: fan shroud
    (0.55, 0.57, 0.60),  # 9 aluminium: heatsink fins
    (0.12, 0.12, 0.14),  # 10 dark cable
    (0.30, 0.33, 0.24),  # 11 olive light
    (0.10, 0.16, 0.12),  # 12 green dark
    (0.36, 0.38, 0.42),  # 13 steel mid
    (0.05, 0.05, 0.06),  # 14 near black
    (0.70, 0.70, 0.72),  # 15 bright metal
]

REQUIRED_NODES = {
    "Frame_Static",
    "Gantry_X",
    "Carriage_X",
    "Bed_Y",
    "LeadScrew_L",
    "LeadScrew_R",
}


def to_blender(p):
    x, y, z = p
    return (x, -y, z)


def uv_for(pal):
    return ((pal + 0.5) / len(PALETTE), 0.5)


# ------------------------------------------------------- geometry builder --


class Geo:
    """Accumulates verts/faces/per-face palette for one mesh object."""

    def __init__(self):
        self.verts = []
        self.faces = []
        self.face_pal = []

    def _push(self, verts, faces, pal_list):
        base = len(self.verts)
        self.verts += verts
        for face, pal in zip(faces, pal_list):
            self.faces.append(tuple(base + i for i in face))
            self.face_pal.append(pal)

    def add_box(self, center, size, pal, pal_top=None, rot_z=0.0):
        """Axis-aligned box in machine coords (optionally spun around Z)."""
        cx, cy, cz = to_blender(center)
        hx, hy, hz = size[0] / 2.0, size[1] / 2.0, size[2] / 2.0
        corners = [
            (-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
            (-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz),
        ]
        c, s = math.cos(rot_z), math.sin(rot_z)
        verts = [
            (cx + x * c - y * s, cy + x * s + y * c, cz + z)
            for (x, y, z) in corners
        ]
        faces = [
            (0, 3, 2, 1),  # bottom
            (4, 5, 6, 7),  # top
            (0, 1, 5, 4),
            (1, 2, 6, 5),
            (2, 3, 7, 6),
            (3, 0, 4, 7),
        ]
        pals = [pal, pal_top if pal_top is not None else pal] + [pal] * 4
        self._push(verts, faces, pals)

    def add_cylinder(self, center, radius, height, pal, segs=6):
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
        self._push(verts, faces, [pal] * len(faces))

    def add_cone(self, tip, radius, height, pal, segs=6):
        """Cone with the tip at `tip` (machine coords), opening upward."""
        cx, cy, cz = to_blender(tip)
        verts = [(cx, cy, cz)]
        for i in range(segs):
            a = i / segs * math.tau
            verts.append((cx + radius * math.cos(a), cy + radius * math.sin(a), cz + height))
        faces = [(0, (i % segs) + 1, ((i + 1) % segs) + 1) for i in range(segs)]
        faces.append(tuple(range(1, segs + 1)))             # top cap
        self._push(verts, faces, [pal] * len(faces))


def build_object(name, geo, material, location=(0.0, 0.0, 0.0)):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(geo.verts, [], geo.faces)
    mesh.validate()
    uv_layer = mesh.uv_layers.new()
    for poly, pal in zip(mesh.polygons, geo.face_pal):
        for li in poly.loop_indices:
            uv_layer.data[li].uv = uv_for(pal)
    mesh.materials.append(material)
    obj = bpy.data.objects.new(name, mesh)
    obj.location = location
    bpy.context.scene.collection.objects.link(obj)
    return obj


# ----------------------------------------------------------------- assets --


def make_material(palette_path):
    img = bpy.data.images.new("psx_palette", width=len(PALETTE), height=1, alpha=False)
    pixels = []
    for r, g, b in PALETTE:
        pixels += [r, g, b, 1.0]
    img.pixels = pixels
    img.filepath_raw = palette_path
    img.file_format = "PNG"
    img.save()

    mat = bpy.data.materials.new("PSX_Palette")
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
        g.add_box((x, 110, -10), (12, 470, 6), 1)
    for y in (-90, 315):
        g.add_box((110, y, -10), (140, 18, 6), 1)
    for x in (60, 160):
        for y in (-100, 320):
            g.add_box((x, y, -17), (16, 20, 8), 5)
    # Y belt + rear Y stepper.
    g.add_box((110, 112, -9), (6, 440, 2), 5)
    g.add_box((110, 338, -4), (42, 42, 34), 2)
    # Uprights + top crossbar, in the gantry plane (machine y = 138).
    for x in (-45, 265):
        g.add_box((x, 138, 117), (14, 14, 260), 0)
    g.add_box((110, 138, 254), (338, 14, 14), 0)
    # Z stepper blocks at the screw bases.
    for x in (-22, 242):
        g.add_box((x, 138, 2), (34, 34, 30), 2)
    # PSU box off to the side.
    g.add_box((262, 250, 15), (60, 100, 58), 2)
    return build_object("Frame_Static", g, mat)


def build_gantry(mat):
    g = Geo()
    g.add_box((110, 138, 42), (330, 16, 16), 0)          # X beam
    for x in (-45, 265):                                  # Z carriage brackets
        g.add_box((x, 138, 42), (30, 30, 44), 0)
    for x in (-22, 242):                                  # brass lead nuts
        g.add_box((x, 138, 42), (14, 14, 18), 7)
    g.add_box((-45, 138, 74), (28, 28, 24), 2)            # X stepper
    g.add_box((110, 129, 42), (300, 2, 5), 5)             # X belt
    return build_object("Gantry_X", g, mat)


def build_carriage(mat):
    g = Geo()
    g.add_box((0, 16, 38), (44, 8, 40), 2)                # carriage plate
    for i in range(5):                                    # heatsink fins
        g.add_box((0, 0, 17 + i * 4), (22, 22, 2.6), 9)
    g.add_box((0, 0, 10), (16, 12, 9), 13)                # heater block
    g.add_cone((0, 0, 0), 2.5, 5, 7)                      # nozzle, tip = origin
    g.add_box((0, -13, 24), (24, 8, 24), 8)               # fan shroud
    g.add_box((0, 16, 62), (18, 10, 8), 10)               # cable box
    return build_object("Carriage_X", g, mat)


def build_bed(mat):
    g = Geo()
    g.add_box((110, 110, -2.5), (220, 220, 5), 6, pal_top=3)   # bed slab
    g.add_box((110, 110, -8), (160, 240, 6), 6)                # Y carriage
    for x in (25, 195):                                        # leveling knobs
        for y in (25, 195):
            g.add_cylinder((x, y, -11), 7, 5, 5, segs=6)
    g.add_box((5, 5, 1), (10, 10, 2), 4)      # orange notch = gcode origin
    g.add_box((110, 224, -6), (30, 8, 4), 10)                  # cable strip
    return build_object("Bed_Y", g, mat)


def build_lead_screw(name, x, mat):
    g = Geo()
    g.add_cylinder((0, 0, 0), 3.5, 230, 1, segs=6)
    # Helical "flights": stepped, progressively rotated fins that read as a
    # spinning screw once the app rotates the node around local Y.
    for k in range(9):
        g.add_box((0, 0, -96 + k * 24), (10, 2.4, 2.4), 15, rot_z=k * 0.8)
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
    import os

    os.makedirs(os.path.dirname(out), exist_ok=True)

    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)

    palette_path = os.path.join(os.path.dirname(out), "..", "textures", "psx_palette.png")
    palette_path = os.path.abspath(palette_path)
    os.makedirs(os.path.dirname(palette_path), exist_ok=True)
    mat = make_material(palette_path)

    build_frame(mat)
    build_gantry(mat)
    build_carriage(mat)
    build_bed(mat)
    build_lead_screw("LeadScrew_L", -22, mat)
    build_lead_screw("LeadScrew_R", 242, mat)

    bpy.ops.export_scene.gltf(filepath=out, export_format="GLB", export_yup=True)
    validate_glb(out)


main()
