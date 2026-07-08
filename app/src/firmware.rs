//! Firmware-emulation backend (phase B): drives the JS/avr8js-based
//! ATmega2560 + RAMPS 1.4 emulator (bundled from `emu/` into
//! `assets/emu_bridge.js`) and feeds its step-event stream into the same
//! `PrintState`/layers rendering pipeline the gcode-sim backend uses.
//! WASM-only: avr8js only runs in a JS engine, so there's no native path.
#![cfg(target_arch = "wasm32")]

use bevy::prelude::*;
use motion::MotionEvent;
use wasm_bindgen::prelude::*;

use crate::playback::PrintState;

#[wasm_bindgen(module = "/assets/emu_bridge.js")]
extern "C" {
    fn createEmulator(hex_text: &str);
    fn isEmulatorLoaded() -> bool;
    fn sendGcode(text: &str);
    fn cyclesPerSecond() -> f64;
    fn currentCycles() -> f64;
    fn hotendCelsius() -> f64;
    fn bedCelsius() -> f64;
    fn drainUartText() -> String;
    fn tickEmulator(cycle_budget: f64);
    fn drainStepEvents() -> Vec<f64>;
}

const MAX_UART_LOG_CHARS: usize = 20_000;
/// Extrusion deltas below this are noise, not real deposition -- mirrors
/// gcode-sim's own epsilon so both backends agree on what "extruding" means.
const EXTRUDE_EPSILON: f32 = 1e-4;

#[derive(Resource, Default)]
pub struct FirmwareState {
    pub loaded: bool,
    pub playing: bool,
    last_e: f32,
    pub hotend_c: f32,
    pub bed_c: f32,
    pub uart_log: String,
}

impl FirmwareState {
    pub fn load_hex(&mut self, hex_text: &str, print_state: &mut PrintState) {
        createEmulator(hex_text);
        self.loaded = true;
        self.playing = true;
        self.last_e = 0.0;
        self.hotend_c = 0.0;
        self.bed_c = 0.0;
        self.uart_log.clear();

        print_state.toolpath.clear();
        print_state.source_lines.clear();
        print_state.thermal.clear();
        print_state.toolpath.push(MotionEvent {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            extruding: false,
            line: 0,
        });
        print_state.time = 0.0;
        print_state.total_time = 0.0;
        print_state.live = true;
        print_state.playing = true;
        print_state.generation += 1;
        print_state.loaded_file_name = "Marlin (RAMPS 1.4, live)".to_string();
    }

    pub fn send_gcode(&self, text: &str) {
        if self.loaded {
            sendGcode(text);
        }
    }

    /// Streams imported gcode bytes into the live emulator's UART -- the
    /// Firmware-mode counterpart of `loader::load_import_bytes`, shared by
    /// BROWSE and drag & drop. Raises an alert instead of silently dropping
    /// the file when no firmware is loaded yet or the bytes don't decode.
    pub fn stream_import(&self, file_name: &str, bytes: &[u8], alerts: &mut crate::ui::AlertState) {
        if !self.loaded {
            let err = format!("{file_name}: load firmware before importing");
            warn!("{err}");
            alerts.raise(err, 4.0);
            return;
        }
        match crate::loader::decode_gcode_bytes(file_name, bytes) {
            Ok(text) => self.send_gcode(&text),
            Err(err) => {
                warn!("{err}");
                alerts.raise(err, 4.0);
            }
        }
    }
}

/// Keeps `PrintState`'s scheduling flags in lockstep with the active backend
/// every frame, independent of whether the side panel is drawn (it can be
/// hidden with Tab). In Firmware mode with a live session, `live` must stay
/// armed (so `advance_time` stays disabled instead of fighting
/// `drive_firmware` over `time`) and `playing` mirrors the real emulator flag
/// (so the codec header's blink/alert logic reflects reality). In Firmware
/// mode with nothing loaded, a leftover Simulation print is parked -- its
/// PLAYBACK controls are hidden, so it must not keep running uncontrollably.
pub fn sync_backend_playback(
    ui_state: Res<crate::ui::UiState>,
    firmware: Res<FirmwareState>,
    mut state: ResMut<PrintState>,
) {
    if ui_state.backend == crate::ui::Backend::Firmware {
        if firmware.loaded {
            state.live = true;
            state.playing = firmware.playing;
        } else if state.playing {
            state.playing = false;
        }
    } else {
        state.live = false;
    }
}

/// Runs once per frame while the firmware backend is active: advances the
/// emulator by one frame's worth of CPU cycles (at real-time pace) and
/// appends whatever it did to `PrintState.toolpath`.
pub fn drive_firmware(
    time: Res<Time>,
    ui_state: Res<crate::ui::UiState>,
    mut firmware: ResMut<FirmwareState>,
    mut print_state: ResMut<PrintState>,
) {
    if ui_state.backend != crate::ui::Backend::Firmware
        || !firmware.loaded
        || !firmware.playing
        || !isEmulatorLoaded()
    {
        return;
    }

    let cycle_budget = cyclesPerSecond() * time.delta_secs_f64().max(0.0);
    tickEmulator(cycle_budget);

    let flat = drainStepEvents();
    let cycles_per_second = cyclesPerSecond();
    for chunk in flat.chunks_exact(5) {
        let [cycle, x, y, z, e] = [chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]];
        let t = cycle / cycles_per_second;
        let e = e as f32;
        let extruding = e - firmware.last_e > EXTRUDE_EPSILON;
        firmware.last_e = e;
        print_state.toolpath.push(MotionEvent {
            t,
            x: x as f32,
            y: y as f32,
            z: z as f32,
            e,
            extruding,
            line: 0,
        });
    }

    // Sync unconditionally, not just when a step event happened -- long
    // heating waits (M109/M190) produce no motion at all, and the display
    // would otherwise freeze at the last motion's timestamp.
    let now = currentCycles() / cycles_per_second;
    print_state.time = now;
    print_state.total_time = now;

    firmware.hotend_c = hotendCelsius() as f32;
    firmware.bed_c = bedCelsius() as f32;

    // Feed the thermal panel with live emulator temps (targets unknown -> 0).
    let stale = print_state
        .thermal
        .last()
        .is_none_or(|sample| now - sample.t >= 0.5);
    if stale {
        print_state.thermal.push(motion::ThermalSample {
            t: now,
            hotend_c: firmware.hotend_c,
            hotend_target: 0.0,
            bed_c: firmware.bed_c,
            bed_target: 0.0,
        });
    }

    let new_uart = drainUartText();
    if !new_uart.is_empty() {
        firmware.uart_log.push_str(&new_uart);
        if firmware.uart_log.len() > MAX_UART_LOG_CHARS {
            let cut = firmware.uart_log.len() - MAX_UART_LOG_CHARS;
            firmware.uart_log.replace_range(0..cut, "");
        }
    }
}
