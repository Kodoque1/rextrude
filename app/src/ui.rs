#[cfg(not(target_arch = "wasm32"))]
use bevy::log::warn;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::layers::LayerVisuals;
use crate::loader::load_gcode_text;
use crate::playback::PrintState;

#[cfg(target_arch = "wasm32")]
use crate::firmware::FirmwareState;

/// Gcode files embedded directly in the binary so the demo works with zero
/// setup on both native and wasm builds, regardless of whether drag & drop
/// or a native file dialog is available.
pub const EXAMPLES: &[(&str, &str)] = &[
    (
        "calibration_cube.gcode",
        include_str!("../../assets/gcode/calibration_cube.gcode"),
    ),
    (
        "hollow_tower.gcode",
        include_str!("../../assets/gcode/hollow_tower.gcode"),
    ),
];

#[cfg(target_arch = "wasm32")]
const MARLIN_RAMPS14_HEX: &str = include_str!("../../assets/firmware/marlin_ramps14.hex");

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum Backend {
    #[default]
    Gcode,
    #[cfg(target_arch = "wasm32")]
    Firmware,
}

#[derive(Resource, Default)]
pub struct UiState {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub native_path: String,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub backend: Backend,
}

pub fn playback_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<PrintState>,
    mut ui_state: ResMut<UiState>,
    layer_visuals: Res<LayerVisuals>,
    #[cfg(target_arch = "wasm32")] mut firmware: ResMut<FirmwareState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Only read from the native-only branch below; touch it unconditionally
    // so wasm builds (which skip that branch) don't warn on an unused param.
    let _ = &mut ui_state;

    egui::Window::new("3D Printer Simulator")
        .default_width(320.0)
        .show(ctx, |ui| {
            #[cfg(target_arch = "wasm32")]
            {
                ui.heading("Backend");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut ui_state.backend, Backend::Gcode, "Simulation (gcode)");
                    ui.selectable_value(
                        &mut ui_state.backend,
                        Backend::Firmware,
                        "Firmware emulation (Marlin)",
                    );
                });
                ui.separator();
            }

            #[cfg(target_arch = "wasm32")]
            if ui_state.backend == Backend::Firmware {
                firmware_ui(ui, &mut firmware, &mut state);
                return;
            }

            ui.heading("Import");
            ui.horizontal_wrapped(|ui| {
                for &(name, contents) in EXAMPLES {
                    if ui.button(name).clicked() {
                        load_gcode_text(&mut state, name.to_string(), contents);
                    }
                }
            });

            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                ui.label("Load from path (native):");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut ui_state.native_path);
                    if ui.button("Load").clicked() {
                        match std::fs::read_to_string(&ui_state.native_path) {
                            Ok(contents) => {
                                let name = ui_state.native_path.clone();
                                load_gcode_text(&mut state, name, &contents);
                            }
                            Err(err) => {
                                warn!("failed to read {}: {err}", ui_state.native_path);
                            }
                        }
                    }
                });
            }
            ui.label("(or drag & drop a .gcode file onto the window)");

            ui.separator();

            if state.toolpath.is_empty() {
                ui.label("No toolpath loaded yet.");
                return;
            }

            ui.heading("Playback");
            ui.label(format!("File: {}", state.loaded_file_name));
            ui.horizontal(|ui| {
                let label = if state.playing { "Pause" } else { "Play" };
                if ui.button(label).clicked() {
                    state.playing = !state.playing;
                }
                if ui.button("Restart").clicked() {
                    state.time = 0.0;
                    state.playing = true;
                }
            });

            ui.add(
                egui::Slider::new(&mut state.speed, 0.1..=20.0)
                    .text("Speed")
                    .logarithmic(true),
            );

            let mut time = state.time;
            if ui
                .add(egui::Slider::new(&mut time, 0.0..=state.total_time.max(0.001)).text("Time (s)"))
                .changed()
            {
                state.time = time;
            }

            let layer_count = layer_visuals.layer_count();
            if layer_count > 0 {
                let idx = state.current_index();
                let current = layer_visuals.layer_containing(idx).unwrap_or(0);
                ui.label(format!("Layer {} / {}", current + 1, layer_count));
            }

            ui.label(format!(
                "t = {:.1}s / {:.1}s",
                state.time, state.total_time
            ));
        });

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn firmware_ui(ui: &mut egui::Ui, firmware: &mut FirmwareState, state: &mut PrintState) {
    ui.heading("Firmware");

    if !firmware.loaded {
        ui.label("No firmware loaded yet.");
        if ui.button("Load Marlin firmware (RAMPS 1.4)").clicked() {
            firmware.load_hex(MARLIN_RAMPS14_HEX, state);
        }
        return;
    }

    ui.horizontal(|ui| {
        let label = if firmware.playing { "Pause" } else { "Resume" };
        if ui.button(label).clicked() {
            firmware.playing = !firmware.playing;
        }
        ui.label(format!("Hotend: {:.1}C   Bed: {:.1}C", firmware.hotend_c, firmware.bed_c));
    });

    ui.separator();
    ui.heading("Send gcode");
    ui.horizontal_wrapped(|ui| {
        for &(name, contents) in EXAMPLES {
            if ui.button(name).clicked() {
                firmware.send_gcode(contents);
            }
        }
    });

    ui.separator();
    ui.label(format!("t = {:.1}s (live)", state.time));

    ui.separator();
    ui.heading("Serial console");
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut firmware.uart_log)
                    .desired_width(f32::INFINITY)
                    .interactive(false)
                    .font(egui::TextStyle::Monospace),
            );
        });
}
