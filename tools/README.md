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
operator-side view), exports a GLB, and asserts that all rig node names the
app looks for (`Frame_Static`, `Gantry_X`, `Carriage_X`, `Bed_Y`,
`LeadScrew_L`, `LeadScrew_R`) survived the export.

## Audio (`app/assets/audio/*.wav`)

Pure-stdlib Python (numpy optional). Run from the repo root:

```sh
python3 tools/gen_audio.py
```

Synthesizes the original MGS-inspired sound set: `codec_call.wav`,
`codec_beep.wav`, `ui_click.wav`, `alert.wav`, `stepper_hum.wav` (seamless
loop, pitch-shifted at runtime by head speed).
