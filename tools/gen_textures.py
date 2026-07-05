"""Generate the PSX-industrial texture atlas and the R&D-facility floor.

Run from the repo root:  python3 tools/gen_textures.py

Everything is procedural (numpy value noise + Pillow drawing + the vendored
VT323 for stencil decals), deterministic (seeded), and finished with a 4x4
Bayer ordered dither + 32-color quantize for the PSX grain.

Outputs:
  app/assets/textures/psx_atlas.png  (256x256, referenced by printer.glb)
  app/assets/textures/floor.png      (512x512, mapped 1:1 on the base plate)
"""

import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, os.path.dirname(__file__))
from texture_regions import ATLAS_SIZE, REGIONS

OUT_DIR = os.path.join("app", "assets", "textures")
FONT_PATH = os.path.join("app", "fonts", "VT323-Regular.ttf")

rng = np.random.default_rng(19980903)  # MGS1 JP release date


# ------------------------------------------------------------ primitives --


def value_noise(w, h, octaves=((8, 1.0), (4, 0.5), (2, 0.25))):
    """Multi-octave value noise in [0,1], (h, w) array."""
    out = np.zeros((h, w))
    total = 0.0
    for cell, amp in octaves:
        gw, gh = max(1, w // cell), max(1, h // cell)
        grid = rng.random((gh, gw))
        img = Image.fromarray((grid * 255).astype(np.uint8)).resize(
            (w, h), Image.BILINEAR
        )
        out += amp * np.asarray(img, dtype=np.float64) / 255.0
        total += amp
    return out / total


def brushed(w, h, vertical=False):
    """Brushed-metal streaks: 1-D noise stretched along one axis."""
    if vertical:
        line = rng.random(w) * 0.5 + rng.random(w // 4).repeat(4)[:w] * 0.5
        return np.tile(line, (h, 1))
    line = rng.random(h) * 0.5 + rng.random(h // 4).repeat(4)[:h] * 0.5
    return np.tile(line[:, None], (1, w))


def shade(base_rgb, luminance):
    """(h,w) luminance -> (h,w,3) uint8 around a base color."""
    arr = np.stack([luminance * c for c in base_rgb], axis=-1)
    return (np.clip(arr, 0.0, 1.0) * 255).astype(np.uint8)


def to_image(rgb_arr):
    return Image.fromarray(rgb_arr, "RGB")


def font(size):
    return ImageFont.truetype(FONT_PATH, size)


def stencil(draw, xy, text, size, fill=(214, 224, 210), anchor="mm"):
    draw.text(xy, text, font=font(size), fill=fill, anchor=anchor)


# --------------------------------------------------------------- regions --


def make_gunmetal(w, h):
    base = (0.30, 0.32, 0.38)
    lum = 0.75 + 0.25 * (0.6 * brushed(w, h) + 0.4 * value_noise(w, h))
    img = to_image(shade(base, lum))
    d = ImageDraw.Draw(img)
    # panel seams
    for x in (w // 3, 2 * w // 3):
        d.line([(x, 0), (x, h)], fill=(28, 30, 36), width=1)
        d.line([(x + 1, 0), (x + 1, h)], fill=(78, 84, 96), width=1)
    d.line([(0, h // 2), (w, h // 2)], fill=(28, 30, 36), width=1)
    # rivets along the seams
    for x in range(6, w, 14):
        for y in (6, h - 7):
            d.ellipse([x - 1, y - 1, x + 1, y + 1], fill=(30, 33, 40))
            d.point((x - 1, y - 1), fill=(120, 128, 140))
    # edge wear
    for _ in range(30):
        x = int(rng.integers(0, w))
        edge = int(rng.integers(0, 2)) * (h - 1)
        d.point((x, edge), fill=(105, 112, 124))
    return img


def make_olive(w, h, label=False):
    base = (0.55, 0.60, 0.42)
    lum = 0.75 + 0.25 * value_noise(w, h)
    img = to_image(shade(base, lum))
    d = ImageDraw.Draw(img)
    # scratches revealing steel
    for _ in range(14):
        x0 = int(rng.integers(2, w - 8))
        y0 = int(rng.integers(2, h - 8))
        dx = int(rng.integers(3, 9)) * (1 if rng.random() < 0.5 else -1)
        dy = int(rng.integers(0, 3))
        d.line([(x0, y0), (x0 + dx, y0 + dy)], fill=(122, 126, 118), width=1)
    if label:
        # riveted label plate with unit designation + barcode
        d.rectangle([6, 8, w - 7, 30], fill=(22, 26, 20), outline=(90, 96, 80))
        stencil(d, (w // 2, 15), "PWR-7700", 13)
        stencil(d, (w // 2, 25), "24V DC  R&D", 9, fill=(150, 160, 140))
        bx = 10
        while bx < w - 12:
            bw = int(rng.integers(1, 3))
            if rng.random() < 0.6:
                d.rectangle([bx, 38, bx + bw - 1, 48], fill=(20, 22, 18))
            bx += bw + 1
        d.rectangle([8, 36, w - 9, 50], outline=(90, 96, 80))
        # warning triangle
        d.polygon([(14, h - 6), (22, h - 6), (18, h - 13)], outline=(190, 170, 60))
        stencil(d, (18, h - 9), "!", 8, fill=(190, 170, 60))
    return img


def make_steel(w, h):
    base = (0.62, 0.65, 0.70)
    lum = 0.72 + 0.28 * (0.7 * brushed(w, h, vertical=True) + 0.3 * value_noise(w, h))
    img = to_image(shade(base, lum))
    d = ImageDraw.Draw(img)
    for _ in range(10):
        x = int(rng.integers(0, w))
        y = int(rng.integers(0, h))
        d.point((x, y), fill=(60, 64, 70))
    return img


def make_bed_top(w, h):
    base = (0.30, 0.55, 0.40)
    lum = 0.30 + 0.10 * value_noise(w, h)
    img = to_image(shade(base, lum))
    d = ImageDraw.Draw(img)
    grid = (26, 52, 38)
    for x in range(0, w, 8):
        d.line([(x, 0), (x, h)], fill=grid, width=1)
    for y in range(0, h, 8):
        d.line([(0, y), (w, y)], fill=grid, width=1)
    # corner crop marks + center crosshair
    bright = (60, 120, 86)
    for cx, cy in [(3, 3), (w - 4, 3), (3, h - 4), (w - 4, h - 4)]:
        d.line([(cx - 2, cy), (cx + 2, cy)], fill=bright)
        d.line([(cx, cy - 2), (cx, cy + 2)], fill=bright)
    d.line([(w // 2 - 3, h // 2), (w // 2 + 3, h // 2)], fill=bright)
    d.line([(w // 2, h // 2 - 3), (w // 2, h // 2 + 3)], fill=bright)
    return img


def hazard_mask(w, h, stripe=8):
    """Boolean (h,w): True where the diagonal stripe is yellow."""
    ys, xs = np.mgrid[0:h, 0:w]
    return ((xs - ys) // stripe) % 2 == 0


def make_hazard(w, h):
    yellow = np.array([196.0, 158.0, 28.0])
    black = np.array([26.0, 26.0, 24.0])
    arr = np.where(hazard_mask(w, h)[:, :, None], yellow, black)
    arr *= (0.8 + 0.2 * value_noise(w, h))[:, :, None]
    return to_image(arr.astype(np.uint8))


def make_caution(w, h):
    base = (0.30, 0.32, 0.30)
    lum = 0.6 + 0.2 * value_noise(w, h)
    img = to_image(shade(base, lum))
    strip = np.where(
        hazard_mask(w, 4)[:, :, None],
        np.array([172.0, 140.0, 30.0]),
        np.array([24.0, 24.0, 22.0]),
    )
    img.paste(to_image(strip.astype(np.uint8)), (0, 0))
    d = ImageDraw.Draw(img)
    stencil(d, (w // 2, h // 2 + 2), "CAUTION", 14)
    stencil(d, (w // 2, h - 7), "HOT SURFACE", 9, fill=(180, 150, 60))
    return img


def make_swatch(w, h, rgb, variance=0.06):
    lum = 1.0 - variance + variance * 2.0 * value_noise(w, h, ((4, 1.0),))
    return to_image(shade(rgb, lum))


# ---------------------------------------------------------------- floor --


def make_floor(size=512):
    # Concrete slab. The base plate is 380x560 mm centered on the machine
    # (world x -80..300, z -150..410); UVs span it 1:1.
    base = (0.42, 0.44, 0.42)
    lum = 0.62 + 0.28 * value_noise(size, size, ((32, 1.0), (8, 0.5), (3, 0.3)))
    img = to_image(shade(base, lum))
    d = ImageDraw.Draw(img)

    # expansion joints
    joint, hi = (34, 36, 34), (96, 100, 96)
    for p in range(0, size + 1, 128):
        p = min(p, size - 1)
        d.line([(p, 0), (p, size)], fill=joint, width=2)
        d.line([(p + 2, 0), (p + 2, size)], fill=hi, width=1)
        d.line([(0, p), (size, p)], fill=joint, width=2)
        d.line([(0, p + 2), (size, p + 2)], fill=hi, width=1)

    # stains
    for _ in range(10):
        x = int(rng.integers(0, size))
        y = int(rng.integers(0, size))
        r = int(rng.integers(8, 36))
        stain = Image.new("L", (2 * r, 2 * r), 0)
        ImageDraw.Draw(stain).ellipse([0, 0, 2 * r - 1, 2 * r - 1], fill=40)
        img.paste(Image.new("RGB", (2 * r, 2 * r), (18, 20, 18)), (x - r, y - r), stain)
    d = ImageDraw.Draw(img)

    # hazard border around the machine envelope (x -60..280, z -140..360 mm
    # of the 380x560 plate starting at (-80, -150)); painted as a masked
    # stripe field so the concrete joints underneath stay intact elsewhere.
    def px(mm_x, mm_z):
        return (
            int((mm_x + 80.0) / 380.0 * size),
            int((mm_z + 150.0) / 560.0 * size),
        )

    x0, y0 = px(-60, -140)
    x1, y1 = px(280, 360)
    band = 10
    arr = np.asarray(img, dtype=np.float64).copy()
    ys, xs = np.mgrid[0:size, 0:size]
    frame = (
        (xs >= x0) & (xs < x1) & (ys >= y0) & (ys < y1)
        & ~((xs >= x0 + band) & (xs < x1 - band) & (ys >= y0 + band) & (ys < y1 - band))
    )
    stripes = ((xs - ys) // 12) % 2 == 0
    yellow = np.array([176.0, 142.0, 30.0])
    black = np.array([30.0, 30.0, 28.0])
    paint = np.where(stripes[:, :, None], yellow, black)
    arr = np.where(frame[:, :, None], paint, arr)
    img = to_image(arr.astype(np.uint8))
    d = ImageDraw.Draw(img)

    # floor stencils, inside the border where the plate has room
    stencil(d, ((x0 + x1) // 2, y1 - band - 26), "BAY 03", 30, fill=(150, 154, 146))
    stencil(d, ((x0 + x1) // 2, y1 - band - 8), "FAB LINE - AUTHORIZED ONLY", 13, fill=(120, 124, 116))

    # worn paint: multiply everything by mild noise
    arr = np.asarray(img, dtype=np.float64)
    arr *= (0.88 + 0.12 * value_noise(size, size, ((6, 1.0),)))[:, :, None]
    return to_image(arr.astype(np.uint8))


# ------------------------------------------------------------ finishing --

BAYER4 = (
    np.array(
        [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]],
        dtype=np.float64,
    )
    / 16.0
    - 0.5
)


def psx_finish(img, colors=32, dither_strength=10.0):
    """Ordered Bayer dither + adaptive-palette quantize."""
    arr = np.asarray(img, dtype=np.float64)
    h, w = arr.shape[:2]
    tiles = np.tile(BAYER4, (h // 4 + 1, w // 4 + 1))[:h, :w]
    arr = np.clip(arr + tiles[:, :, None] * dither_strength, 0, 255)
    dithered = Image.fromarray(arr.astype(np.uint8), "RGB")
    return dithered.quantize(colors=colors, dither=Image.Dither.NONE).convert("RGB")


def main():
    os.makedirs(OUT_DIR, exist_ok=True)

    atlas = Image.new("RGB", (ATLAS_SIZE, ATLAS_SIZE), (8, 8, 8))
    makers = {
        "gunmetal": make_gunmetal,
        "olive": lambda w, h: make_olive(w, h, label=False),
        "psu_decal": lambda w, h: make_olive(w, h, label=True),
        "steel": make_steel,
        "bed_top": make_bed_top,
        "hazard": make_hazard,
        "caution_decal": make_caution,
        "brass": lambda w, h: make_swatch(w, h, (0.62, 0.50, 0.22)),
        "rubber": lambda w, h: make_swatch(w, h, (0.10, 0.10, 0.11)),
        "alu": lambda w, h: make_swatch(w, h, (0.55, 0.57, 0.60)),
        "dark_steel": lambda w, h: make_swatch(w, h, (0.22, 0.24, 0.26)),
        "orange": lambda w, h: make_swatch(w, h, (0.85, 0.42, 0.10), 0.03),
        "cable": lambda w, h: make_swatch(w, h, (0.12, 0.12, 0.14)),
    }
    for name, (x, y, w, h) in REGIONS.items():
        atlas.paste(makers[name](w, h), (x, y))
    atlas = psx_finish(atlas, colors=32)
    # The tiny solid swatches contribute too few pixels to earn palette
    # entries; re-paste them after quantization so brass/orange survive.
    for name in ("brass", "rubber", "alu", "dark_steel", "orange", "cable"):
        x, y, w, h = REGIONS[name]
        atlas.paste(makers[name](w, h), (x, y))
    atlas_path = os.path.join(OUT_DIR, "psx_atlas.png")
    atlas.save(atlas_path)
    print(f"wrote {atlas_path}")

    floor = psx_finish(make_floor(), colors=24, dither_strength=8.0)
    floor_path = os.path.join(OUT_DIR, "floor.png")
    floor.save(floor_path)
    print(f"wrote {floor_path}")


if __name__ == "__main__":
    main()
