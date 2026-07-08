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
pub struct PendingGcodePick(Option<PickInFlight>);

struct PickInFlight {
    task: Task<Option<(String, Vec<u8>)>>,
    /// Destination snapshotted when the dialog was opened: `true` streams
    /// the file into the firmware emulator's UART, `false` loads the
    /// gcode-sim pipeline. Decided at spawn time so switching backends while
    /// the (non-page-blocking) dialog is open can't re-route the file.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    to_firmware: bool,
}

/// Opens the native (or, on wasm32, browser) file picker restricted to
/// gcode files. Call from the BROWSE button handler; `to_firmware` says
/// where the picked file should go (see [`PickInFlight::to_firmware`]).
pub fn spawn_file_pick(pending: &mut PendingGcodePick, to_firmware: bool) {
    if pending.0.is_some() {
        return; // ignore repeat clicks while a dialog is already open
    }
    let task = IoTaskPool::get().spawn(async move {
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
    });
    pending.0 = Some(PickInFlight { task, to_firmware });
}

/// Drains a finished file-pick task each frame and loads its result.
/// Registered unconditionally in `Update` on both targets.
pub fn poll_file_pick(
    mut pending: ResMut<PendingGcodePick>,
    mut state: ResMut<PrintState>,
    mut alerts: ResMut<AlertState>,
    #[cfg(target_arch = "wasm32")] firmware: Res<crate::firmware::FirmwareState>,
) {
    let Some(pick) = pending.0.as_mut() else {
        return;
    };
    let Some(result) = block_on(poll_once(&mut pick.task)) else {
        return; // still picking / still reading
    };
    #[cfg(target_arch = "wasm32")]
    let to_firmware = pick.to_firmware;
    pending.0 = None;
    let Some((name, bytes)) = result else {
        return; // user cancelled the dialog
    };

    // A BROWSE launched from Firmware mode streams the file into the live
    // emulator's UART instead of the gcode-sim pipeline `load_import_bytes`
    // drives.
    #[cfg(target_arch = "wasm32")]
    if to_firmware {
        firmware.stream_import(&name, &bytes, &mut alerts);
        return;
    }

    if let Err(err) = load_import_bytes(&mut state, name, &bytes) {
        warn!("BROWSE: {err}");
        alerts.raise(err, 4.0);
    }
}
