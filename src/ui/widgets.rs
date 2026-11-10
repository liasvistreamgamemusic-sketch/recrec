//! Small custom widgets: level meter and clip LED.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::meter::{METER_FLOOR_DB, MeterDisplay};

const GREEN_BELOW_DB: f32 = -18.0;
const YELLOW_BELOW_DB: f32 = -6.0;
const GREEN: Color32 = Color32::from_rgb(64, 190, 96);
const YELLOW: Color32 = Color32::from_rgb(230, 190, 60);
const RED: Color32 = Color32::from_rgb(230, 70, 60);
pub const REC_RED: Color32 = Color32::from_rgb(220, 50, 47);

fn x_for_db(rect: Rect, db: f32) -> f32 {
    let t = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
    rect.left() + t * rect.width()
}

/// Horizontal dBFS meter with colored zones and a peak-hold tick.
pub fn meter(ui: &mut Ui, display: &MeterDisplay, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

    let level_x = x_for_db(rect, display.level_db);
    let zones = [
        (METER_FLOOR_DB, GREEN_BELOW_DB, GREEN),
        (GREEN_BELOW_DB, YELLOW_BELOW_DB, YELLOW),
        (YELLOW_BELOW_DB, 0.0, RED),
    ];
    for (from, to, color) in zones {
        let left = x_for_db(rect, from);
        let right = x_for_db(rect, to).min(level_x);
        if right > left {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom())),
                0.0,
                color,
            );
        }
    }
    if display.hold_db > METER_FLOOR_DB {
        let x = x_for_db(rect, display.hold_db);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, ui.visuals().strong_text_color()),
        );
    }
    response.on_hover_text(format!("{:.1} dBFS", display.hold_db))
}

/// Clip indicator; click to clear.
pub fn clip_led(ui: &mut Ui, clipped: bool) -> Response {
    let size = Vec2::splat(ui.spacing().interact_size.y * 0.6);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let color = if clipped {
        RED
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    ui.painter()
        .circle_filled(rect.center(), rect.width() / 2.0, color);
    response.on_hover_text(if clipped {
        "クリップしました (クリックで消去)"
    } else {
        "クリップなし"
    })
}

/// Peak-hold readout such as "-12.3".
pub fn db_label(db: f32) -> String {
    if db <= METER_FLOOR_DB {
        "-inf".to_owned()
    } else {
        format!("{db:.1}")
    }
}

/// Round record/stop button.
pub fn record_button(ui: &mut Ui, recording: bool, enabled: bool) -> Response {
    let size = Vec2::splat(56.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let visuals = ui.visuals();
    let ring = if response.hovered() && enabled {
        visuals.strong_text_color()
    } else {
        visuals.widgets.inactive.fg_stroke.color
    };
    let painter = ui.painter();
    painter.circle_stroke(
        rect.center(),
        rect.width() / 2.0 - 1.0,
        Stroke::new(2.0, ring),
    );
    let fill = if enabled {
        REC_RED
    } else {
        visuals.widgets.inactive.bg_fill
    };
    if recording {
        let half = rect.width() * 0.18;
        painter.rect_filled(
            Rect::from_center_size(rect.center(), Vec2::splat(half * 2.0)),
            3.0,
            fill,
        );
    } else {
        painter.circle_filled(rect.center(), rect.width() * 0.3, fill);
    }
    let response = response.on_hover_text(if recording {
        "停止 (Space)"
    } else {
        "録音 (Space)"
    });
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
