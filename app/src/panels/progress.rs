use bevy_egui::egui;

use crate::layers::LayerVisuals;
use crate::playback::PrintState;
use crate::theme;

fn fmt_clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

/// Print progress: layer bar, elapsed/remaining wall-clock (at the current
/// playback speed), and filament consumed (cumulative E axis).
pub fn show(ui: &mut egui::Ui, state: &PrintState, layer_visuals: &LayerVisuals) {
    if state.toolpath.is_empty() {
        ui.label(egui::RichText::new("NO DATA").color(theme::TEXT_DIM));
        return;
    }

    let fraction = (state.time / state.total_time.max(0.001)) as f32;
    ui.add(
        egui::ProgressBar::new(fraction)
            .text(
                egui::RichText::new(format!("{:.0}%", fraction * 100.0))
                    .monospace()
                    .color(theme::TEXT),
            )
            .fill(theme::STROKE_HOVER),
    );

    let layer_count = layer_visuals.layer_count();
    if layer_count > 0 {
        let idx = state.current_index();
        let current = layer_visuals.layer_containing(idx).unwrap_or(0);
        ui.label(
            egui::RichText::new(format!("LAYER {:02} / {:02}", current + 1, layer_count))
                .monospace()
                .color(theme::TEXT),
        );
    }

    let speed = state.speed.max(0.01) as f64;
    ui.label(
        egui::RichText::new(format!(
            "ELAPSED   {}   (sim {})",
            fmt_clock(state.time / speed),
            fmt_clock(state.time),
        ))
        .monospace()
        .color(theme::TEXT_DIM),
    );
    ui.label(
        egui::RichText::new(format!(
            "REMAINING {}",
            fmt_clock((state.total_time - state.time) / speed)
        ))
        .monospace()
        .color(theme::TEXT),
    );

    let idx = state.current_index();
    let used = state.toolpath[idx].e.max(0.0);
    let total = state.toolpath.last().map(|ev| ev.e.max(0.0)).unwrap_or(0.0);
    ui.label(
        egui::RichText::new(format!("FILAMENT  {used:>7.1} / {total:.1} mm"))
            .monospace()
            .color(theme::TEXT),
    );
}
