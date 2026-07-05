"""Shared atlas contract between gen_textures.py (system python) and
gen_printer_assets.py (Blender's python): region name -> (x, y, w, h) texels.

Textures are authored top-left origin (PIL); UV space is bottom-left origin.
`region_uv_rect()` does that flip so both scripts agree on the mapping.
"""

ATLAS_SIZE = 256

REGIONS = {
    # main surfaces
    "gunmetal": (0, 0, 128, 64),      # brushed dark panels: frame, beam, brackets
    "olive": (128, 0, 64, 64),        # painted olive: steppers, carriage plate
    "psu_decal": (192, 0, 64, 64),    # olive + unit label + barcode (PSU front)
    "steel": (0, 64, 64, 64),         # brushed steel: rails, screws, crossbars
    "bed_top": (64, 64, 64, 64),      # buildplate grid
    "hazard": (128, 64, 64, 32),      # yellow/black diagonal stripes
    "caution_decal": (128, 96, 64, 32),  # CAUTION HOT plate
    # solid-ish swatches
    "brass": (192, 64, 16, 16),
    "rubber": (208, 64, 16, 16),
    "alu": (224, 64, 16, 16),
    "dark_steel": (240, 64, 16, 16),
    "orange": (192, 80, 16, 16),      # origin notch marker
    "cable": (208, 80, 16, 16),
}

# UV inset in texels, so fit-to-region sampling can't bleed into neighbors.
REGION_INSET = 0.5


def region_uv_rect(name):
    """(u0, v0, u1, v1) in glTF/Bevy UV space (v=0 is texture TOP in glTF).

    glTF UV origin is top-left, same as PIL, so no vertical flip is needed:
    v = y / ATLAS_SIZE.
    """
    x, y, w, h = REGIONS[name]
    s = float(ATLAS_SIZE)
    return (
        (x + REGION_INSET) / s,
        (y + REGION_INSET) / s,
        (x + w - REGION_INSET) / s,
        (y + h - REGION_INSET) / s,
    )
