"""MGS-style REXTRUDE logo for the README header.

Renders docs/logo-light.png (black ink) and docs/logo-dark.png (white ink),
both RGBA on a transparent background, from a single procedural ink mask:
custom polygon letterforms for the wordmark, a tracked tagline, and a solid
bar with the bar text knocked out (transparent, so the page shows through).

The layout constants in TARGETS were derived once from the Metal Gear Solid
logo (reference image not committed) via:

    python3 tools/gen_logo.py --analyze <path-to-reference.jpg>

`--check` re-measures the committed PNGs with the same analyze() and gates
them against TARGETS, rigcheck-style. `--check --regen` additionally
regenerates into a temp dir and compares sha256 with the committed files;
this is kept out of plain `--check` (and CI) because byte-identical output
is only guaranteed for matching Pillow/freetype versions.

Deterministic by construction: no randomness, fixed fonts, fixed geometry.
"""

from __future__ import annotations

import argparse
import hashlib
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
OUT_LIGHT = DOCS / "logo-light.png"
OUT_DARK = DOCS / "logo-dark.png"

TAGLINE = "TACTICAL EXTRUSION ACTION"
WORDMARK = "REXTRUDE"
BAR_TEXT = "SIMULATOR"

FONT = "/usr/share/fonts/truetype/roboto/unhinted/RobotoCondensed-Bold.ttf"

# Output geometry: 4x the 420x88 reference canvas, supersampled 4x while drawing.
OUT_W, OUT_H = 1680, 352
SS = 4

# Design targets measured from the MGS reference via --analyze (see module docstring).
TARGETS = {
    "aspect": 4.7727,  # canvas w/h
    "tagline_h": 0.0795,  # tagline band height / canvas h
    "tagline_w": 0.8119,  # tagline ink width / canvas w
    "gap": 0.0568,  # tagline->wordmark gap / canvas h
    "block_h": 0.8182,  # wordmark block height / canvas h
    "block_ink": 0.6182,  # ink density inside the wordmark block
    "bar_h": 0.2222,  # bar height / block height
    "bar_w": 0.9667,  # bar width / canvas w
    "bar_offset": 0.0,  # gap between bar bottom and block bottom / block height
    "bar_gaps": 0.0025,  # empty columns inside the bar extent / bar width (a real bar has ~none)
    "letter_bar_gap": 0.0694,  # clearance between letter bottoms and bar top / block height
    "stroke": 0.2083,  # median horizontal ink run / block height
}
# Relative tolerance per metric for --check ("abs" entries are absolute).
TOLERANCES = {
    "aspect": 0.02,
    "tagline_h": 0.15,
    "tagline_w": 0.15,
    "gap": 0.25,
    "block_h": 0.10,
    "block_ink": 0.20,
    "bar_h": 0.15,
    "bar_w": 0.05,
    "bar_offset": ("abs", 0.02),
    "bar_gaps": ("abs", 0.005),
    "letter_bar_gap": 0.30,  # relative, so an overlap (gap 0) can never pass
    "stroke": 0.30,
}

TOP_MARGIN = 0.0114  # blank rows above the tagline / canvas h (from reference)
SIDE_MARGIN = 0.0024  # wordmark side margin / canvas w (reference runs nearly edge to edge)
LETTER_GAP = 3.0  # grid units between glyphs (reference letters nearly touch)
BAR_TEXT_WIDTH = 0.40  # bar-text span / bar width (SOLID is narrow inside its bar)
BAR_TEXT_CAP = 0.52  # bar-text cap height / bar height


# --- measurement ---------------------------------------------------------------


def ink_from_image(img: Image.Image) -> np.ndarray:
    """Boolean ink mask: alpha channel for RGBA output, darkness for the reference JPG."""
    if img.mode == "RGBA":
        alpha = np.asarray(img.getchannel("A"))
        if (alpha < 255).any():
            return alpha > 128
    return np.asarray(img.convert("L")) < 128


def _runs(bools: np.ndarray) -> list[tuple[int, int]]:
    """(start, end) half-open runs of True."""
    d = np.diff(np.concatenate(([0], bools.astype(np.int8), [0])))
    return list(zip(np.nonzero(d == 1)[0], np.nonzero(d == -1)[0]))


