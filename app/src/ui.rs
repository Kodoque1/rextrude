use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::audio::SfxEvent;
use crate::layers::LayerVisuals;
use crate::loader::load_gcode_text;
use crate::playback::PrintState;
use crate::theme;

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

/// Where a data section lives: docked in the side panel or popped out into
/// its own movable window.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Docked,
    Floating,
}

#[derive(Resource)]
pub struct UiState {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub backend: Backend,
    pub show_panel: bool,
    pub crt: bool,
    pub dro: Placement,
    pub progress: Placement,
    pub gcode_stream: Placement,
    pub thermal: Placement,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            show_panel: true,
            crt: true,
            dro: Placement::default(),
            progress: Placement::default(),
            gcode_stream: Placement::default(),
            thermal: Placement::default(),
        }
    }
}

/// Whether the pointer is over the docked panel, header, or a popped-out
/// window this frame, so other systems (e.g. camera scroll-to-zoom) can
/// avoid fighting egui for wheel/drag input. `egui::Context::wants_pointer_input`
/// can't be used for this: it only recognizes `egui::Area`/`Window` (which
/// register in egui's layer registry), not the `egui::Panel`s this UI is
/// built from, so it never reports true while hovering the docked panel.
#[derive(Resource, Default)]
pub struct PointerOverUi(pub bool);

/// Codec-style "!" notification: message + seconds left on screen.
#[derive(Resource, Default)]
pub struct AlertState {
    pub current: Option<(String, f32)>,
}

impl AlertState {
    pub fn raise(&mut self, message: impl Into<String>, seconds: f32) {
        self.current = Some((message.into(), seconds));
    }
}

/// Raises alerts on toolpath load and print completion, and expires them.
pub fn update_alerts(
    time: Res<Time>,
    state: Res<PrintState>,
    mut alerts: ResMut<AlertState>,
    mut sfx: MessageWriter<SfxEvent>,
    mut last_generation: Local<u64>,
    mut was_playing: Local<bool>,
) {
    if state.generation != *last_generation {
        *last_generation = state.generation;
        if !state.toolpath.is_empty() {
            alerts.raise(
                format!("DATA RECEIVED: {}", state.loaded_file_name.to_uppercase()),
                3.5,
            );
            sfx.write(SfxEvent::Alert);
        }
    }
    if *was_playing && !state.playing && state.total_time > 0.0 && state.time >= state.total_time {
        alerts.raise("PRINT COMPLETE", 2.6);
        sfx.write(SfxEvent::CodecCall);
    }
    *was_playing = state.playing;

    if let Some((_, remaining)) = &mut alerts.current {
        *remaining -= time.delta_secs();
        if *remaining <= 0.0 {
            alerts.current = None;
        }
    }
}

/// Tab hides/shows the side panel, like dismissing the codec screen.
pub fn keyboard_toggles(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut sfx: MessageWriter<SfxEvent>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        ui_state.show_panel = !ui_state.show_panel;
        sfx.write(SfxEvent::Click);
    }
}

fn codec_header(
    root: &mut egui::Ui,
    state: &PrintState,
    ui_state: &mut UiState,
    sfx: &mut MessageWriter<SfxEvent>,
) {
    egui::Panel::top("codec_header")
        .frame(
            egui::Frame::new()
                .fill(theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12, 6)),
        )
        .show(root, |ui| {
            let ctx = ui.ctx().clone();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("140.85").size(30.0).color(theme::TEXT));
                ui.label(egui::RichText::new("‹ ›").size(22.0).color(theme::TEXT_DIM));

                ui.separator();

                let blink = !state.playing || ctx.input(|i| i.time) % 1.0 < 0.65;
                let title = if state.toolpath.is_empty() {
                    "STANDBY".to_string()
                } else {
                    state.loaded_file_name.to_uppercase()
                };
                ui.label(
                    egui::RichText::new(if blink {
                        "▶ PRINT OPS"
                    } else {
                        "  PRINT OPS"
                    })
                    .size(24.0)
                    .color(theme::TEXT),
                );
                ui.label(egui::RichText::new(title).size(20.0).color(theme::TEXT_DIM));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if ui_state.show_panel {
                        "PANEL ▸"
                    } else {
                        "◂ PANEL"
                    };
                    if ui.button(label).on_hover_text("Tab").clicked() {
                        ui_state.show_panel = !ui_state.show_panel;
                        sfx.write(SfxEvent::Click);
                    }
                    if !state.toolpath.is_empty() {
                        ui.label(
                            egui::RichText::new(format!(
                                "t = {:>6.1}s / {:.1}s",
                                state.time, state.total_time
                            ))
                            .size(20.0)
                            .color(theme::TEXT),
                        );
                    }
                });
            });
        });
}

