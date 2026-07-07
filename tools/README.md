# Asset generation tools

Generated assets are committed to the repo, so these scripts only need to be
re-run when changing the printer model, textures, or the sound set.

## Textures (`app/assets/textures/*.png`)

Pillow + numpy, deterministic. Run from the repo root:

```sh
python3 tools/gen_textures.py
```

Builds `psx_atlas.png` (industrial regions + stencil decals; the atlas layout
lives in `texture_regions.py`, shared with the Blender script) and
`floor.png` (R&D-facility floor, mapped 1:1 on the base plate). Both are
finished with Bayer ordered dithering + a small quantized palette.

## Printer model (`app/assets/models/printer.glb`)

Requires Blender (tested with 5.1) and the atlas above. Run from the repo
root:

```sh
blender --background --factory-startup --python tools/gen_printer_assets.py -- \
    --out app/assets/models/printer.glb
```

The script builds the low-poly i3-style bedslinger, planar-maps every face
into its named atlas region (fit-to-region, oriented unmirrored for the
operator-side view), exports a GLB, and writes `printer.evidence.json` — a
tier-2 evidence manifest of labeled machine-space AABBs consumed by
`rigcheck` (see below).

Design/kinematic validation of the exported model is a separate step, via
the `rigcheck` CLI (`crates/rigcheck`, schema and check reference in
`crates/rigcheck/README.md`):

```sh
cargo run -p rigcheck --release -- check app/assets/models/printer.glb
```

This is gated in CI against the committed model. `rigcheck` is driven by the
sidecar spec `app/assets/models/printer.machine.toml`, which is the single
source of truth for the rig node contract — update it (not app code or this
script) when adding, renaming, or reshaping a rig node.

## Audio (`app/assets/audio/*.wav`)

Pure-stdlib Python (numpy optional). Run from the repo root:

```sh
python3 tools/gen_audio.py
```

Synthesizes the original MGS-inspired sound set: `codec_call.wav`,
`codec_beep.wav`, `ui_click.wav`, `alert.wav`, `stepper_hum.wav` (seamless
loop, pitch-shifted at runtime by head speed).
