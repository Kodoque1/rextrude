use bevy_egui::egui;

use crate::kinematics::HeadVelocity;
use crate::playback::PrintState;
use crate::theme;

/// CNC-style digital readout: live gcode XYZ, executed feedrate, and
/// whether the head is laying filament right now.
pub fn show(ui: &mut egui::Ui, state: &PrintState, velocity: &HeadVelocity) {
    let Some((x, y, z)) = state.interpolated_position() else {
        ui.label(egui::RichText::new("NO SIGNAL").color(theme::TEXT_DIM));
        return;
    };

    for (axis, value) in [("X", x), ("Y", y), ("Z", z)] {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(axis)
                    .monospace()
                    .size(22.0)
                    .color(theme::TEXT_DIM),
            );
            ui.label(
                egui::RichText::new(format!("{value:>8.2}"))
                    .monospace()
                    .size(22.0)
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new("mm")
                    .monospace()
                    .color(theme::TEXT_DIM),
            );
        });
    }

    ui.separator();

    let idx = state.current_index();
    let extruding = state.playing && state.toolpath.get(idx + 1).is_some_and(|ev| ev.extruding);
    let moving = state.playing && velocity.mm_per_s > 0.05;
    let (status, color) = match (extruding, moving) {
        (true, _) => ("EXTRUDE", theme::STROKE_ACTIVE),
        (false, true) => ("TRAVEL", theme::TEXT),
        (false, false) => ("IDLE", theme::TEXT_DIM),
    };

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("F {:>6.1} mm/s", velocity.mm_per_s))
                .monospace()
                .size(20.0)
                .color(theme::TEXT),
        );
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(status)
                .monospace()
                .size(20.0)
                .strong()
                .color(color),
        );
    });
}