def analyze(mask: np.ndarray) -> dict[str, float]:
    """Measure the logo layout ratios from a boolean ink mask (shared by all modes)."""
    h, w = mask.shape
    bands = _runs(mask.mean(axis=1) > 0.01)
    if len(bands) < 2:
        raise ValueError(f"expected tagline + wordmark bands, found {len(bands)}")
    tag = bands[0]
    # The block is everything below the tagline: letters and bar may render as
    # separate bands (they are only merged in the reference by residual ink).
    block = (bands[1][0], bands[-1][1])
    tag_cols = np.nonzero(mask[tag[0] : tag[1]].mean(axis=0) > 0.02)[0]
    block_h = block[1] - block[0]
    blk = mask[block[0] : block[1]]

    # The bar is the near-solid row run at the bottom of the block (letter top/bottom
    # arms can also be near-solid across the width, so runs away from the bottom are
    # letter geometry, not the bar). 0.8 keeps knocked-out text rows (~0.85) in the
    # run while rows above the bar stay well below (~0.6).
    solid_runs = [r for r in _runs(blk.mean(axis=1) > 0.8) if r[1] >= 0.95 * block_h]
    if solid_runs:
        start, end = solid_runs[-1]
        bar_cols = np.nonzero(blk[start:end].mean(axis=0) > 0.9)[0]
        bar_h = (end - start) / block_h
        bar_offset = (block_h - end) / block_h
        letters = np.concatenate([blk[:start], blk[end:]])
        # Clearance between the letter bottoms and the bar top: rows above the bar
        # count as letter ink only above 0.1 (the reference has ~0.06 residual ink
        # in its gap rows from the trademark glyph).
        letter_rows = np.nonzero(blk[:start].mean(axis=1) > 0.1)[0]
        letter_bar_gap = (start - 1 - letter_rows.max()) / block_h if len(letter_rows) else 0.0
        if len(bar_cols):
            extent = blk[start:end, bar_cols.min() : bar_cols.max() + 1]
            bar_w = extent.shape[1] / w
            # Empty columns inside the extent mean the "bar" is really exposed
            # letter geometry with gaps, not one contiguous bar.
            bar_gaps = float((extent.mean(axis=0) < 0.05).mean())
        else:
            bar_w = bar_gaps = 0.0
    else:
        bar_h = bar_w = bar_offset = bar_gaps = letter_bar_gap = 0.0
        letters = blk

    run_lengths = [e - s for row in letters for s, e in _runs(row)]
    stroke = float(np.median(run_lengths)) / block_h if run_lengths else 0.0

    return {
        "aspect": w / h,
        "tagline_h": (tag[1] - tag[0]) / h,
        "tagline_w": (tag_cols.max() - tag_cols.min() + 1) / w if len(tag_cols) else 0.0,
        "gap": (block[0] - tag[1]) / h,
        "block_h": block_h / h,
        "block_ink": float(blk.mean()),
        "bar_h": bar_h,
        "bar_w": bar_w,
        "bar_offset": bar_offset,
        "bar_gaps": bar_gaps,
        "letter_bar_gap": letter_bar_gap,
        "stroke": stroke,
    }


# --- wordmark glyphs -----------------------------------------------------------


def _rect(x0: float, y0: float, x1: float, y1: float) -> list[tuple[float, float]]:
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def glyph_polys(ch: str, w: float, s: float) -> tuple[list, list]:
    """(add, cut) polygons for one letter on a cap-height-100 grid, y down.

    Squared MGS-style strokes; curves are hinted with 45-degree chamfer cuts.
    Horizontal arms render thinner than vertical stems (`arm` vs `s`) since a
    horizontal stroke measured equal to a vertical one reads heavier at this
    weight; diagonals use `diag` for the same optical-weight reason.
    """
    arm = 0.92 * s  # horizontal-arm thickness, optically balanced against s
    diag = 1.2 * s  # diagonal-stroke horizontal projection, optically balanced against s
    c = 0.75 * s  # chamfer size, shared by every curve hint
    if ch == "R":
        # Crown (top arm + bowl) is one solid rect, not two arm-thickness rects:
        # at this stroke weight the two would nearly touch, and thinning them
        # independently to `arm` can open an unintended hairline seam between
        # them. The design has no open bowl counter here, only the leg below.
        return [
            _rect(0, 0, s, 100),
            _rect(0, 0, w, 55),
            [(w - 1.4 * s, 55), (w - 0.2 * s, 55), (w, 100), (w - diag, 100)],
        ], []
    if ch == "E":
        return [
            _rect(0, 0, s, 100),
            _rect(0, 0, w, arm),
            _rect(0, 100 - arm, w, 100),
            _rect(0, 48 - arm / 2, 0.88 * w, 48 + arm / 2),
        ], []
    if ch == "X":
        return [
            [(0, 0), (diag, 0), (w, 100), (w - diag, 100)],
            [(w - diag, 0), (w, 0), (diag, 100), (0, 100)],
        ], []
    if ch == "T":
        return [_rect(0, 0, w, arm), _rect(w / 2 - s / 2, 0, w / 2 + s / 2, 100)], []
    if ch == "U":
        return [
            _rect(0, 0, s, 100),
            _rect(w - s, 0, w, 100),
            _rect(0, 100 - arm, w, 100),
        ], [
            [(0, 100), (0, 100 - c), (c, 100)],
            [(w, 100), (w - c, 100), (w, 100 - c)],
        ]
    if ch == "D":
        return [
            _rect(0, 0, s, 100),
            _rect(0, 0, w, arm),
            _rect(0, 100 - arm, w, 100),
            _rect(w - s, 0, w, 100),
        ], [
            [(w, 0), (w - c, 0), (w, c)],
            [(w, 100), (w - c, 100), (w, 100 - c)],
        ]
    raise ValueError(f"no glyph for {ch!r}")


