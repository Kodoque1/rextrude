use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use gcode_sim::{simulate, PrinterConfig};

use crate::playback::PrintState;

pub fn load_gcode_text(state: &mut PrintState, file_name: String, gcode: &str) {
    let toolpath = simulate(gcode, &PrinterConfig::default());
    state.load(file_name, toolpath);
}

/// Native: dropping a file onto the window loads it via `bevy::window::FileDragAndDrop`.
/// (The wasm build wires up the equivalent via `wasm_drop`, since winit's web
/// backend doesn't surface that event.)
#[cfg(not(target_arch = "wasm32"))]
pub fn handle_file_drop(mut events: MessageReader<FileDragAndDrop>, mut state: ResMut<PrintState>) {
    for ev in events.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = ev {
            match std::fs::read_to_string(path_buf) {
                Ok(contents) => {
                    let name = path_buf
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "gcode".to_string());
                    load_gcode_text(&mut state, name, &contents);
                }
                Err(err) => {
                    warn!("failed to read dropped file {:?}: {err}", path_buf);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn handle_file_drop(_events: MessageReader<FileDragAndDrop>, _state: ResMut<PrintState>) {}
