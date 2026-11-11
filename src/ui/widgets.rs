//! Custom-painted widgets: record button, meters, toggles, chips and small text helpers.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use super::theme::{self, icon};
use crate::meter::{CLIP_THRESHOLD, METER_FLOOR_DB, MeterDisplay};
use crate::waveform::{Extent, WaveHistory};

const GREEN_BELOW_DB: f32 = -18.0;
const YELLOW_BELOW_DB: f32 = -6.0;
/// The meter is cut into LED-like segments this many dB wide.
const SEGMENT_DB: f32 = 2.0;
pub const RECORD_BUTTON_SIZE: f32 = 88.0;
const ICON_BUTTON_SIZE: f32 = 28.0;
const SEGMENTED_HEIGHT: f32 = 30.0;

/// Keyboard-focus ring: the one place the accent color always appears.
fn focus_stroke() -> Stroke {
    Stroke::new(2.0, theme::ACCENT)
}

/// Height fraction (0 = floor, 1 = full scale) of a level in dB.
fn level_fraction(db: f32) -> f32 {
    ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn paint_vmeter_bar(ui: &Ui, rect: Rect, display: &MeterDisplay) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(2), theme::SURFACE_3);
    let y_for = |db: f32| rect.bottom() - level_fraction(db) * rect.height();
    let level_y = y_for(display.level_db);
    let zones = [
        (METER_FLOOR_DB, GREEN_BELOW_DB, theme::METER_LOW),
        (GREEN_BELOW_DB, YELLOW_BELOW_DB, theme::METER_MID),
        (YELLOW_BELOW_DB, 0.0, theme::METER_HIGH),
    ];
    for (from, to, color) in zones {
        let bottom = y_for(from);
        let top = y_for(to).max(level_y);
        if bottom > top {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), bottom)),
                0.0,
                color,
            );
        }
    }
    // Segment gaps give the bar its LED look and make level easier to judge at a glance.
    let mut db = METER_FLOOR_DB + SEGMENT_DB;
    while db < 0.0 {
        let y = y_for(db).round();
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, theme::SURFACE_1),
        );
        db += SEGMENT_DB;
    }
    if display.hold_db > METER_FLOOR_DB {
        let y = y_for(display.hold_db);
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(2.0, theme::INK),
        );
    }
}

/// Vertical level meter, one bar per channel side by side.
pub fn vmeter(ui: &mut Ui, channels: &[&MeterDisplay], size: Vec2) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let gap = 2.0;
    let n = channels.len().max(1) as f32;
    let bar_width = (rect.width() - gap * (n - 1.0)) / n;
    let mut hold = METER_FLOOR_DB;
    for (i, display) in channels.iter().enumerate() {
        let left = rect.left() + i as f32 * (bar_width + gap);
        let bar = Rect::from_min_size(
            Pos2::new(left, rect.top()),
            Vec2::new(bar_width, rect.height()),
        );
        paint_vmeter_bar(ui, bar, display);
        hold = hold.max(display.hold_db);
    }
    response.on_hover_text(format!("ピーク {} dBFS", db_label(hold)));
}

/// What to draw in a [`waveform`] lane besides the samples.
#[derive(Debug, Default)]
pub struct WaveOverlay {
    /// First column (0 = oldest) that belongs to the take being recorded.
    pub recording_from: Option<usize>,
    /// Columns of bar lines and of the lighter beat lines between them.
    pub bars: Vec<usize>,
    pub beats: Vec<usize>,
    pub markers: Vec<usize>,
}