fn alert_overlay(ctx: &egui::Context, alerts: &AlertState) {
    let Some((message, remaining)) = &alerts.current else {
        return;
    };
    // Hard blink for the first half second, like the codec call sting.
    if *remaining > 1.6 && ctx.input(|i| i.time * 6.0) as i64 % 2 == 0 {
        return;
    }
    egui::Area::new(egui::Id::new("codec_alert"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::BG_PANEL)
                .stroke(egui::Stroke::new(2.0, theme::ALERT_RED))
                .inner_margin(egui::Margin::symmetric(18, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("!")
                                .size(40.0)
                                .strong()
                                .color(theme::ALERT_RED),
                        );
                        ui.label(egui::RichText::new(message).size(22.0).color(theme::TEXT));
                    });
                });
        });
}

fn import_section(
    ui: &mut egui::Ui,
    state: &mut PrintState,
    pending_pick: &mut crate::file_picker::PendingGcodePick,
    sfx: &mut MessageWriter<SfxEvent>,
) {
    ui.horizontal_wrapped(|ui| {
        for &(name, contents) in EXAMPLES {
            if ui.button(name).clicked() {
                load_gcode_text(state, name.to_string(), contents);
                sfx.write(SfxEvent::Beep);
            }
        }
        if ui.button("BROWSE").clicked() {
            crate::file_picker::spawn_file_pick(pending_pick, false);
            sfx.write(SfxEvent::Beep);
        }
    });

    ui.label(
        egui::RichText::new("(or drag & drop a .gcode / .bgcode file)")
            .small()
            .color(theme::TEXT_DIM),
    );
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("NEED GCODE?")
                .small()
                .color(theme::TEXT_DIM),
        );
        ui.hyperlink_to(
            egui::RichText::new("printables.com pre-sliced files").small(),
            "https://www.printables.com/tag/gcode",
        )
        .on_hover_text("Search Printables for ready-made .gcode / .bgcode files");
    });
}

fn playback_section(
    ui: &mut egui::Ui,
    state: &mut PrintState,
    layer_visuals: &LayerVisuals,
    sfx: &mut MessageWriter<SfxEvent>,
) {
    if state.toolpath.is_empty() {
        ui.label(egui::RichText::new("NO TOOLPATH LOADED").color(theme::TEXT_DIM));
        return;
    }

    ui.horizontal(|ui| {
        let label = if state.playing {
            "|| PAUSE"
        } else {
            "▶ PLAY"
        };
        if ui.button(label).clicked() {
            state.playing = !state.playing;
            sfx.write(SfxEvent::Click);
        }
        if ui.button("↺ RESTART").clicked() {
            state.time = 0.0;
            state.playing = true;
            sfx.write(SfxEvent::Click);
        }
    });

    ui.add(
        egui::Slider::new(&mut state.speed, 0.1..=20.0)
            .text("SPEED")
            .logarithmic(true),
    );

    let mut time = state.time;
    if ui
        .add(egui::Slider::new(&mut time, 0.0..=state.total_time.max(0.001)).text("TIME (S)"))
        .changed()
    {
        state.time = time;
    }

    layer_readout(ui, state, layer_visuals);
}

