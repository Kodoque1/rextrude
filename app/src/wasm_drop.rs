//! Browser drag & drop for gcode files. Winit's web backend doesn't surface
//! `FileDragAndDrop`, so this listens on the document directly and stashes
//! the dropped file's contents in a thread-local for a Bevy system to pick
//! up on the next frame. Sound because wasm here is single-threaded: Bevy's
//! app and the JS callbacks both run on the browser's one JS thread.
use std::cell::RefCell;

use bevy::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, FileReader, ProgressEvent};

use crate::playback::PrintState;
use crate::ui::AlertState;

thread_local! {
    static DROPPED_FILE: RefCell<Option<(String, Vec<u8>)>> = const { RefCell::new(None) };
}

pub fn install_drop_listener() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };

    let dragover = Closure::<dyn FnMut(DragEvent)>::new(move |event: DragEvent| {
        event.prevent_default();
    });
    let _ =
        document.add_event_listener_with_callback("dragover", dragover.as_ref().unchecked_ref());
    dragover.forget();

    let drop = Closure::<dyn FnMut(DragEvent)>::new(move |event: DragEvent| {
        event.prevent_default();
        let Some(data_transfer) = event.data_transfer() else {
            return;
        };
        let Some(files) = data_transfer.files() else {
            return;
        };
        let Some(file) = files.get(0) else {
            return;
        };
        let name = file.name();

        let reader = FileReader::new().expect("FileReader is supported");
        let reader_clone = reader.clone();
        let onload = Closure::<dyn FnMut(ProgressEvent)>::new(move |_event: ProgressEvent| {
            if let Ok(buf) = reader_clone.result() {
                let bytes = web_sys::js_sys::Uint8Array::new(&buf).to_vec();
                DROPPED_FILE.with(|slot| *slot.borrow_mut() = Some((name.clone(), bytes)));
            }
        });
        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        onload.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    let _ = document.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
    drop.forget();
}

pub fn poll_dropped_file(
    mut state: ResMut<PrintState>,
    mut alerts: ResMut<AlertState>,
    ui_state: Res<crate::ui::UiState>,
    firmware: Res<crate::firmware::FirmwareState>,
) {
    let dropped = DROPPED_FILE.with(|slot| slot.borrow_mut().take());
    let Some((name, bytes)) = dropped else {
        return;
    };

    // In Firmware mode, a dropped file streams into the live emulator's UART
    // instead of the gcode-sim pipeline `load_import_bytes` drives -- same
    // dispatch as the BROWSE button in `file_picker::poll_file_pick`.
    if ui_state.backend == crate::ui::Backend::Firmware {
        match crate::loader::decode_gcode_bytes(&name, &bytes) {
            Ok(text) => firmware.send_gcode(&text),
            Err(err) => {
                warn!("{err}");
                alerts.raise(err, 4.0);
            }
        }
        return;
    }

    if let Err(err) = crate::loader::load_import_bytes(&mut state, name, &bytes) {
        warn!("{err}");
        alerts.raise(err, 4.0);
    }
}
