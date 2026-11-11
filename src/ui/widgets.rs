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
        let start = waves.total_blocks() as f64 - blocks as f64;
        for (col, extent) in waves
            .columns(*channel, start, blocks as f64, columns)
            .iter()
            .enumerate()
        {
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
    grid_px: &[(f32, bool)],
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
    for (px, bar) in grid_px {
        let x = rect.left() + px;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(
                1.0,
                if *bar {
                    theme::HAIRLINE_STRONG
                } else {
                    theme::HAIRLINE
                },
            ),
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

/// Tempo grid for bar/beat display: seconds per beat and beats per bar. Bar 1 starts at 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatGrid {
    pub beat_secs: f64,
    pub beats_per_bar: u32,
}

impl BeatGrid {
    /// "bar.beat" (both 1-based) of a time, e.g. "12.3".
    pub fn label(&self, secs: f64) -> String {
        let beats = (secs.max(0.0) / self.beat_secs).floor() as u64;
        let per_bar = u64::from(self.beats_per_bar.max(1));
        format!("{}.{}", beats / per_bar + 1, beats % per_bar + 1)
    }

    /// Positions (in seconds) of the grid lines between `from` and `to`, each flagged as a
    /// bar line or not. Lines closer together than `min_gap_secs` are left out.
    pub fn lines(&self, from: f64, to: f64, min_gap_secs: f64) -> Vec<(f64, bool)> {
        let per_bar = u64::from(self.beats_per_bar.max(1));
        // Step in beats: every beat, every bar, or every 2/4/8… bars, whichever is wide enough.
        let mut step = 1u64;
        if self.beat_secs < min_gap_secs {
            step = per_bar;
            while self.beat_secs * (step as f64) < min_gap_secs {
                step *= 2;
            }
        }
        let mut lines = Vec::new();
        let mut k = ((from.max(0.0) / self.beat_secs / step as f64).ceil() as u64) * step;
        while (k as f64) * self.beat_secs <= to {
            lines.push(((k as f64) * self.beat_secs, k.is_multiple_of(per_bar)));
            k += step;
        }
        lines
    }
}

/// Time scale above the lanes. `(left, right)` is the horizontal extent of the lanes; with a
/// `grid` the labels are bars (and beats when zoomed in) instead of minutes and seconds.
/// Returns the pixel (from the left of the lanes) the playhead was clicked or dragged to.
pub fn time_ruler(
    ui: &mut Ui,
    (left, right): (f32, f32),
    view_start_secs: f64,
    secs_per_px: f64,
    total_secs: f64,
    playhead_px: Option<f32>,
    grid: Option<BeatGrid>,
) -> Option<f32> {
    const HEIGHT: f32 = 18.0;
    const MIN_LABEL_GAP: f64 = 64.0;
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let width = (right - left).max(1.0);
    let ticks = Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new(right, row.bottom()));
    let painter = ui.painter_at(row);
    let view_end_secs = (view_start_secs + f64::from(width) * secs_per_px).min(total_secs);
    let x_of = |t: f64| left + ((t - view_start_secs) / secs_per_px) as f32;
    let tick = |x: f32, tall: bool| {
        painter.line_segment(
            [
                Pos2::new(x, ticks.bottom() - if tall { 6.0 } else { 3.0 }),
                Pos2::new(x, ticks.bottom()),
            ],
            Stroke::new(1.0, theme::HAIRLINE_STRONG),
        );
    };
    let label = |x: f32, text: String| {
        painter.text(
            Pos2::new(x + 3.0, ticks.center().y - 1.0),
            Align2::LEFT_CENTER,
            text,
            theme::mono(10.0),
            theme::INK_TERTIARY,
        );
    };

    match grid {
        Some(grid) => {
            // Labeled lines far enough apart to read; unlabeled ticks for the finer grid.
            for (t, bar) in grid.lines(view_start_secs, view_end_secs, 6.0 * secs_per_px) {
                tick(x_of(t), bar);
            }
            let labeled = grid.lines(view_start_secs, view_end_secs, MIN_LABEL_GAP * secs_per_px);
            let beats_labeled = labeled.iter().any(|(_, bar)| !bar);
            for (t, _) in labeled {
                let text = grid.label(t + grid.beat_secs * 0.5);
                // Whole bars read better without the ".1".
                let text = if beats_labeled {
                    text
                } else {
                    text.split('.').next().unwrap_or_default().to_owned()
                };
                label(x_of(t), text);
            }
        }
        None => {
            let steps = [
                0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0,
            ];
            let step = steps
                .into_iter()
                .find(|s| s / secs_per_px >= MIN_LABEL_GAP)
                .unwrap_or(3600.0);
            let mut t = (view_start_secs.max(0.0) / step).ceil() * step;
            while t <= view_end_secs {
                tick(x_of(t), true);
                let text = if step < 1.0 {
                    format!("{}:{:04.1}", (t / 60.0) as u64, t % 60.0)
                } else {
                    format!("{}:{:02}", (t / 60.0) as u64, (t % 60.0) as u64)
                };
                label(x_of(t), text);
                t += step;
            }
        }
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
    let response = ui.interact(
        ticks,
        ui.scope_id().with("ruler_ticks"),
        Sense::click_and_drag(),
    );
    let seek = (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pos| (pos.x - left).clamp(0.0, width));
    response.on_hover_cursor(CursorIcon::PointingHand);
    seek
}

/// Which part of the overview window a drag holds on to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// The whole window, held this many pixels right of its left edge.
    Move(f32),
    Left,
    Right,
}

