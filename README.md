<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-dark.png">
    <img src="docs/logo-light.png" alt="REXTRUDE — Tactical Extrusion Action" width="560">
  </picture>
</div>

[![CI](https://github.com/Kodoque1/rextrude/actions/workflows/ci.yml/badge.svg)](https://github.com/Kodoque1/rextrude/actions/workflows/ci.yml)
[![Live Demo](https://img.shields.io/badge/demo-GitHub%20Pages-blue)](https://Kodoque1.github.io/rextrude/)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#license)

An MGS-styled, PSX-aesthetic 3D printer simulator built with [Bevy](https://bevy.org/):
G-code playback, thermal simulation, and real Marlin firmware emulated in-browser
on an ATmega2560 (RAMPS 1.4) via [avr8js](https://github.com/wokwi/avr8js).

**[Live demo](https://Kodoque1.github.io/rextrude/)**

## Demo

<video src="https://github.com/Kodoque1/rextrude/releases/download/demo-media/rextrude_clip.mp4" controls loop muted playsinline width="100%"></video>

## Quickstart

Native:

```bash
cargo run -p app                       # base build, no system deps
cargo run -p app --features audio      # needs libasound2-dev (ALSA) on Linux
```

Web (wasm):

```bash
npm ci --prefix emu
cd app && trunk serve                  # audio feature enabled via Trunk.toml (WebAudio)
```

## Project layout

| Path               | What                                                          |
|--------------------|----------------------------------------------------------------|
| `app/`             | Bevy application (native + wasm entry point)                   |
| `crates/motion`    | Motion/kinematics simulation                                   |
| `crates/gcode-sim` | G-code parsing and playback                                    |
| `emu/`             | avr8js Marlin firmware emulation harness (TypeScript)           |
| `tools/`           | Python asset generators — see [tools/README.md](tools/README.md) |
| `assets/`          | G-code demos, firmware hex, textures, audio (committed)         |

## License

Dual-licensed under MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