/// Scrolling waveform: one lane per channel, newest audio at the right edge.
pub fn waveform(
    ui: &mut Ui,
    size: Vec2,
    waves: &WaveHistory,
    channels: &[usize],
    blocks: usize,
    overlay: &dyn Fn(usize) -> WaveOverlay,
) -> Rect {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return rect;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_XS), theme::SURFACE_2);
    let columns = rect.width().floor().max(1.0) as usize;
    let overlay = overlay(columns);
    let x_of = |col: usize| rect.left() + col as f32 + 0.5;
    let vline = |col: usize, color: Color32| {
        painter.line_segment(
            [
                Pos2::new(x_of(col), rect.top()),
                Pos2::new(x_of(col), rect.bottom()),
            ],
            Stroke::new(1.0, color),
        );
    };
    for col in &overlay.beats {
        vline(*col, theme::HAIRLINE);
    }
    for col in &overlay.bars {
        vline(*col, theme::HAIRLINE_STRONG);
    }

    let lanes = channels.len().max(1) as f32;
    let lane_height = rect.height() / lanes;
    for (lane, channel) in channels.iter().enumerate() {
        let mid = rect.top() + lane_height * (lane as f32 + 0.5);
        let half = lane_height / 2.0 - 1.0;
        painter.line_segment(
            [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
            Stroke::new(1.0, theme::HAIRLINE),
        );
        for (col, extent) in waves.columns(*channel, blocks, columns).iter().enumerate() {
            let Some(extent) = extent else { continue };
            let recorded = overlay.recording_from.is_some_and(|from| col >= from);
            let clipped = extent.max >= CLIP_THRESHOLD || extent.min <= -CLIP_THRESHOLD;
            let color = if clipped {
                theme::WARN
            } else if recorded {
                theme::REC
            } else {
                theme::INK_TERTIARY
            };
            let top = mid - extent.max.clamp(-1.0, 1.0) * half;
            let bottom = mid - extent.min.clamp(-1.0, 1.0) * half;
            // Keep even silence visible as a thin trace.
            let (top, bottom) = if bottom - top < 1.0 {
                (mid - 0.5, mid + 0.5)
            } else {
                (top, bottom)
            };
            painter.line_segment(
                [Pos2::new(x_of(col), top), Pos2::new(x_of(col), bottom)],
                Stroke::new(1.0, color),
            );
        }
    }
    for col in &overlay.markers {
        vline(*col, theme::INK);
        painter.text(
            Pos2::new(x_of(*col) + 2.0, rect.top() + 1.0),
            Align2::LEFT_TOP,
            icon::FLAG,
            theme::icons(11.0),
            theme::INK,
        );
    }
    rect
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

    if response.has_focus() {
        painter.circle_stroke(center, outer + 4.0, focus_stroke());
    }
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
    if response.has_focus() {
        painter.circle_stroke(center, 14.0, focus_stroke());
    }
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
        theme::icons(theme::ICON_SIZE),
        theme::mix(base, target, hover),
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            focus_stroke(),
            StrokeKind::Inside,
        );
    }
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
    let job = theme::with_icon(
        icon::FILE_AUDIO,
        name,
        theme::regular(theme::TEXT_SMALL + 0.5),
        theme::INK_MUTED,
    );
    let galley = ui.painter().layout_job(job);
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
    ui.painter()
        .galley(rect.min + padding, galley, theme::INK_MUTED);
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

