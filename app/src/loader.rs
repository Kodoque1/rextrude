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

/// Hard cap on an imported file's raw byte size, checked before any parsing
/// or decompression runs. This is the common choke point for every import
/// path (BROWSE, drag & drop, autoload, firmware-mode send), so it also
/// bounds the input to the `.bgcode` decompressor against decompression-bomb
/// style inputs -- a well-formed print file is nowhere close to this size.
const MAX_IMPORT_BYTES: usize = 256 * 1024 * 1024;

fn enforce_import_size(file_name: &str, len: usize, limit: usize) -> Result<(), String> {
    if len > limit {
        return Err(format!(
            "{file_name}: file too large ({len} bytes, limit {limit})"
        ));
    }
    Ok(())
}

/// Decodes raw imported bytes to gcode text: `.bgcode` via the binary
/// decoder, everything else validated as UTF-8 gcode text. Borrows on the
/// (common) plain-UTF-8 path; only bgcode decoding allocates.
pub fn decode_gcode_bytes<'a>(
    file_name: &str,
    bytes: &'a [u8],
) -> Result<std::borrow::Cow<'a, str>, String> {
    enforce_import_size(file_name, bytes.len(), MAX_IMPORT_BYTES)?;
    if crate::bgcode::is_bgcode(file_name, bytes) {
        return crate::bgcode::decode(bytes).map(std::borrow::Cow::Owned);
    }
    std::str::from_utf8(bytes)
        .map(std::borrow::Cow::Borrowed)
        .map_err(|_| format!("{file_name}: not UTF-8 gcode and not bgcode"))
}

/// Single entry point for every import path (BROWSE, drag & drop, autoload):
/// decodes `.bgcode` to ASCII first, then reuses the plain-text pipeline.
pub fn load_import_bytes(
    state: &mut PrintState,
    file_name: String,
    bytes: &[u8],
) -> Result<(), String> {
    let text = decode_gcode_bytes(&file_name, bytes)?;
    load_gcode_text(state, file_name, &text);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_size_cap_rejects_only_over_the_limit() {
        assert!(enforce_import_size("f.gcode", 1000, 1000).is_ok());
        assert!(enforce_import_size("f.gcode", 1001, 1000).is_err());
    }

    #[test]
    fn plain_gcode_under_the_cap_decodes_as_utf8() {
        let bytes = b"G28\nG1 X10\n";
        let text = decode_gcode_bytes("f.gcode", bytes).unwrap();
        assert_eq!(&*text, "G28\nG1 X10\n");
    }

    #[test]
    fn invalid_utf8_under_the_cap_is_rejected_as_not_gcode() {
        let bytes = [0xFFu8, 0xFE, 0xFD];
        let err = decode_gcode_bytes("f.gcode", &bytes).unwrap_err();
        assert!(err.contains("not UTF-8"), "unexpected error: {err}");
    }
}