# --- rendering -----------------------------------------------------------------


def _snap(v: float) -> float:
    """Round a supersampled coordinate to the nearest output-pixel boundary.

    Forces every stem edge to the same subpixel phase before the LANCZOS
    downscale, so nominally-identical strokes don't pick up different
    apparent weight from landing at different fractional offsets.
    """
    return round(v / SS) * SS


def _font_for_cap(path: str, cap_px: float) -> ImageFont.FreeTypeFont:
    """Font whose uppercase cap height is close to cap_px."""
    probe = ImageFont.truetype(path, 100)
    box = probe.getbbox("T")
    return ImageFont.truetype(path, max(1, round(100 * cap_px / (box[3] - box[1]))))


def _draw_tracked(
    draw: ImageDraw.ImageDraw,
    text: str,
    font: ImageFont.FreeTypeFont,
    center_x: float,
    top_y: float,
    target_w: float,
    fill: int,
) -> None:
    """Draw text glyph-by-glyph, tracked to span target_w, cap top at top_y.

    Word spaces get double tracking so word breaks read as a deliberate gap
    rather than an accident of the space glyph's own (usually narrow) width.
    """
    widths = [font.getlength(ch) for ch in text]
    gap_weights = [2.0 if ch == " " else 1.0 for ch in text[:-1]]
    tracking = (target_w - sum(widths)) / sum(gap_weights) if gap_weights else 0.0
    cap_top = font.getbbox("T")[1]
    x = center_x - target_w / 2
    y = _snap(top_y - cap_top)
    for i, (ch, cw) in enumerate(zip(text, widths)):
        draw.text((_snap(x), y), ch, font=font, fill=fill)
        if i < len(text) - 1:
            x += cw + gap_weights[i] * tracking


def render_mask() -> Image.Image:
    """The full lockup as a grayscale ink mask at output size (antialiased)."""
    w, h = OUT_W * SS, OUT_H * SS
    mask = Image.new("L", (w, h), 0)
    draw = ImageDraw.Draw(mask)

    tag_top = _snap(TOP_MARGIN * h)
    tag_h = TARGETS["tagline_h"] * h
    block_top = _snap(tag_top + tag_h + TARGETS["gap"] * h)
    block_h = TARGETS["block_h"] * h
    block_bottom = _snap(block_top + block_h)

    # 1. Tagline, tracked to the measured span.
    _draw_tracked(
        draw,
        TAGLINE,
        _font_for_cap(FONT, tag_h),
        w / 2,
        tag_top,
        TARGETS["tagline_w"] * w,
        255,
    )

    # 2. Wordmark: custom glyphs spanning the canvas, ending above the bar with
    #    the measured clearance (the reference letters do not touch the bar).
    letters_h = block_h * (1 - TARGETS["bar_h"] - TARGETS["letter_bar_gap"])
    unit = letters_h / 100.0
    margin = SIDE_MARGIN * w
    total_units = (w - 2 * margin) / unit
    glyph_w = (total_units - LETTER_GAP * (len(WORDMARK) - 1)) / len(WORDMARK)
    # Stroke weight is measured relative to the whole block; keep it absolute
    # rather than scaling it down with the shorter letter height. Snapped so
    # independently-rounded left/right stem edges always land the same
    # integer number of output pixels apart, for every glyph.
    stroke = _snap(TARGETS["stroke"] * block_h / letters_h * 100.0 * unit) / unit
    # Snap the per-glyph advance to whole output pixels too, then recompute
    # the actual rendered span and recenter: cumulative advances are now
    # exact multiples of the output-pixel grid, so no float drift can creep
    # into the wordmark's centering.
    advance_px = _snap((glyph_w + LETTER_GAP) * unit)
    total_span = advance_px * (len(WORDMARK) - 1) + glyph_w * unit
    x = _snap((w - total_span) / 2)
    for ch in WORDMARK:
        add, cut = glyph_polys(ch, glyph_w, stroke)
        for poly, fill in [(p, 255) for p in add] + [(p, 0) for p in cut]:
            draw.polygon([(_snap(x + px * unit), _snap(block_top + py * unit)) for px, py in poly], fill=fill)
        x += advance_px

    # 3. Bar over the letters' lower portion, flush with the block bottom,
    #    with the bar text knocked out (transparent in the final PNGs).
    bar_w = TARGETS["bar_w"] * w
    bar_h = TARGETS["bar_h"] * block_h
    bar_top = _snap(block_bottom - bar_h)
    bar_left = _snap((w - bar_w) / 2)
    bar_right = _snap((w + bar_w) / 2)
    draw.rectangle([bar_left, bar_top, bar_right, block_bottom], fill=255)
    cap = BAR_TEXT_CAP * bar_h
    _draw_tracked(
        draw,
        BAR_TEXT,
        _font_for_cap(FONT, cap),
        w / 2,
        bar_top + (bar_h - cap) / 2,
        BAR_TEXT_WIDTH * bar_w,
        0,
    )

    return mask.resize((OUT_W, OUT_H), Image.LANCZOS)


