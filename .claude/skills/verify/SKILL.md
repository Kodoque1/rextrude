---
name: verify
description: Build, launch, and drive the rextrude web app to verify changes end-to-end (screenshots via Playwright).
---

# Verifying rextrude changes at the browser surface

Most behavior (firmware mode especially) is wasm-only; verify in a real
browser. Native `cargo run -p app` only covers the Simulation backend.

## Build & serve

`cd app && trunk serve --port 8091` works: `app/Trunk.toml`'s `[watch]`
section ignores `assets/emu_bridge.js` (the `pre_build` hook's output — without
that ignore, trunk loops forever rebuilding its own artifact) and also watches
`../emu/src` so bridge TypeScript edits rebuild. Keep that section intact.

For a fully static run (no live-reload resets at all, e.g. long driven
sessions):

```bash
cd app && trunk build            # one-shot; runs the emu bridge hook itself
cd dist && python3 -m http.server 8092
```

## Drive with Playwright (headed only)

Headless Chromium (SwiftShader) loses the WebGL context ("CONTEXT_LOST_WEBGL")
and the canvas stays blank — launch headed on the real display:

```js
import { chromium } from '<playwright>/index.mjs'; // npx cache has it
const browser = await chromium.launch({ headless: false }); // DISPLAY=:1
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
await page.goto('http://127.0.0.1:8092/');
await page.waitForTimeout(12000); // wasm boot; shorter waits miss clicks
```

- The UI is egui-on-canvas: no DOM to query — click by coordinates from a
  screenshot. At 1280x800 with the default layout: FIRMWARE backend button
  (1135, 87), SIMULATION (1017, 87), first example gcode (1063, 140),
  LOAD MARLIN (1066, 162), BROWSE in SEND GCODE (1155, 262).
- Bevy `warn!`/`info!` reach the browser console — capture `page.on('console')`
  to observe internal paths (e.g. import errors).
- Drag & drop: dispatch a synthetic `DragEvent('drop', { dataTransfer })` on
  `document` from `page.evaluate` (the listener is on `document`).
- BROWSE: rfd creates a hidden `<input type=file>` but Playwright's
  `filechooser` event does NOT fire — grab `page.$('input[type=file]')`
  after clicking BROWSE and call `setInputFiles` on it.
- Alert overlays hard-blink (invisible ~50% of frames) for their first
  2.4s — screenshot at least 2.7s after raising one, or take a burst.
- Audio can't be verified (AudioContext needs a user gesture).