/// Small square toggle (monitor / mute / solo). `tint` is its color when on.
pub fn toggle_chip(ui: &mut Ui, label: &str, is_icon: bool, on: bool, tint: Color32) -> Response {
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), sense);
    let ctx = ui.ctx();
    let on_t = theme::animate(ctx, response.id.with("on"), on, theme::DURATION_FAST);
    let hover = theme::animate(
        ctx,
        response.id.with("hover"),
        ui.is_enabled() && response.hovered(),
        theme::DURATION_FAST,
    );
    let painter = ui.painter();
    let idle_fill = theme::mix(theme::SURFACE_2, theme::SURFACE_4, hover);
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        theme::mix(idle_fill, theme::mix(theme::SURFACE_1, tint, 0.28), on_t),
        Stroke::new(1.0, theme::mix(theme::HAIRLINE, tint, on_t)),
        StrokeKind::Inside,
    );
    let font = if is_icon {
        theme::icons(14.0)
    } else {
        theme::medium(theme::TEXT_SMALL)
    };
    let idle_text = if ui.is_enabled() {
        theme::INK_SUBTLE
    } else {
        theme::INK_TERTIARY.gamma_multiply(0.6)
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        theme::mix(idle_text, tint, on_t),
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            focus_stroke(),
            StrokeKind::Inside,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// On/off switch with a sliding knob. Returns true when it was flipped.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(34.0, 20.0), sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = theme::animate(ui.ctx(), response.id.with("on"), *on, theme::DURATION_FAST);
    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect(
        rect,
        radius,
        theme::mix(theme::SURFACE_3, theme::ACCENT, t),
        Stroke::new(
            1.0,
            theme::mix(theme::HAIRLINE_STRONG, theme::ACCENT_HOVER, t),
        ),
        StrokeKind::Inside,
    );
    let x = theme::lerp(rect.left() + radius, rect.right() - radius, t);
    painter.circle_filled(
        Pos2::new(x, rect.center().y),
        radius - 3.0,
        theme::mix(theme::INK_SUBTLE, theme::INK, t),
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            radius + 2.0,
            focus_stroke(),
            StrokeKind::Outside,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// One dot per beat of the bar; the current beat lights up and fades until the next one.
pub fn beat_lights(ui: &mut Ui, beats_per_bar: u32, current: Option<(u32, f32)>) {
    let dot = 8.0;
    let gap = 5.0;
    let width = beats_per_bar as f32 * (dot + gap) - gap;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    for beat in 0..beats_per_bar {
        let center = Pos2::new(
            rect.left() + dot / 2.0 + beat as f32 * (dot + gap),
            rect.center().y,
        );
        let glow = match current {
            Some((b, phase)) if b == beat => {
                if theme::reduced_motion() {
                    1.0
                } else {
                    (1.0 - phase).powi(2)
                }
            }
            _ => 0.0,
        };
        // The downbeat is marked by size, the running beat by brightness.
        let radius = if beat == 0 {
            dot / 2.0
        } else {
            dot / 2.0 - 1.0
        };
        ui.painter().circle_filled(
            center,
            radius + glow,
            theme::mix(theme::SURFACE_4, theme::INK, glow),
        );
    }
}

/// A lane of an opened take: its waveform for the visible window, markers and the playhead.
/// Click or drag to move the playhead (the response carries the pointer).
pub fn take_lane(
    ui: &mut Ui,
    size: Vec2,
    lanes: &[Vec<Option<Extent>>],
    playhead_px: Option<f32>,
    marker_px: &[f32],
    empty_text: Option<&str>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_XS), theme::SURFACE_2);
    if let Some(text) = empty_text {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            theme::regular(theme::TEXT_SMALL),
            theme::INK_TERTIARY,
        );
    }
    let lane_height = rect.height() / lanes.len().max(1) as f32;
    for (lane, columns) in lanes.iter().enumerate() {
        let mid = rect.top() + lane_height * (lane as f32 + 0.5);
        let half = lane_height / 2.0 - 1.0;
        painter.line_segment(
            [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
            Stroke::new(1.0, theme::HAIRLINE),
        );
        for (col, extent) in columns.iter().enumerate() {
            let Some(extent) = extent else { continue };
            let x = rect.left() + col as f32 + 0.5;
            let played = playhead_px.is_some_and(|p| (col as f32) < p);
            let color = if extent.max >= CLIP_THRESHOLD || extent.min <= -CLIP_THRESHOLD {
                theme::WARN
            } else if played {
                theme::INK_MUTED
            } else {
                theme::INK_SUBTLE
            };
            let top = mid - extent.max.clamp(-1.0, 1.0) * half;
            let bottom = mid - extent.min.clamp(-1.0, 1.0) * half;
            let (top, bottom) = if bottom - top < 1.0 {
                (mid - 0.5, mid + 0.5)
            } else {
                (top, bottom)
            };
            painter.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bottom)],
                Stroke::new(1.0, color),
            );
        }
    }
    for px in marker_px
        .iter()
        .filter(|px| (0.0..=rect.width()).contains(*px))
    {
        let x = rect.left() + px;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, theme::INK_TERTIARY),
        );
    }
    if let Some(px) = playhead_px.filter(|px| (0.0..=rect.width()).contains(px)) {
        let x = rect.left() + px;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.5, theme::INK),
        );
    }
    response.on_hover_cursor(CursorIcon::Text)
}

/// What the user did on the [`time_ruler`].
#[derive(Debug, Default)]
pub struct RulerAction {
    /// Pixel (from the left of the lanes) the playhead was dragged or clicked to.
    pub seek_px: Option<f32>,
    /// Horizontal drag of the scroll thumb, in pixels of the scroll bar.
    pub scroll_fraction: Option<f32>,
}

