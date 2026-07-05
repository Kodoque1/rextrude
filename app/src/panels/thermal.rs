use bevy_egui::egui;

use crate::playback::PrintState;
use crate::theme;

/// Seconds of history shown in the scrolling graph window.
const WINDOW_S: f64 = 60.0;
const GRAPH_HEIGHT: f32 = 130.0;

/// Hotend + bed temperature graph with target lines, scrolling with the
/// playhead. Temperature is a pure function of playback time, so the time
/// slider scrubs this panel like everything else.
pub fn show(ui: &mut egui::Ui, state: &PrintState) {
    let Some(current) = state.thermal_at(state.time) else {
        ui.label(egui::RichText::new("NO THERMAL DATA").color(theme::TEXT_DIM));
        return;
    };

    let readout = |name: &str, temp: f32, target: f32| {
        let (status, color) = if target <= 0.0 {
            ("--", theme::TEXT_DIM)
        } else if (temp - target).abs() <= 3.0 {
            ("STABLE", theme::STROKE_ACTIVE)
        } else if temp < target {
            ("HEATING", theme::ALERT_RED)
        } else {
            ("COOLING", theme::TEXT_DIM)
        };
        let target_text = if target > 0.0 {
            format!("{target:>3.0}")
        } else {
            "---".to_string()
        };
        (
            format!("{name} {temp:>5.1}C / {target_text}"),
            status,
            color,
        )
    };

    for (label, status, color) in [
        readout("HOT", current.hotend_c, current.hotend_target),
        readout("BED", current.bed_c, current.bed_target),
    ] {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(18.0)
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new(status)
                    .monospace()
                    .size(18.0)
                    .strong()
                    .color(color),
            );
        });
    }

    if state.thermal.len() < 2 {
        return;
    }

    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width(), GRAPH_HEIGHT),
        egui::Sense::hover(),
    );
    let rect = response.rect.shrink(2.0);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(4, 10, 6));
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, theme::STROKE),
        egui::StrokeKind::Inside,
    );

    let t_max = state.time.max(WINDOW_S);
    let t_min = t_max - WINDOW_S;
    let max_temp = state
        .thermal
        .iter()
        .map(|s| s.hotend_c.max(s.hotend_target))
        .fold(100.0_f32, f32::max)
        + 25.0;

    let to_pos = |t: f64, temp: f32| {
        egui::pos2(
            rect.left() + ((t - t_min) / WINDOW_S) as f32 * rect.width(),
            rect.bottom() - (temp / max_temp).clamp(0.0, 1.0) * rect.height(),
        )
    };

    // Grid: 50C horizontals, 15s verticals.
    let grid = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(30, 77, 43, 90));
    let mut temp = 50.0;
    while temp < max_temp {
        painter.line_segment([to_pos(t_min, temp), to_pos(t_max, temp)], grid);
        temp += 50.0;
    }
    let mut t = (t_min / 15.0).ceil() * 15.0;
    while t <= t_max {
        painter.line_segment([to_pos(t, 0.0), to_pos(t, max_temp)], grid);
        t += 15.0;
    }

    // Visible slice of the timeline, clipped to [t_min, t_max].
    let start = state.thermal.partition_point(|s| s.t < t_min);
    let end = state.thermal.partition_point(|s| s.t <= state.time);
    let slice = &state.thermal[start.saturating_sub(1)..end];

    for (bed, color) in [(true, theme::STROKE_ACTIVE), (false, theme::TEXT)] {
        // Dashed target line at the latest target in view.
        let target = if bed {
            current.bed_target
        } else {
            current.hotend_target
        };
        if target > 0.0 {
            painter.add(egui::Shape::dashed_line(
                &[to_pos(t_min, target), to_pos(t_max, target)],
                egui::Stroke::new(1.0, theme::TEXT_DIM),
                6.0,
                4.0,
            ));
        }

        let mut points: Vec<egui::Pos2> = slice
            .iter()
            .map(|s| to_pos(s.t.max(t_min), if bed { s.bed_c } else { s.hotend_c }))
            .collect();
        points.push(to_pos(
            state.time,
            if bed { current.bed_c } else { current.hotend_c },
        ));
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, color)));
    }

    painter.text(
        rect.left_bottom() + egui::vec2(4.0, -4.0),
        egui::Align2::LEFT_BOTTOM,
        "HOT ─ bright   BED ─ dim   60s",
        egui::FontId::monospace(12.0),
        theme::TEXT_DIM,
    );
}
