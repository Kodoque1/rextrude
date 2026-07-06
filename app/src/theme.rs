use std::sync::Arc;

use bevy_egui::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
};

/// MGS codec palette.
pub const BG: Color32 = Color32::from_rgb(10, 26, 15);
pub const BG_PANEL: Color32 = Color32::from_rgb(6, 16, 10);
pub const BG_WIDGET: Color32 = Color32::from_rgb(14, 36, 21);
pub const TEXT: Color32 = Color32::from_rgb(102, 255, 153);
pub const TEXT_DIM: Color32 = Color32::from_rgb(58, 140, 88);
pub const STROKE: Color32 = Color32::from_rgb(30, 77, 43);
pub const STROKE_HOVER: Color32 = Color32::from_rgb(46, 125, 70);
pub const STROKE_ACTIVE: Color32 = Color32::from_rgb(69, 179, 107);
pub const ALERT_RED: Color32 = Color32::from_rgb(196, 38, 30);

/// Installs VT323 and the codec-green style. Called once per context
/// (guarded by a `Local<bool>` at the call site).
pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "vt323".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../fonts/VT323-Regular.ttf"
        ))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "vt323".into());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        // VT323 renders small for its point size; scale everything up.
        style.text_styles = [
            (egui::TextStyle::Heading, FontId::proportional(26.0)),
            (egui::TextStyle::Body, FontId::proportional(19.0)),
            (egui::TextStyle::Monospace, FontId::monospace(19.0)),
            (egui::TextStyle::Button, FontId::proportional(19.0)),
            (egui::TextStyle::Small, FontId::proportional(15.0)),
        ]
        .into();

        let v = &mut style.visuals;
        v.dark_mode = true;
        v.override_text_color = Some(TEXT);
        v.panel_fill = BG_PANEL;
        v.window_fill = BG;
        v.extreme_bg_color = Color32::from_rgb(4, 10, 6);
        v.faint_bg_color = BG_WIDGET;
        v.window_stroke = Stroke::new(1.0, STROKE);
        v.selection.bg_fill = STROKE_HOVER;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.hyperlink_color = STROKE_ACTIVE;

        for (widget, bg, stroke) in [
            (&mut v.widgets.noninteractive, BG_PANEL, STROKE),
            (&mut v.widgets.inactive, BG_WIDGET, STROKE),
            (&mut v.widgets.hovered, BG_WIDGET, STROKE_HOVER),
            (&mut v.widgets.active, STROKE, STROKE_ACTIVE),
            (&mut v.widgets.open, BG_WIDGET, STROKE_HOVER),
        ] {
            widget.bg_fill = bg;
            widget.weak_bg_fill = bg;
            widget.bg_stroke = Stroke::new(1.0, stroke);
            widget.fg_stroke = Stroke::new(1.0, TEXT);
            widget.corner_radius = CornerRadius::ZERO;
        }
        v.window_corner_radius = CornerRadius::ZERO;
        v.menu_corner_radius = CornerRadius::ZERO;
    });
}

/// CRT scanline + border overlay across the whole screen, drawn on a
/// foreground layer so it sits over panels and windows alike.
pub fn draw_scanlines(ctx: &egui::Context) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("crt_overlay"),
    ));
    let rect = ctx.viewport_rect();
    let line = Color32::from_black_alpha(46);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), y),
                egui::pos2(rect.right(), y + 1.0),
            ),
            0.0,
            line,
        );
        y += 3.0;
    }
    painter.rect_stroke(
        rect.shrink(1.0),
        0.0,
        Stroke::new(2.0, Color32::from_rgba_unmultiplied(30, 77, 43, 120)),
        egui::StrokeKind::Inside,
    );
}