/// Miniature of the whole take with a window marking the part shown in the lanes. Drag the
/// window to scroll, drag its edges to zoom, click elsewhere to jump there. `window` and the
/// result are fractions (0..1) of the take.
pub fn overview(
    ui: &mut Ui,
    (left, right): (f32, f32),
    columns: &[Option<Extent>],
    window: (f64, f64),
    playhead: Option<f64>,
) -> Option<(f64, f64)> {
    const HEIGHT: f32 = 26.0;
    const EDGE: f32 = 6.0;
    const MIN_WINDOW_PX: f32 = 8.0;
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let rect = Rect::from_min_max(
        Pos2::new(left, row.top() + 2.0),
        Pos2::new(right, row.bottom() - 2.0),
    );
    let width = rect.width().max(1.0);
    let id = ui.scope_id().with("overview");
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_XS), theme::SURFACE_2);

    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 2.0;
    for (col, extent) in columns.iter().enumerate() {
        let Some(extent) = extent else { continue };
        let x = rect.left() + col as f32 + 0.5;
        let reach = extent.max.abs().max(extent.min.abs()).clamp(0.0, 1.0) * half;
        painter.line_segment(
            [
                Pos2::new(x, mid - reach.max(0.5)),
                Pos2::new(x, mid + reach.max(0.5)),
            ],
            Stroke::new(1.0, theme::INK_TERTIARY),
        );
    }

    let x_left = rect.left() + window.0 as f32 * width;
    let x_right = rect.left() + window.1 as f32 * width;
    let shown = Rect::from_min_max(
        Pos2::new(x_left, rect.top()),
        Pos2::new(x_right.max(x_left + 2.0), rect.bottom()),
    );
    let hot = response.hovered() || response.dragged();
    painter.rect(
        shown,
        CornerRadius::same(theme::RADIUS_XS),
        theme::INK.gamma_multiply(if hot { 0.14 } else { 0.08 }),
        Stroke::new(
            1.0,
            if hot {
                theme::INK_MUTED
            } else {
                theme::INK_SUBTLE
            },
        ),
        StrokeKind::Inside,
    );
    if let Some(p) = playhead.filter(|p| (0.0..=1.0).contains(p)) {
        let x = rect.left() + p as f32 * width;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, theme::INK),
        );
    }

    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let part_at = |x: f32| -> Option<Grab> {
        if (x - x_left).abs() <= EDGE {
            Some(Grab::Left)
        } else if (x - x_right).abs() <= EDGE {
            Some(Grab::Right)
        } else if (x_left..=x_right).contains(&x) {
            Some(Grab::Move(x - x_left))
        } else {
            None
        }
    };
    if let Some(pos) = pointer {
        ui.ctx().set_cursor_icon(match part_at(pos.x) {
            Some(Grab::Left | Grab::Right) => CursorIcon::ResizeHorizontal,
            Some(Grab::Move(_)) => CursorIcon::Grab,
            None => CursorIcon::PointingHand,
        });
    }

    let length = (window.1 - window.0).max(0.0);
    let fraction = |x: f32| f64::from(((x - rect.left()) / width).clamp(0.0, 1.0));
    let centered = |x: f32| {
        let start = (fraction(x) - length / 2.0).clamp(0.0, (1.0 - length).max(0.0));
        (start, start + length)
    };
    let mut result = None;
    if response.drag_started()
        && let Some(pos) = pointer
    {
        // Pressing outside the window first brings the window there, then drags it.
        let grab = part_at(pos.x).unwrap_or(Grab::Move(length as f32 * width / 2.0));
        if part_at(pos.x).is_none() {
            result = Some(centered(pos.x));
        }
        ui.data_mut(|d| d.insert_temp(id, grab));
    }
    if response.dragged()
        && let Some(pos) = pointer
        && let Some(grab) = ui.data(|d| d.get_temp::<Grab>(id))
    {
        let min_len = f64::from(MIN_WINDOW_PX / width);
        result = Some(match grab {
            Grab::Move(offset) => {
                let start = fraction(pos.x - offset).min((1.0 - length).max(0.0));
                (start, start + length)
            }
            Grab::Left => (fraction(pos.x).min(window.1 - min_len).max(0.0), window.1),
            Grab::Right => (window.0, fraction(pos.x).max(window.0 + min_len).min(1.0)),
        });
    } else if response.clicked()
        && let Some(pos) = pointer
        && part_at(pos.x).is_none()
    {
        result = Some(centered(pos.x));
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: BeatGrid = BeatGrid {
        beat_secs: 0.5,
        beats_per_bar: 4,
    };

    #[test]
    fn bar_and_beat_labels_are_one_based() {
        assert_eq!(GRID.label(0.0), "1.1");
        assert_eq!(GRID.label(0.49), "1.1");
        assert_eq!(GRID.label(0.5), "1.2");
        assert_eq!(GRID.label(2.0), "2.1");
        assert_eq!(GRID.label(23.6), "12.4");
    }

    #[test]
    fn grid_lines_thin_out_as_the_view_zooms_out() {
        // Zoomed in: every beat, bars flagged.
        assert_eq!(
            GRID.lines(0.0, 2.0, 0.1),
            vec![
                (0.0, true),
                (0.5, false),
                (1.0, false),
                (1.5, false),
                (2.0, true)
            ]
        );
        // Beats too close: bars only.
        assert_eq!(
            GRID.lines(1.0, 6.0, 1.0),
            vec![(2.0, true), (4.0, true), (6.0, true)]
        );
        // Bars too close: every second, fourth… bar.
        assert_eq!(
            GRID.lines(0.0, 16.0, 5.0),
            vec![(0.0, true), (8.0, true), (16.0, true)]
        );
        // Nothing before time zero.
        assert_eq!(GRID.lines(-3.0, 0.6, 0.1), vec![(0.0, true), (0.5, false)]);
    }
}
