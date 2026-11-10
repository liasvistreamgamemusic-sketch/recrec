//! Custom-painted widgets: record button, meters, toggles, chips and small text helpers.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use super::theme::{self, icon};
use crate::meter::{METER_FLOOR_DB, MeterDisplay};

const GREEN_BELOW_DB: f32 = -18.0;
const YELLOW_BELOW_DB: f32 = -6.0;
/// The meter is cut into LED-like segments this many dB wide.
const SEGMENT_DB: f32 = 2.0;
pub const RECORD_BUTTON_SIZE: f32 = 88.0;
const ICON_BUTTON_SIZE: f32 = 28.0;
const SEGMENTED_HEIGHT: f32 = 30.0;

fn x_for_db(rect: Rect, db: f32) -> f32 {
    let t = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
    rect.left() + t * rect.width()
}

fn paint_meter_bar(ui: &Ui, rect: Rect, display: &MeterDisplay) {
    let painter = ui.painter_at(rect);
    let radius = CornerRadius::same(2);
    painter.rect_filled(rect, radius, theme::SURFACE_3);

    let level_x = x_for_db(rect, display.level_db);
    let zones = [
        (METER_FLOOR_DB, GREEN_BELOW_DB, theme::METER_LOW),
        (GREEN_BELOW_DB, YELLOW_BELOW_DB, theme::METER_MID),
        (YELLOW_BELOW_DB, 0.0, theme::METER_HIGH),
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
    // Segment gaps give the bar its LED look and make level easier to judge at a glance.
    let mut db = METER_FLOOR_DB + SEGMENT_DB;
    while db < 0.0 {
        let x = x_for_db(rect, db).round();
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, theme::SURFACE_1),
        );
        db += SEGMENT_DB;
    }
    if display.hold_db > METER_FLOOR_DB {
        let x = x_for_db(rect, display.hold_db);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, theme::INK),
        );
    }
}

/// One bar per channel (stacked for stereo) inside `size`. Returns the allocated rect.
pub fn meter(ui: &mut Ui, channels: &[&MeterDisplay], size: Vec2) -> Rect {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let gap = 2.0;
    let n = channels.len().max(1) as f32;
    let bar_height = (rect.height() - gap * (n - 1.0)) / n;
    let mut hold = METER_FLOOR_DB;
    for (i, display) in channels.iter().enumerate() {
        let top = rect.top() + i as f32 * (bar_height + gap);
        let bar = Rect::from_min_size(
            Pos2::new(rect.left(), top),
            Vec2::new(rect.width(), bar_height),
        );
        paint_meter_bar(ui, bar, display);
        hold = hold.max(display.hold_db);
    }
    response.on_hover_text(format!("ピーク {} dBFS", db_label(hold)));
    rect
}

/// dB tick labels drawn above the meters, spanning `left..right`.
pub fn meter_scale(ui: &mut Ui, left: f32, right: f32) {
    let height = 14.0;
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let span = Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new(right, row.bottom()));
    for db in [-60.0f32, -48.0, -36.0, -24.0, -18.0, -12.0, -6.0, 0.0] {
        let align = if db == 0.0 {
            Align2::RIGHT_CENTER
        } else if db == METER_FLOOR_DB {
            Align2::LEFT_CENTER
        } else {
            Align2::CENTER_CENTER
        };
        ui.painter().text(
            Pos2::new(x_for_db(span, db), row.center().y),
            align,
            format!("{db:.0}"),
            theme::mono(10.0),
            theme::INK_TERTIARY,
        );
    }
}

/// Peak-hold readout such as "-12.3".
pub fn db_label(db: f32) -> String {
    if db <= METER_FLOOR_DB {
        "-inf".to_owned()
    } else {
        format!("{db:.1}")
    }
}

/// Right-aligned monospace dB readout in a fixed-width slot (so rows never jitter).
pub fn db_readout(ui: &mut Ui, db: f32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, ui.spacing().interact_size.y),
        Sense::hover(),
    );
    let color = if db >= YELLOW_BELOW_DB {
        theme::REC
    } else if db > METER_FLOOR_DB {
        theme::INK_MUTED
    } else {
        theme::INK_TERTIARY
    };
    ui.painter().text(
        rect.right_center(),
        Align2::RIGHT_CENTER,
        db_label(db),
        theme::mono(theme::TEXT_BODY),
        color,
    );
}

