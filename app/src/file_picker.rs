//! Cross-platform (native + wasm32) "BROWSE" file picker for .gcode files.
//! Built on `rfd::AsyncFileDialog` + Bevy's `IoTaskPool`. The same
//! `IoTaskPool::spawn` call works unchanged on both targets: bevy_tasks
//! dispatches to a cooperative single-threaded executor natively (ticked
//! every frame by `bevy_app::TaskPoolPlugin`, already part of
//! `DefaultPlugins`) and to `wasm_bindgen_futures::spawn_local` on wasm32
//! internally. No manual thread-local/Arc<Mutex> bridge is needed here,
//! unlike the wasm-only drag-and-drop glue in `wasm_drop.rs`.

use bevy::log::warn;
use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, IoTaskPool, Task};

use crate::loader::load_import_bytes;
use crate::playback::PrintState;
use crate::ui::AlertState;

/// The in-flight "user is picking a file" task, if any.
#[derive(Resource, Default)]
pub struct PendingGcodePick(Option<Task<Option<(String, Vec<u8>)>>>);

/// Opens the native (or, on wasm32, browser) file picker restricted to
/// gcode files. Call from the BROWSE button handler.
pub fn spawn_file_pick(pending: &mut PendingGcodePick) {
    if pending.0.is_some() {
        return; // ignore repeat clicks while a dialog is already open
    }
    pending.0 = Some(IoTaskPool::get().spawn(async move {
        let Some(handle) = rfd::AsyncFileDialog::new()
            .add_filter("gcode", &["gcode", "gco", "bgcode"])
            .set_title("Import G-code")
            .pick_file()
            .await
        else {
            return None; // user cancelled
        };
        let name = handle.file_name();
        let bytes = handle.read().await;
        Some((name, bytes))
    }));
}

/// Drains a finished file-pick task each frame and loads its result.
/// Registered unconditionally in `Update` on both targets.
pub fn poll_file_pick(
    mut pending: ResMut<PendingGcodePick>,
    mut state: ResMut<PrintState>,
    mut alerts: ResMut<AlertState>,
) {
    let Some(task) = pending.0.as_mut() else {
        return;
    };
    let Some(result) = block_on(poll_once(task)) else {
        return; // still picking / still reading
    };
    pending.0 = None;
    let Some((name, bytes)) = result else {
        return; // user cancelled the dialog
    };
    if let Err(err) = load_import_bytes(&mut state, name, &bytes) {
        warn!("BROWSE: {err}");
        alerts.raise(err, 4.0);
    }
}