fn layer_readout(ui: &mut egui::Ui, state: &PrintState, layer_visuals: &LayerVisuals) {
    let layer_count = layer_visuals.layer_count();
    if layer_count > 0 {
        let idx = state.current_index();
        let current = layer_visuals.layer_containing(idx).unwrap_or(0);
        ui.label(format!("LAYER {:02} / {:02}", current + 1, layer_count));
    }
}

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).size(21.0).color(theme::TEXT))
        .default_open(true)
        .show(ui, add_contents);
    ui.add_space(4.0);
}

/// A data section that can be popped out of the side panel into its own
/// movable window. Renders the docked variant (with a POP OUT button);
/// the floating variant is drawn by [`floating_section`] at context level.
fn poppable_section(
    ui: &mut egui::Ui,
    title: &str,
    placement: &mut Placement,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    if *placement == Placement::Floating {
        return;
    }
    egui::CollapsingHeader::new(egui::RichText::new(title).size(21.0).color(theme::TEXT))
        .default_open(true)
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui
                    .small_button(egui::RichText::new("POP OUT").small())
                    .clicked()
                {
                    *placement = Placement::Floating;
                }
            });
            add_contents(ui);
        });
    ui.add_space(4.0);
}

/// The floating window twin of [`poppable_section`]; closing re-docks it.
fn floating_section(
    ctx: &egui::Context,
    title: &str,
    placement: &mut Placement,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<egui::Rect> {
    if *placement != Placement::Floating {
        return None;
    }
    let mut open = true;
    let response = egui::Window::new(egui::RichText::new(title).size(20.0).color(theme::TEXT))
        .open(&mut open)
        .resizable(true)
        .default_width(300.0)
        .show(ctx, |ui| add_contents(ui));
    if !open {
        *placement = Placement::Docked;
    }
    response.map(|r| r.response.rect)
}

pub fn playback_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<PrintState>,
    mut ui_state: ResMut<UiState>,
    mut pending_pick: ResMut<crate::file_picker::PendingGcodePick>,
    layer_visuals: Res<LayerVisuals>,
    alerts: Res<AlertState>,
    velocity: Res<crate::kinematics::HeadVelocity>,
    mut sfx: MessageWriter<SfxEvent>,
    mut theme_applied: Local<bool>,
    mut pointer_over_ui: ResMut<PointerOverUi>,
    #[cfg(target_arch = "wasm32")] mut firmware: ResMut<FirmwareState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*theme_applied {
        theme::apply(ctx);
        *theme_applied = true;
    }

    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("codec_root"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    codec_header(&mut root, &state, &mut ui_state, &mut sfx);

    if ui_state.show_panel {
        egui::Panel::right("codec_panel")
            .exact_size(330.0)
            .resizable(false)
            .show(&mut root, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(6.0);

                        #[cfg(target_arch = "wasm32")]
                        let firmware_active = ui_state.backend == Backend::Firmware;
                        #[cfg(not(target_arch = "wasm32"))]
                        let firmware_active = false;

                        #[cfg(target_arch = "wasm32")]
                        {
                            section(ui, "BACKEND", |ui| {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(
                                        &mut ui_state.backend,
                                        Backend::Gcode,
                                        "SIMULATION",
                                    );
                                    ui.selectable_value(
                                        &mut ui_state.backend,
                                        Backend::Firmware,
                                        "FIRMWARE (MARLIN)",
                                    );
                                });
                            });
                            // (live/playing bookkeeping happens in
                            // `firmware::sync_backend_playback`, which runs
                            // even while this panel is hidden.)
                            if firmware_active {
                                firmware_ui(
                                    ui,
                                    &mut firmware,
                                    &mut state,
                                    &mut pending_pick,
                                    &layer_visuals,
                                    &mut sfx,
                                );
                            }
                        }

                        if !firmware_active {
                            section(ui, "IMPORT", |ui| {
                                import_section(ui, &mut state, &mut pending_pick, &mut sfx);
                            });
                            section(ui, "PLAYBACK", |ui| {
                                playback_section(ui, &mut state, &layer_visuals, &mut sfx);
                            });
                        }
                        poppable_section(ui, "MOTION DRO", &mut ui_state.dro, |ui| {
                            crate::panels::dro::show(ui, &state, &velocity);
                        });
                        poppable_section(ui, "PROGRESS", &mut ui_state.progress, |ui| {
                            crate::panels::progress::show(ui, &state, &layer_visuals);
                        });
                        poppable_section(ui, "THERMAL", &mut ui_state.thermal, |ui| {
                            crate::panels::thermal::show(ui, &state);
                        });
                        poppable_section(ui, "G-CODE STREAM", &mut ui_state.gcode_stream, |ui| {
                            crate::panels::gcode_stream::show(ui, &state);
                        });
                        section(ui, "SYSTEM", |ui| {
                            ui.checkbox(&mut ui_state.crt, "CRT SCANLINES");
                        });
                    });
            });
    }

    // Everything above allocates out of `root` (header + docked panel), so
    // whatever's left is the passthrough game-view rect. Floating windows
    // don't shrink `root`, so their rects are tracked separately below.
    let viewport_rect = root.available_rect_before_wrap();

    let dro_rect = floating_section(ctx, "MOTION DRO", &mut ui_state.dro, |ui| {
        crate::panels::dro::show(ui, &state, &velocity);
    });
    let progress_rect = floating_section(ctx, "PROGRESS", &mut ui_state.progress, |ui| {
        crate::panels::progress::show(ui, &state, &layer_visuals);
    });
    let gcode_rect = floating_section(ctx, "G-CODE STREAM", &mut ui_state.gcode_stream, |ui| {
        crate::panels::gcode_stream::show(ui, &state);
    });
    let thermal_rect = floating_section(ctx, "THERMAL", &mut ui_state.thermal, |ui| {
        crate::panels::thermal::show(ui, &state);
    });

    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
    pointer_over_ui.0 = pointer_pos.is_some_and(|pos| {
        !viewport_rect.contains(pos)
            || [dro_rect, progress_rect, gcode_rect, thermal_rect]
                .into_iter()
                .flatten()
                .any(|r| r.contains(pos))
    });

    alert_overlay(ctx, &alerts);

    if ui_state.crt {
        theme::draw_scanlines(ctx);
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn firmware_ui(
    ui: &mut egui::Ui,
    firmware: &mut FirmwareState,
    state: &mut PrintState,
    pending_pick: &mut crate::file_picker::PendingGcodePick,
    layer_visuals: &LayerVisuals,
    sfx: &mut MessageWriter<SfxEvent>,
) {
    section(ui, "FIRMWARE", |ui| {
        if !firmware.loaded {
            ui.label(egui::RichText::new("NO FIRMWARE LOADED").color(theme::TEXT_DIM));
            if ui.button("LOAD MARLIN (RAMPS 1.4)").clicked() {
                firmware.load_hex(MARLIN_RAMPS14_HEX, state);
            }
            return;
        }

        ui.horizontal(|ui| {
            let label = if firmware.playing {
                "|| PAUSE"
            } else {
                "▶ RESUME"
            };
            if ui.button(label).clicked() {
                firmware.playing = !firmware.playing;
                sfx.write(SfxEvent::Click);
            }
            ui.label(format!(
                "HOTEND {:.1}C   BED {:.1}C",
                firmware.hotend_c, firmware.bed_c
            ));
        });
        if firmware.playing {
            ui.label(format!("t = {:.1}s (LIVE)", state.time));
        } else {
            ui.add(
                egui::Slider::new(&mut state.time, 0.0..=state.total_time.max(0.001))
                    .text("TIME (S)"),
            );
        }
        layer_readout(ui, state, layer_visuals);
    });

    if !firmware.loaded {
        return;
    }

    section(ui, "SEND GCODE", |ui| {
        ui.horizontal_wrapped(|ui| {
            for &(name, contents) in EXAMPLES {
                if ui.button(name).clicked() {
                    firmware.send_gcode(contents);
                    sfx.write(SfxEvent::Beep);
                }
            }
            if ui.button("BROWSE").clicked() {
                crate::file_picker::spawn_file_pick(pending_pick, true);
                sfx.write(SfxEvent::Beep);
            }
        });
    });

    section(ui, "SERIAL CONSOLE", |ui| {
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
    });
}