/// "CLIP" badge that fades in when a channel clipped; click to clear. Keeps its slot when hidden.
pub fn clip_badge(ui: &mut Ui, id: egui::Id, clipped: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, 20.0), Sense::click());
    let t = theme::animate(ui.ctx(), id, clipped, theme::DURATION_FAST);
    if t > 0.0 {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_XS),
            theme::mix(theme::SURFACE_1, theme::REC, 0.22 * t),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "CLIP",
            theme::medium(10.5),
            theme::REC.gamma_multiply(t),
        );
    }
    if clipped {
        response
            .on_hover_text("クリップしました (クリックで消去)")
            .on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// The hero transport button: a red dot that morphs into a stop square, with pulse rings
/// while recording.
pub fn record_button(ui: &mut Ui, recording: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(RECORD_BUTTON_SIZE), sense);
    let ctx = ui.ctx().clone();
    let id = response.id;
    let hover = theme::animate(
        &ctx,
        id.with("hover"),
        enabled && response.hovered(),
        theme::DURATION_FAST,
    );
    let press = theme::animate(
        &ctx,
        id.with("press"),
        response.is_pointer_button_down_on(),
        0.08,
    );
    let morph = theme::animate(&ctx, id.with("morph"), recording, theme::DURATION_BASE);
    let center = rect.center();
    let outer = RECORD_BUTTON_SIZE / 2.0 - 2.0;
    let painter = ui.painter();

    if recording && !theme::reduced_motion() {
        let time = ui.input(|i| i.time);
        for offset in [0.0, 0.5] {
            let phase = ((time / theme::PULSE_PERIOD + offset) % 1.0) as f32;
            painter.circle_stroke(
                center,
                outer + phase * 14.0,
                Stroke::new(1.5, theme::REC.gamma_multiply((1.0 - phase) * 0.55)),
            );
        }
    }

    let ring = theme::mix(
        theme::mix(theme::HAIRLINE_STRONG, theme::INK_SUBTLE, hover),
        theme::REC,
        morph,
    );
    painter.circle_filled(
        center,
        outer,
        theme::mix(theme::SURFACE_2, theme::SURFACE_3, hover),
    );
    painter.circle_stroke(center, outer, Stroke::new(1.5, ring));

    let fill = if enabled {
        theme::REC
    } else {
        theme::INK_TERTIARY
    };
    let scale = 1.0 + 0.06 * hover - 0.08 * press;
    let half = theme::lerp(outer * 0.56, outer * 0.36, morph) * scale;
    let corner = theme::lerp(half, 5.0, morph);
    painter.rect_filled(
        Rect::from_center_size(center, Vec2::splat(half * 2.0)),
        corner,
        fill,
    );

    let response = response.on_hover_text(if recording {
        "停止 (Space)"
    } else {
        "録音 (Space)"
    });
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Round arm toggle of a track: filled red when the track will be recorded.
pub fn arm_toggle(ui: &mut Ui, armed: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_BUTTON_SIZE), sense);
    let ctx = ui.ctx();
    let on = theme::animate(ctx, response.id.with("on"), armed, theme::DURATION_FAST);
    let hover = theme::animate(
        ctx,
        response.id.with("hover"),
        enabled && response.hovered(),
        theme::DURATION_FAST,
    );
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_filled(
        center,
        11.0,
        theme::mix(
            theme::SURFACE_3,
            theme::mix(theme::SURFACE_3, theme::REC, 0.22),
            on,
        ),
    );
    painter.circle_stroke(
        center,
        11.0,
        Stroke::new(
            1.0,
            theme::mix(
                theme::mix(theme::HAIRLINE_STRONG, theme::INK_SUBTLE, hover),
                theme::REC,
                on,
            ),
        ),
    );
    painter.circle_filled(
        center,
        theme::lerp(3.0, 5.0, on),
        theme::mix(theme::INK_TERTIARY, theme::REC, on),
    );
    let response = response.on_hover_text(if armed {
        "録音します (クリックで外す)"
    } else {
        "録音しません (クリックで録音対象に)"
    });
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Borderless square icon button; `danger` tints it red on hover.
pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str, danger: bool) -> Response {
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ICON_BUTTON_SIZE), sense);
    let hover = theme::animate(
        ui.ctx(),
        response.id.with("hover"),
        ui.is_enabled() && response.hovered(),
        theme::DURATION_FAST,
    );
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        theme::SURFACE_4.gamma_multiply(hover),
    );
    let target = if danger { theme::REC } else { theme::INK };
    let base = if ui.is_enabled() {
        theme::INK_SUBTLE
    } else {
        theme::INK_TERTIARY.gamma_multiply(0.5)
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        theme::regular(theme::ICON_SIZE),
        theme::mix(base, target, hover),
    );
    response
        .on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// Full-width segmented control with a sliding selection. Returns the newly picked index.
