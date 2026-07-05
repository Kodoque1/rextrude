use bevy_egui::egui;

use crate::playback::PrintState;
use crate::theme;

/// How many source lines to show either side of the executing one.
const CONTEXT_LINES: usize = 5;

/// Codec-subtitle style stream of the gcode around the currently executing
/// command. The window follows the playhead; the active line is highlighted.
pub fn show(ui: &mut egui::Ui, state: &PrintState) {
    if state.source_lines.is_empty() {
        ui.label(egui::RichText::new("NO SOURCE STREAM").color(theme::TEXT_DIM));
        return;
    }

    // The segment being executed leads *to* the next event, so that event's
    // source line is the command in flight.
    let idx = state.current_index();
    let current = state
        .toolpath
        .get(idx + 1)
        .or_else(|| state.toolpath.get(idx))
        .map(|ev| ev.line as usize)
        .unwrap_or(0);

    let total = state.source_lines.len();
    let lo = current.saturating_sub(CONTEXT_LINES + 1);
    let hi = (current + CONTEXT_LINES).min(total);

    for line_no in lo..hi {
        let text = &state.source_lines[line_no];
        let is_current = line_no + 1 == current;
        let (marker, color) = if is_current {
            ("▶", theme::TEXT)
        } else {
            (" ", theme::TEXT_DIM)
        };
        let row = egui::RichText::new(format!("{marker}{:>4} {text}", line_no + 1))
            .monospace()
            .size(16.0)
            .color(color);
        if is_current {
            let frame = egui::Frame::new()
                .fill(theme::BG_WIDGET)
                .inner_margin(egui::Margin::symmetric(2, 0));
            frame.show(ui, |ui| {
                ui.add(egui::Label::new(row).truncate());
            });
        } else {
            ui.add(egui::Label::new(row).truncate());
        }
    }
}