def colorize(mask: Image.Image, rgb: tuple[int, int, int]) -> Image.Image:
    out = Image.new("RGBA", mask.size, rgb + (0,))
    out.putalpha(mask)
    return out


def generate(out_dir: Path) -> list[Path]:
    mask = render_mask()
    out_dir.mkdir(parents=True, exist_ok=True)
    paths = [out_dir / OUT_LIGHT.name, out_dir / OUT_DARK.name]
    colorize(mask, (10, 10, 10)).save(paths[0])
    colorize(mask, (240, 240, 240)).save(paths[1])
    return paths


# --- check (rigcheck-style gate on the committed PNGs) --------------------------


def check(regen: bool) -> int:
    failures = []

    def expect(name: str, ok: bool, detail: str) -> None:
        print(f"{'ok  ' if ok else 'FAIL'} {name}: {detail}")
        if not ok:
            failures.append(name)

    light = Image.open(OUT_LIGHT)
    dark = Image.open(OUT_DARK)
    for img, path in [(light, OUT_LIGHT), (dark, OUT_DARK)]:
        expect(f"rgba:{path.name}", img.mode == "RGBA", f"mode={img.mode}")
    la, da = np.asarray(light.getchannel("A")), np.asarray(dark.getchannel("A"))
    expect("theme-parity", np.array_equal(la, da), "light/dark alpha channels identical")
    corners = [la[0, 0], la[0, -1], la[-1, 0], la[-1, -1]]
    expect("transparent-corners", max(corners) == 0, f"corner alphas {corners}")

    mask = la > 128
    measured = analyze(mask)
    for name, target in TARGETS.items():
        tol = TOLERANCES[name]
        if isinstance(tol, tuple):
            ok = abs(measured[name] - target) <= tol[1]
        else:
            ok = abs(measured[name] - target) <= tol * abs(target)
        expect(name, ok, f"measured {measured[name]:.4f}, target {target:.4f}")

    # Bar-text knockout: transparent letter strokes inside the otherwise solid bar.
    h = mask.shape[0]
    block_bottom = round((TOP_MARGIN + TARGETS["tagline_h"] + TARGETS["gap"] + TARGETS["block_h"]) * h)
    bar = mask[round(block_bottom - TARGETS["bar_h"] * TARGETS["block_h"] * h) : block_bottom]
    holed = _runs((~bar).mean(axis=0) > 0.15)
    expect("knockout", len(holed) >= len(BAR_TEXT), f"{len(holed)} knockout column runs (want >= {len(BAR_TEXT)})")

    if regen:
        with tempfile.TemporaryDirectory() as tmp:
            for fresh, committed in zip(generate(Path(tmp)), [OUT_LIGHT, OUT_DARK]):
                same = hashlib.sha256(fresh.read_bytes()).digest() == hashlib.sha256(committed.read_bytes()).digest()
                expect(f"regen:{committed.name}", same, "regenerated bytes match committed file")

    if failures:
        print(f"\n{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("\nall checks passed")
    return 0


# --- cli -----------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--analyze", metavar="IMAGE", help="measure a reference image and print the parameter dict")
    parser.add_argument("--check", action="store_true", help="validate the committed PNGs against TARGETS")
    parser.add_argument("--regen", action="store_true", help="with --check: also regenerate and compare sha256")
    args = parser.parse_args()

    if args.analyze:
        measured = analyze(ink_from_image(Image.open(args.analyze)))
        print("{")
        for key, value in measured.items():
            print(f'    "{key}": {value:.4f},')
        print("}")
        return 0
    if args.check:
        return check(args.regen)
    for path in generate(DOCS):
        print(f"wrote {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