pub fn segmented(ui: &mut Ui, id: egui::Id, labels: &[&str], selected: usize) -> Option<usize> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SEGMENTED_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS_MD),
        theme::SURFACE_2,
        Stroke::new(1.0, theme::HAIRLINE),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(3.0);
    let cell = inner.width() / labels.len() as f32;
    let x = theme::animate_value(
        ui.ctx(),
        id.with("x"),
        inner.left() + cell * selected as f32,
        theme::DURATION_BASE,
    );
    painter.rect(
        Rect::from_min_size(Pos2::new(x, inner.top()), Vec2::new(cell, inner.height())),
        CornerRadius::same(theme::RADIUS_SM),
        theme::SURFACE_4,
        Stroke::new(1.0, theme::HAIRLINE_STRONG),
        StrokeKind::Inside,
    );
    let mut picked = None;
    for (i, label) in labels.iter().enumerate() {
        let cell_rect = Rect::from_min_size(
            Pos2::new(inner.left() + cell * i as f32, inner.top()),
            Vec2::new(cell, inner.height()),
        );
        let sense = if ui.is_enabled() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(cell_rect, id.with(i), sense);
        let color = if i == selected {
            theme::INK
        } else if response.hovered() {
            theme::INK_MUTED
        } else {
            theme::INK_SUBTLE
        };
        ui.painter().text(
            cell_rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::medium(theme::TEXT_SMALL + 1.0),
            color,
        );
        if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && i != selected {
            picked = Some(i);
        }
    }
    picked
}

/// A draggable file chip. Returns its response (check `drag_started`).
pub fn file_chip(ui: &mut Ui, name: &str, draggable: bool, tooltip: &str) -> Response {
    let font = theme::regular(theme::TEXT_SMALL + 0.5);
    let text = format!("{}  {name}", icon::FILE_AUDIO);
    let galley = ui.painter().layout_no_wrap(text, font, theme::INK_MUTED);
    let padding = Vec2::new(theme::SPACE_XS, 5.0);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::click_and_drag());
    let hover = theme::animate(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered() || response.dragged(),
        theme::DURATION_FAST,
    );
    ui.painter().rect(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        theme::mix(theme::SURFACE_2, theme::SURFACE_4, hover),
        Stroke::new(
            1.0,
            theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hover),
        ),
        StrokeKind::Inside,
    );
    ui.painter().galley_with_override_text_color(
        rect.min + padding,
        galley,
        theme::mix(theme::INK_MUTED, theme::INK, hover),
    );
    let response = response.on_hover_text(tooltip);
    if draggable {
        response.on_hover_cursor(CursorIcon::Grab)
    } else {
        response
    }
}

/// Small colored status badge.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::medium(10.5), color);
    let padding = Vec2::new(6.0, 3.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_XS),
        theme::mix(theme::SURFACE_1, color, 0.18),
    );
    ui.painter().galley(rect.min + padding, galley, color);
}

/// Quiet section label used in the side panels.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::medium(theme::TEXT_SMALL))
            .color(theme::INK_SUBTLE),
    );
}

/// Field caption placed above a control.
pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::regular(theme::TEXT_SMALL))
            .color(theme::INK_TERTIARY),
    );
}

/// Thin horizontal usage bar (0..1).
pub fn usage_bar(ui: &mut Ui, fraction: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), theme::SURFACE_3);
    let fraction = fraction.clamp(0.0, 1.0);
    let color = if fraction > 0.9 {
        theme::REC
    } else {
        theme::INK_SUBTLE
    };
    let mut filled = rect;
    filled.set_width(rect.width() * fraction);
    painter.rect_filled(filled, CornerRadius::same(2), color);
}
