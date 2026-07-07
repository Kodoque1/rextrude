use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use gcode_sim::{simulate_full, PrinterConfig};

use crate::playback::PrintState;
use crate::ui::AlertState;

pub fn load_gcode_text(state: &mut PrintState, file_name: String, gcode: &str) {
    let sim = simulate_full(gcode, &PrinterConfig::default());
    let source_lines = gcode.lines().map(str::to_string).collect();
    state.load(file_name, sim.toolpath, source_lines, sim.thermal);
}

/// Single entry point for every import path (BROWSE, drag & drop, autoload):
/// decodes `.bgcode` to ASCII first, then reuses the plain-text pipeline.
pub fn load_import_bytes(
    state: &mut PrintState,
    file_name: String,
    bytes: &[u8],
) -> Result<(), String> {
    if crate::bgcode::is_bgcode(&file_name, bytes) {
        let text = crate::bgcode::decode(bytes)?;
        load_gcode_text(state, file_name, &text);
        return Ok(());
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            load_gcode_text(state, file_name, text);
            Ok(())
        }
        Err(_) => Err(format!("{file_name}: not UTF-8 gcode and not bgcode")),
    }
}

/// Dev aid (native only): `SIM_AUTOLOAD=<path|example name>` loads a gcode
/// file at startup so the app can be exercised without clicking the UI.
/// Bare names resolve against the embedded examples (see `ui::EXAMPLES`).
#[cfg(not(target_arch = "wasm32"))]
pub fn autoload_from_env(mut state: ResMut<PrintState>) {
    let Ok(target) = std::env::var("SIM_AUTOLOAD") else {
        return;
    };
    if let Some(&(name, contents)) = crate::ui::EXAMPLES
        .iter()
        .find(|(name, _)| *name == target || target == "1")
    {
        load_gcode_text(&mut state, name.to_string(), contents);
        return;
    }
    match std::fs::read(&target) {
        Ok(bytes) => {
            if let Err(err) = load_import_bytes(&mut state, target.clone(), &bytes) {
                warn!("SIM_AUTOLOAD: {err}");
            }
        }
        Err(err) => warn!("SIM_AUTOLOAD: failed to read {target}: {err}"),
    }
}

/// Native: dropping a file onto the window loads it via `bevy::window::FileDragAndDrop`.
/// (The wasm build wires up the equivalent via `wasm_drop`, since winit's web
/// backend doesn't surface that event.)
#[cfg(not(target_arch = "wasm32"))]
pub fn handle_file_drop(
    mut events: MessageReader<FileDragAndDrop>,
    mut state: ResMut<PrintState>,
    mut alerts: ResMut<AlertState>,
) {
    for ev in events.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = ev {
            match std::fs::read(path_buf) {
                Ok(bytes) => {
                    let name = path_buf
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "gcode".to_string());
                    if let Err(err) = load_import_bytes(&mut state, name, &bytes) {
                        warn!("{err}");
                        alerts.raise(err, 4.0);
                    }
                }
                Err(err) => {
                    warn!("failed to read dropped file {:?}: {err}", path_buf);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn handle_file_drop(
    _events: MessageReader<FileDragAndDrop>,
    _state: ResMut<PrintState>,
    _alerts: ResMut<AlertState>,
) {
}