/// Time scale above the lanes plus a scroll bar showing which part of the take is visible.
/// `left..right` is the horizontal extent of the lanes; times are in seconds.
pub fn time_ruler(
    ui: &mut Ui,
    left: f32,
    right: f32,
    view_start_secs: f64,
    secs_per_px: f64,
    total_secs: f64,
    playhead_px: Option<f32>,
) -> RulerAction {
    const TICK_ROW: f32 = 18.0;
    const BAR_ROW: f32 = 8.0;
    const MIN_LABEL_GAP: f64 = 72.0;
    let (row, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), TICK_ROW + BAR_ROW),
        Sense::hover(),
    );
    let width = (right - left).max(1.0);
    let ticks = Rect::from_min_max(
        Pos2::new(left, row.top()),
        Pos2::new(right, row.top() + TICK_ROW),
    );
    let bar = Rect::from_min_max(
        Pos2::new(left, ticks.bottom() + 2.0),
        Pos2::new(right, row.bottom() - 2.0),
    );
    let mut action = RulerAction::default();
    let painter = ui.painter_at(row);

    // Labels at a "round" interval wide enough not to collide.
    let steps = [
        0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ];
    let step = steps
        .into_iter()
        .find(|s| s / secs_per_px >= MIN_LABEL_GAP)
        .unwrap_or(1800.0);
    let mut t = (view_start_secs / step).ceil() * step;
    while t <= view_start_secs + f64::from(width) * secs_per_px && t <= total_secs {
        let x = left + ((t - view_start_secs) / secs_per_px) as f32;
        painter.line_segment(
            [
                Pos2::new(x, ticks.bottom() - 5.0),
                Pos2::new(x, ticks.bottom()),
            ],
            Stroke::new(1.0, theme::HAIRLINE_STRONG),
        );
        let label = if step < 1.0 {
            format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0)
        } else {
            format!("{}:{:02}", (t / 60.0) as u64, (t % 60.0) as u64)
        };
        painter.text(
            Pos2::new(x + 3.0, ticks.center().y - 1.0),
            Align2::LEFT_CENTER,
            label,
            theme::mono(10.0),
            theme::INK_TERTIARY,
        );
        t += step;
    }
    if let Some(px) = playhead_px.filter(|px| (0.0..=width).contains(px)) {
        let x = left + px;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 5.0, ticks.bottom() - 7.0),
                Pos2::new(x + 5.0, ticks.bottom() - 7.0),
                Pos2::new(x, ticks.bottom()),
            ],
            theme::INK,
            Stroke::NONE,
        ));
    }
    let ticks_response = ui.interact(
        ticks,
        ui.scope_id().with("ruler_ticks"),
        Sense::click_and_drag(),
    );
    if (ticks_response.clicked() || ticks_response.dragged())
        && let Some(pos) = ticks_response.interact_pointer_pos()
    {
        action.seek_px = Some((pos.x - left).clamp(0.0, width));
    }
    ticks_response.on_hover_cursor(CursorIcon::PointingHand);

    // Scroll bar: the thumb is the visible share of the take.
    painter.rect_filled(bar, CornerRadius::same(2), theme::SURFACE_2);
    let visible = (f64::from(width) * secs_per_px / total_secs.max(1e-9)).min(1.0) as f32;
    if visible < 1.0 {
        let start = (view_start_secs / total_secs) as f32;
        let thumb = Rect::from_min_max(
            Pos2::new(bar.left() + start * bar.width(), bar.top()),
            Pos2::new(bar.left() + (start + visible) * bar.width(), bar.bottom()),
        );
        let response = ui.interact(
            bar.expand2(Vec2::new(0.0, 3.0)),
            ui.scope_id().with("ruler_scroll"),
            Sense::click_and_drag(),
        );
        let hot = response.hovered() || response.dragged();
        painter.rect_filled(
            thumb,
            CornerRadius::same(2),
            if hot {
                theme::INK_SUBTLE
            } else {
                theme::HAIRLINE_STRONG
            },
        );
        if response.dragged() {
            action.scroll_fraction = Some(response.drag_delta().x / bar.width());
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Clicking beside the thumb centers the view there.
            let target = (pos.x - bar.left()) / bar.width() - visible / 2.0;
            action.scroll_fraction = Some(target - start);
        }
        response.on_hover_cursor(CursorIcon::Grab);
    }
    action
}

/// Round transport button (play, stop, pause…) with an icon.
pub fn transport_button(ui: &mut Ui, glyph: &str, tooltip: &str, active: bool) -> Response {
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), sense);
    let hover = theme::animate(
        ui.ctx(),
        response.id.with("hover"),
        ui.is_enabled() && response.hovered(),
        theme::DURATION_FAST,
    );
    let on = theme::animate(
        ui.ctx(),
        response.id.with("on"),
        active,
        theme::DURATION_FAST,
    );
    let painter = ui.painter();
    let fill = theme::mix(
        theme::mix(theme::SURFACE_3, theme::SURFACE_4, hover),
        theme::INK,
        on,
    );
    painter.circle_filled(rect.center(), 15.0, fill);
    painter.circle_stroke(
        rect.center(),
        15.0,
        Stroke::new(
            1.0,
            theme::mix(theme::HAIRLINE_STRONG, theme::INK_SUBTLE, hover),
        ),
    );
    let idle = if ui.is_enabled() {
        theme::INK_MUTED
    } else {
        theme::INK_TERTIARY.gamma_multiply(0.6)
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        theme::icons(theme::ICON_SIZE),
        theme::mix(theme::mix(idle, theme::INK, hover), theme::CANVAS, on),
    );
    if response.has_focus() {
        painter.circle_stroke(rect.center(), 17.0, focus_stroke());
    }
    response
        .on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}
